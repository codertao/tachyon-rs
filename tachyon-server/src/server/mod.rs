//! The server: Netsock sessions, the engine layer per connection (startup stages, reliable
//! events, round-robin sends) and the game rules hooks. Single-threaded, ticked at 50 Hz.
//! References: `decomp/notes/multiplayer.md`, especially "Server-side startup details".

mod combat;
mod objects;
mod rules;

use crate::map::Map;
use crate::packetlog::PacketLog;
use crate::world::{distance, Entity, Owner, Power, World, ANGLE_MASK, FX};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::{Duration, Instant};
use tachyon_core::mpc::GameType;
use tachyon_core::net::proto::RecordOut;
use tachyon_core::net::events::{self as ev};
use std::rc::Rc;
use tachyon_core::des::Des;
use tachyon_core::net::netsock::{self, ClientMsg};
use tachyon_core::net::packet::{self, Outbox, PacketWriter, TagRing, BUDGET_RELIABLE};
use tachyon_core::net::proto::{self, parse_join_blob, parse_player_data, JoinInfo, StatusOut};
use tachyon_core::net::startup::{self, RosterEntry};
use tachyon_core::weapons;

pub const TICK: Duration = Duration::from_millis(20);
/// Netsock drops a silent connection after 120 s.
const TIMEOUT: Duration = Duration::from_secs(120);
/// Near / far interest radii (`Ent_SetQuadrant 0x4577d0`), 16.16.
const NEAR_RADIUS: i64 = 3750 * FX as i64;
const FAR_RADIUS: i64 = 10000 * FX as i64;
/// Explosion effect radii (`0x535a74`, units), indexed by effect.
const EFFECT_RADIUS: [i64; 12] = [256, 625, 1000, 1000, 5000, 1500, 1500, 5000, 1250, 1250, 1500, 625];
/// The protocol/build magic in the join blob.
const JOIN_MAGIC: u32 = 0x1002;
/// The player ships of `vars.cfg` `ship_type`, preloaded so their model and descriptor slots
/// are the same for every client (see DEVIATIONS.md).
const PLAYER_SHIPS: [&str; 11] = [
    "GALSPAN_PEGASUS", "GALSPAN_POSEIDON", "GALSPAN_ORION", "GALSPAN_ARCHANGEL", "GALSPAN_PHOENIX",
    "BORA_MACE", "BORA_CUTLASS", "BORA_BATTLEAXE", "BORA_WARHAMMER", "BORA_CLAYMORE", "MAKO",
];

/// The highest shield recharge rate a stock client sends: 2.0 in 16.16.
const MAX_SHIELD_RATE: i32 = 2 << 16;

pub struct Options {
    pub name: String,
    pub password: String,
    pub max_players: u32,
    pub original_bugs: bool,
    /// Deathmatch kill limit (`spacesav[0x62]`, default 20; 0 = none).
    pub max_kills: u32,
    /// Round length in minutes (`spacesav[0x61]`, default 30; 0 = untimed).
    pub time_limit_min: u32,
    /// Print every damage calculation and shield change to the console (`--debug-damage`,
    /// console `/debug damage`).
    pub debug_damage: bool,
    /// Deathmatch POWERUPS option (`spacesav[0x5a]`, default on): off spawns credit crystals only.
    pub powerups: bool,
}

/// Delivery spec (`NetSrv_QueueEvent`; filter `Tach_SrvEventFilter 0x4620b0`).
#[derive(Clone, Copy, Default)]
pub struct Delivery {
    pub budget: i32,
    pub flags: u16,
    pub radius: i64,
    pub pos: [i32; 3],
    pub player: Option<u16>,
    pub team: i32,
}

pub const D_RADIUS: u16 = 0x01;
pub const D_EXCEPT: u16 = 0x02;
pub const D_ONLY: u16 = 0x04;
pub const D_TEAM: u16 = 0x08;
pub const D_GROUP: u16 = 0x10;
pub const D_QUADRANT: u16 = 0x20;
pub const D_SYNCED: u16 = 0x40;
pub const D_LOADED: u16 = 0x80;

impl Delivery {
    pub fn all() -> Self {
        Delivery { budget: BUDGET_RELIABLE, ..Default::default() }
    }
    pub fn only(slot: u16) -> Self {
        Delivery { budget: BUDGET_RELIABLE, flags: D_ONLY, player: Some(slot), ..Default::default() }
    }
    pub fn except(slot: u16) -> Self {
        Delivery { budget: BUDGET_RELIABLE, flags: D_EXCEPT, player: Some(slot), ..Default::default() }
    }
}

/// One player connection: the Netsock peer plus the engine state (`TachConn`).
pub struct Conn {
    pub addr: SocketAddr,
    pub netsock_id: u32,
    pub slot: u16,
    pub name: String,
    #[allow(dead_code)] // team ships and rules hooks read it from M2 on
    pub join: JoinInfo,
    tx_seq: u32,
    rx_ack: u32,
    outbox: Outbox,
    tags: TagRing,
    /// STARTUPDATA +4 (conn +0x8c), starting at 1.
    stage_wire: u32,
    /// Blob index (conn +0x90).
    stage_idx: u32,
    pub in_startup: bool,
    client_frame: u32,
    pub last_heard: Instant,
    pub kick: Option<u8>,
    kick_sends: u32,
    /// conn +0x13c: 3 placed, 0 in game, 2 loading, 1 empty snapshots.
    pub state: u8,
    pub camera: [i32; 3],
    pub ship: u32,
    /// conn +0x140 (preferred / campaign team).
    pub team: u32,
    pub systems: u32,
    /// `(weapon id, max ammo, ammo)` per mount (conn +0x164, stride 0xc).
    pub mounts: [Option<(u8, u16, u16)>; 8],
    pub portrait: i16,
    chat_flood: i32,
    pub kills: u16,
    pub deaths: u16,
    far_list: Vec<u32>,
    far_cursor: Option<usize>,
    /// conn +0x1d0: dead or frozen; takes no damage.
    pub dead: bool,
    /// conn `inQuadrant`: snapshots carry records only while set. Cleared by 0x1007, set
    /// again by 0x2008 kind 2.
    pub in_quadrant: bool,
    /// Static object pages (`Srv_SendStaticObjs`; conn +0x120..+0x130): the list sorted by
    /// distance (0 = removed since), how many the client has acked, and the page id.
    static_list: Vec<u32>,
    static_acked: usize,
    static_page: u32,
}

/// A Netsock peer that has joined (0x1020) but may not have created a player yet.
struct Peer {
    last_heard: Instant,
    player: Option<u32>,
}

pub struct Server {
    pub opts: Options,
    pub map: Map,
    pub world: World,
    pub conns: Vec<Option<Conn>>,
    peers: HashMap<SocketAddr, Peer>,
    session: u32,
    pub game_frame: u32,
    net_tick: u32,
    portraits: Vec<String>,
    /// Datagrams to send: (address, Netsock message).
    pub outgoing: Vec<(SocketAddr, Vec<u8>)>,
    pub log: Option<PacketLog>,
    rng: u64,
    /// Round timer and between-rounds freeze, in ticks (`DAT_005b2824` / `DAT_005b2820`).
    pub round_ticks: u32,
    pub freeze_ticks: u32,
    /// Starbase roots → their parts (`objects.rs`).
    pub bases: HashMap<u32, objects::Base>,
    /// Powerup entities.
    pub pickups: HashMap<u32, objects::Pickup>,
    /// Part offsets per OCF file, computed once.
    part_offsets: HashMap<String, Vec<[f32; 3]>>,
    /// Class info after `{id, descSlot}` for gates, beacons, mines and speed rings.
    pub class_tails: HashMap<u32, Vec<u8>>,
}

fn now_seed() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1) | 1
}

impl Server {
    pub fn new(opts: Options, map: Map, log: Option<PacketLog>) -> Server {
        let slots = opts.max_players.min(map.config.max_players).max(1) as usize;
        let seed = now_seed();
        let mut s = Server {
            opts,
            map,
            world: World::new(),
            // Slot 0 is the stock host's own player (Netsock id 0x10000); remote players take
            // slots 1.., so slot == Netsock id − 0x10000 as the client assumes (roster slot, class
            // info, private chat target, startup kind 1 all use that one number).
            conns: (0..=slots).map(|_| None).collect(),
            peers: HashMap::new(),
            session: (seed as u32 & 0xffff) | 1,
            game_frame: 0,
            net_tick: 100,
            portraits: vec![],
            outgoing: vec![],
            log,
            rng: seed,
            round_ticks: 0,
            freeze_ticks: 0,
            bases: HashMap::new(),
            pickups: HashMap::new(),
            part_offsets: HashMap::new(),
            class_tails: HashMap::new(),
        };
        s.load_world();
        s.start_round_timer();
        s
    }

    fn rand(&mut self) -> u32 {
        // xorshift64*
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        (self.rng.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 32) as u32
    }

    /// Register the map's descriptors in SPX order, then the player ships.
    fn load_world(&mut self) {
        for d in self.map.decls.iter().flatten() {
            self.world.tables.add_des(d);
        }
        for alias in PLAYER_SHIPS {
            if let Ok(d) = self.map.registry.resolve(&self.map.src, alias) {
                self.world.tables.add_des(&d);
            }
        }
        self.spawn_statics();
    }

    /// The SPX placements the host builds as entities: asteroids (`Asteroid_SpawnAtLoad`: net
    /// flags 7, hitpoints = int(scale) x `HITPOINTS`), starbases, powerups, and gates, beacons,
    /// mines and speed rings (drawn only; `objects.rs`). SPX ships aren't built yet.
    fn spawn_statics(&mut self) {
        let placements = self.map.spx.placements.clone();
        for p in placements {
            if p.deferred() {
                continue;
            }
            let Some(des) = self.map.des_for(p.decl).cloned() else { continue };
            match des.desc_type.as_str() {
                "ASTEROID" => {}
                "STARBASE" => {
                    self.spawn_starbase(&p, des);
                    continue;
                }
                "POWERUP" => {
                    self.spawn_powerup(&p, des);
                    continue;
                }
                "HYPER_GATE" | "BEACON" | "MINE" | "SPEED_RING" => {
                    self.spawn_marker(&p, des);
                    continue;
                }
                _ => continue,
            }
            let Some((desc_slot, model)) = self.world.tables.add_des(&des) else { continue };
            let scale = (p.scale * 65536.0) as i32;
            let hp = (scale >> 16) * des_num(&des, "MAIN", "HITPOINTS") as i32;
            let pos = p.pos.map(|v| (v * 65536.0) as i32);
            let angles = p.orient.map(|a| (a << 16) & ANGLE_MASK);
            let id = self.world.alloc_id();
            self.world.spawn(Entity {
                id,
                owner: Owner::Static,
                des,
                desc_slot,
                model: model.unwrap_or(0),
                pos,
                vel: [0; 3],
                angles,
                net_pos: pos,
                net_angles: angles,
                state_flags: 0,
                throttle: 0,
                team: 0,
                net_flags: 7,
                scale,
                power: Power::default(),
                target: 0,
                hp,
            });
        }
    }

    /// `MpSectors_Reset`: drop every static and build them again from the SPX (new ids).
    fn respawn_statics(&mut self) {
        let statics: Vec<u32> = self.world.order.iter().copied().filter(|id| self.world.get(*id).is_some_and(|e| e.owner == Owner::Static)).collect();
        for id in statics {
            self.world.remove(id);
        }
        self.bases.clear();
        self.pickups.clear();
        self.class_tails.clear();
        self.spawn_statics();
    }

    /// Remove a static and blank it in every static list (`Srv_NotifyEntityRemoved`).
    fn remove_static(&mut self, id: u32) {
        self.world.remove(id);
        self.hide_static(id);
    }

    /// Blank a static in every connection's static list (`Srv_BroadcastToQuadrant`), so pages
    /// still to come skip it.
    fn hide_static(&mut self, id: u32) {
        for c in self.conns.iter_mut().flatten() {
            for e in c.static_list.iter_mut().filter(|e| **e == id) {
                *e = 0;
            }
        }
    }

    /// `Srv_SendStaticObjs`: every entity with net flags `& 3 == 3`, nearest to the ship first;
    /// page id = (id + 1) | 1.
    fn build_static_list(&mut self, slot: u16) {
        let Some(c) = self.conn(slot) else { return };
        let origin = self.world.get(c.ship).map_or(c.camera, |e| e.pos);
        let mut list: Vec<(i64, u32)> = self.world.order.iter().filter_map(|id| self.world.get(*id)).filter(|e| e.net_flags & 3 == 3).map(|e| (distance(origin, e.pos), e.id)).collect();
        list.sort_by_key(|(d, _)| *d);
        let c = self.conn_mut(slot).unwrap();
        c.static_list = list.into_iter().map(|(_, id)| id).collect();
        c.static_acked = 0;
        c.static_page = c.static_page.wrapping_add(1) | 1;
    }

    pub fn max_players(&self) -> u32 {
        self.conns.len() as u32 - 1
    }

    pub fn player_count(&self) -> usize {
        self.conns.iter().flatten().count()
    }

    fn session_record(&self) -> Vec<u8> {
        netsock::session_record(&self.opts.name, self.max_players(), self.player_count() as u32, !self.opts.password.is_empty(),
            // The stock host leaves the session GUID zero, and the NovaWorld join path
            // (`NwLaunch_JoinSession`) looks the session up by an all-zero GUID.
            [0; 16])
    }

    fn send_raw(&mut self, addr: SocketAddr, msg: Vec<u8>) {
        if let Some(l) = &mut self.log
            && netsock::kind(&msg) != netsock::GAME_DATA
        {
            l.session(false, addr, &msg);
        }
        self.outgoing.push((addr, msg));
    }

    // ---- Netsock layer ----

    pub fn on_datagram(&mut self, addr: SocketAddr, msg: &[u8]) {
        if let Some(l) = &mut self.log
            && netsock::kind(msg) != netsock::GAME_DATA
        {
            l.session(true, addr, msg);
        }
        let Some(m) = netsock::parse_client(msg) else { return };
        if let Some(p) = self.peers.get_mut(&addr) {
            p.last_heard = Instant::now();
        }
        if let Some(c) = self.conns.iter_mut().flatten().find(|c| c.addr == addr) {
            c.last_heard = Instant::now();
        }
        match m {
            ClientMsg::Enum { guid_ok: true } => {
                let r = self.session_record();
                self.send_raw(addr, netsock::enum_reply(&r));
            }
            ClientMsg::Enum { .. } => {}
            ClientMsg::Join { guid_ok, password } => {
                let refuse = if !guid_ok {
                    Some(netsock::REFUSE_GUID)
                } else if !self.opts.password.is_empty() && password != self.opts.password {
                    Some(netsock::REFUSE_PASSWORD)
                } else if !self.peers.contains_key(&addr) && self.conns.iter().skip(1).all(Option::is_some) {
                    Some(netsock::REFUSE_FULL)
                } else {
                    None
                };
                match refuse {
                    Some(r) => self.send_raw(addr, netsock::join_reply(false, r, netsock::SERVER_PLAYER_ID)),
                    None => {
                        self.peers.entry(addr).or_insert(Peer { last_heard: Instant::now(), player: None });
                        self.send_raw(addr, netsock::join_reply(true, 0, netsock::SERVER_PLAYER_ID));
                    }
                }
            }
            ClientMsg::CreatePlayer { name, data, .. } => {
                if !self.peers.contains_key(&addr) {
                    return;
                }
                // A repeated 0x1030 gets the existing id back.
                if let Some(id) = self.peers[&addr].player {
                    self.send_raw(addr, netsock::create_reply(0, id));
                    return;
                }
                let Some(slot) = self.conns.iter().skip(1).position(Option::is_none).map(|i| i + 1) else {
                    self.send_raw(addr, netsock::create_reply(1, 0));
                    return;
                };
                // Stock ids: 0x10000 + table slot, with the host's own player in slot 0.
                let id = netsock::SERVER_PLAYER_ID + slot as u32;
                self.peers.get_mut(&addr).unwrap().player = Some(id);
                self.send_raw(addr, netsock::create_reply(0, id));
                let data = data.to_vec();
                self.player_joined(addr, id, slot as u16, &name, &data);
            }
            ClientMsg::DestroyPlayer { player_id } => {
                self.send_raw(addr, netsock::destroy_reply());
                if let Some(slot) = self.slot_of(addr).filter(|&s| self.conns[s as usize].as_ref().unwrap().netsock_id == player_id) {
                    self.remove_player(slot, "left");
                }
                if let Some(p) = self.peers.get_mut(&addr) {
                    p.player = None;
                }
            }
            ClientMsg::Close => {
                self.send_raw(addr, netsock::close_reply());
                if let Some(slot) = self.slot_of(addr) {
                    self.remove_player(slot, "left");
                }
                self.peers.remove(&addr);
            }
            ClientMsg::GameData(p) => {
                if let Some(slot) = self.slot_of(addr) {
                    let p = p.to_vec();
                    self.on_packet(slot, &p);
                }
            }
            ClientMsg::Other(_) => {}
        }
    }

    fn slot_of(&self, addr: SocketAddr) -> Option<u16> {
        self.conns.iter().flatten().find(|c| c.addr == addr).map(|c| c.slot)
    }

    pub fn conn(&self, slot: u16) -> Option<&Conn> {
        self.conns.get(slot as usize).and_then(Option::as_ref)
    }

    fn conn_mut(&mut self, slot: u16) -> Option<&mut Conn> {
        self.conns.get_mut(slot as usize).and_then(Option::as_mut)
    }

    // ---- events ----

    fn passes(&self, c: &Conn, d: &Delivery) -> bool {
        if d.flags & D_LOADED != 0 && c.in_startup {
            return false;
        }
        if d.flags & D_SYNCED != 0 && c.state != 0 {
            return false;
        }
        if d.flags & D_RADIUS != 0 && distance(c.camera, d.pos) > d.radius {
            return false;
        }
        if d.flags & D_EXCEPT != 0 && d.player == Some(c.slot) {
            return false;
        }
        if d.flags & D_ONLY != 0 && d.player != Some(c.slot) {
            return false;
        }
        let ship_team = self.world.get(c.ship).map_or(-1, |e| e.team as i32);
        if d.flags & D_TEAM != 0 && ship_team != d.team {
            return false;
        }
        if d.flags & D_GROUP != 0 && c.team as i32 != d.team {
            return false;
        }
        // One quadrant per map for now, so D_QUADRANT always passes.
        true
    }

    pub fn queue(&mut self, kind: u16, body: Vec<u8>, d: Delivery) {
        let targets: Vec<u16> = self.conns.iter().flatten().filter(|c| self.passes(c, &d)).map(|c| c.slot).collect();
        for s in targets {
            let c = self.conn_mut(s).unwrap();
            c.outbox.queue(kind, body.clone(), d.budget);
        }
    }

    pub fn text_to(&mut self, d: Delivery, color: u16, s: &str) {
        self.queue(ev::EV_TEXT, ev::text(0, color, s), d);
    }

    fn roster_entry(&self, c: &Conn) -> RosterEntry {
        RosterEntry {
            slot: c.slot,
            portrait: c.portrait,
            a: 0,
            b: 0,
            team: self.world.get(c.ship).map_or(0xff, |e| e.team),
            name: c.name.clone(),
        }
    }

    fn scoreboard(&mut self) {
        let mut rows: Vec<(u16, u16, u16)> = self.conns.iter().flatten().map(|c| (c.slot, c.kills, c.deaths)).collect();
        rows.sort_by(|a, b| b.1.cmp(&a.1).then(a.2.cmp(&b.2)));
        self.queue(ev::EV_SCOREBOARD, ev::scoreboard(&rows), Delivery::all());
    }

    fn portrait_slot(&mut self, name: &str) -> i16 {
        if name.is_empty() {
            return -1;
        }
        if let Some(i) = self.portraits.iter().position(|p| p.eq_ignore_ascii_case(name)) {
            return i as i16;
        }
        if self.portraits.len() >= 64 {
            return -1;
        }
        self.portraits.push(name.to_string());
        (self.portraits.len() - 1) as i16
    }

    // ---- join / leave (`Tach_SrvPlacePlayer 0x461280`, `Tach_SrvRemovePlayer 0x461400`) ----

    fn player_joined(&mut self, addr: SocketAddr, netsock_id: u32, slot: u16, name: &str, blob: &[u8]) {
        let join = parse_join_blob(blob);
        let name = if name.is_empty() { "JAKE_LOGAN".to_string() } else { name.to_string() };
        if let Some(l) = &mut self.log {
            l.note(&format!("slot {slot} = {addr} ({name:?}, player id {netsock_id:#x})"));
        }
        let kick = (join.magic != JOIN_MAGIC).then_some(5u8);
        let mut c = Conn {
            addr,
            netsock_id,
            slot,
            name: name.clone(),
            join: join.clone(),
            tx_seq: 1,
            rx_ack: 0,
            outbox: Outbox::new(0x1000),
            tags: TagRing::default(),
            stage_wire: 1,
            stage_idx: 0,
            in_startup: true,
            client_frame: 0,
            last_heard: Instant::now(),
            kick,
            kick_sends: 0,
            state: 3,
            camera: [0; 3],
            ship: 0,
            team: join.preferred_team,
            systems: join.systems,
            mounts: [None; 8],
            portrait: -1,
            chat_flood: 0,
            kills: 0,
            deaths: 0,
            far_list: vec![],
            far_cursor: None,
            dead: false,
            in_quadrant: true,
            static_list: vec![],
            static_acked: 0,
            static_page: 0,
        };
        // Loadout: each mount at its max ammo (`MpRules_CopyLoadout`, then `MpRules_ClearLoadout`
        // copies max into the ammo counter).
        for (i, &m) in join.mounts.iter().enumerate() {
            if m == 0 {
                continue;
            }
            if let Some(w) = weapons::by_id((m - 1) as u8) {
                let max = if join.systems & 0x10_0000 != 0 { w.ammo + (w.ammo >> 2) } else { w.ammo } as u16;
                c.mounts[i] = Some((w.id, max, max));
            }
        }
        self.conns[slot as usize] = Some(c);

        // The mode's join hook picks team and ship, then spawn.
        let (team, alias) = match self.map.config.game_type {
            GameType::BaseWars => {
                // Placeholder until M6: team from the preferred side, the matching team ship.
                if join.preferred_team == 1 { (1u32, join.bora_ship.clone()) } else { (2u32, join.galspan_ship.clone()) }
            }
            _ => (join.dm_team, join.dm_ship.clone()),
        };
        let des = match self.map.registry.resolve(&self.map.src, &alias) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("{name}: ship {alias:?}: {e}");
                self.map.registry.resolve(&self.map.src, "BORA_CUTLASS").expect("BORA_CUTLASS")
            }
        };
        let (desc_slot, model) = self.world.tables.add_des(&des).unwrap_or((0, None));
        let id = self.world.alloc_id();
        let (pos, angles) = self.dm_spawn_pos();
        let power = Power::for_des(&des);
        self.world.spawn(Entity {
            id,
            owner: Owner::Player(slot),
            des: des.clone(),
            desc_slot,
            model: model.unwrap_or(0),
            pos,
            vel: [0; 3],
            angles,
            net_pos: pos,
            net_angles: angles,
            state_flags: 5,
            throttle: 0,
            team: team as u8,
            net_flags: 0,
            scale: FX,
            power,
            target: 0,
            hp: 0,
        });
        let portrait_name = join.portrait.clone();
        let portrait = self.portrait_slot(&portrait_name);
        let is_bw = self.map.config.game_type == GameType::BaseWars;
        {
            let c = self.conn_mut(slot).unwrap();
            c.ship = id;
            c.camera = pos;
            c.portrait = portrait;
            if !is_bw {
                c.team = team;
            }
        }
        self.build_static_list(slot);
        self.queue(ev::EV_TEAM_SHIP, ev::team_ship(&des.file, desc_slot), Delivery::all());
        if portrait >= 0 {
            self.queue(ev::EV_PORTRAIT, ev::portrait(&portrait_name, portrait as u16), Delivery::all());
        }
        self.text_to(Delivery::except(slot), ev::COLOR_JOIN, &format!("{name} is entering the game"));
        let e = self.roster_entry(self.conn(slot).unwrap());
        self.queue(ev::EV_ROSTER, ev::roster(&e), Delivery::all());
        self.scoreboard();
        println!("{name} joined from {addr} as slot {slot}, flying {} ({}), team {team}", des.file, alias);
    }

    /// `DM_PickSpawnPos`: (0, 625, 2500) rotated about Y by a random yaw, facing the origin.
    fn dm_spawn_pos(&mut self) -> ([i32; 3], [i32; 3]) {
        let yaw = (self.rand() as i32) & ANGLE_MASK;
        let a = yaw as f64 / (512.0 * 65536.0) * std::f64::consts::TAU;
        let (x, z) = (2500.0 * a.sin(), 2500.0 * a.cos());
        let pos = [(x * 65536.0) as i32, 625 * FX, (z * 65536.0) as i32];
        (pos, face_toward(pos, [0; 3]))
    }

    pub fn remove_player(&mut self, slot: u16, why: &str) {
        let Some(c) = self.conns[slot as usize].take() else { return };
        if let Some(l) = &mut self.log {
            l.note(&format!("slot {slot} ({}) removed: {why}", c.addr));
        }
        if let Some(p) = self.peers.get_mut(&c.addr) {
            p.player = None;
        }
        if let Some(e) = self.world.remove(c.ship) {
            let pos = e.pos;
            let mut body = Vec::with_capacity(0x16);
            for v in pos {
                body.extend_from_slice(&v.to_le_bytes());
            }
            body.extend_from_slice(&[0; 8]);
            body.extend_from_slice(&[3, 0]);
            self.queue(ev::EV_DESTROYED, body, Delivery { budget: 4, flags: D_SYNCED | D_QUADRANT | D_RADIUS, radius: EFFECT_RADIUS[3] * FX as i64, pos, ..Default::default() });
        }
        self.text_to(Delivery::all(), ev::COLOR_JOIN, &format!("{} has left the game", c.name));
        self.queue(ev::EV_ROSTER_CLEARED, ev::roster_cleared(slot), Delivery::all());
        self.scoreboard();
        println!("{} ({}) {why}", c.name, c.addr);
    }

    pub fn kick(&mut self, slot: u16, reason: u8) {
        if let Some(c) = self.conn_mut(slot) {
            c.kick = Some(reason);
        }
    }

    // ---- engine packets in ----

    fn on_packet(&mut self, slot: u16, p: &[u8]) {
        if let Some(l) = &mut self.log {
            l.packet(true, slot, p);
        }
        let Some((seq, ack, subs)) = packet::parse(p) else { return };
        {
            let c = self.conn_mut(slot).unwrap();
            if c.rx_ack != 0 && seq <= c.rx_ack {
                return;
            }
            c.rx_ack = seq;
            c.outbox.on_ack(ack);
        }
        for s in subs {
            match s.kind {
                packet::MSG_PLAYERDATA => self.on_player_data(slot, s.body),
                packet::EV_STARTUP_ACK => {
                    let c = self.conn_mut(slot).unwrap();
                    if c.tags.first_time(s.tag.unwrap_or(0)) && proto::u16_at(s.body, 0) == 1 {
                        let stage = proto::u32_at(s.body, 2);
                        self.on_startup_ack(slot, stage);
                    }
                }
                k if k >= 8 => {
                    let c = self.conn_mut(slot).unwrap();
                    if c.tags.first_time(s.tag.unwrap_or(0)) {
                        self.on_event(slot, k, s.body);
                    }
                }
                _ => {}
            }
        }
    }

    /// `NetSrv_HandlePlayerData` + `Tach_SrvReadPlayerData 0x460b30`.
    fn on_player_data(&mut self, slot: u16, b: &[u8]) {
        let (frame, session) = (proto::u32_at(b, 4), proto::u32_at(b, 8));
        if session != self.session {
            return;
        }
        let Some(pd) = b.get(12..).and_then(parse_player_data) else { return };
        let c = self.conn_mut(slot).unwrap();
        if frame <= c.client_frame {
            return;
        }
        c.client_frame = frame;
        c.camera = pd.camera;
        let ship = c.ship;
        if let Some(e) = self.world.get_mut(ship) {
            if pd.has_ship {
                e.vel = pd.vel;
                e.net_pos = pd.pos;
                e.net_angles = pd.angles;
            }
            e.target = pd.target;
            // DEVIATIONS.md "Shield regen rate": the original takes any value; a stock client
            // only ever sends 0..2.0 (`Power_StepRate` wraps at 2.0).
            e.power.shield_rate = if self.opts.original_bugs { pd.shield_rate } else { pd.shield_rate.clamp(0, MAX_SHIELD_RATE) };
            e.throttle = (pd.throttle >> 3) as u8;
        }
    }

    /// `NetSrv_AdvanceStartupStage` + `Tach_SrvStartupStageDone 0x462080`.
    fn on_startup_ack(&mut self, slot: u16, stage: u32) {
        let c = self.conn_mut(slot).unwrap();
        if !c.in_startup || stage != c.stage_wire {
            return;
        }
        let done = c.stage_idx == 5;
        c.stage_wire += 1;
        c.stage_idx += 1;
        if done {
            c.in_startup = false;
            c.state = 0;
            let name = c.name.clone();
            self.queue(ev::EV_STARTUP_DONE, ev::startup_done(), Delivery::only(slot));
            println!("{name} finished loading");
        }
    }

    fn on_event(&mut self, slot: u16, kind: u16, b: &[u8]) {
        match kind {
            ev::CL_CHAT => self.on_chat(slot, b),
            ev::CL_DATA_ACK => self.on_data_ack(slot, b),
            ev::CL_FIRE => self.on_fire(slot, b),
            ev::CL_HIT => self.on_hit(slot, ev::parse_hit(b)),
            ev::CL_PART_HIT => self.on_hit(slot, ev::parse_part_hit(b)),
            ev::CL_POWER => self.on_power_op(slot, b),
            ev::CL_COLLISION => self.on_collision(slot, b),
            ev::CL_RESPAWN => self.on_respawn(slot),
            ev::CL_CLASS_INFO => self.on_class_info_req(slot, b),
            ev::CL_ENTER_WORLD => {
                if let Some(c) = self.conn_mut(slot) {
                    c.state = 0;
                }
            }
            ev::CL_READY => {
                if let Some(c) = self.conn_mut(slot) {
                    c.state = 2;
                }
            }
            ev::CL_QUADRANT_ENTERED => {}
            ev::CL_LOADOUT => self.on_loadout(slot, b),
            _ => {
                if let Some(l) = &mut self.log {
                    l.note(&format!("slot {slot}: unhandled event {kind:#06x} ({} bytes)", b.len()));
                }
            }
        }
    }

    /// 0x2002 (`TachSrv_OnClassInfoReq`): a client met a record with state flag 0x04 for an
    /// entity it has no descriptor for. Reply 0x1100 to that player; without it the client never
    /// builds the replica (`ShipReplica_Init` links it into the render list), so the ship stays
    /// invisible.
    fn on_class_info_req(&mut self, slot: u16, b: &[u8]) {
        let id = proto::u32_at(b, 0);
        if self.object_class_info(slot, id) {
            return;
        }
        let Some(e) = self.world.get(id) else { return };
        // Player ships (`LAB_00463690`, desc +0x24 of SHIP). AI ships come in M5.
        let Owner::Player(owner) = e.owner else { return };
        let body = ev::class_info_player(id, e.desc_slot, owner);
        self.queue(ev::EV_CLASS_INFO, body, Delivery::only(slot));
    }

    /// 0x2008 (`TachSrv_OnDataChunk`): kind 1 acks a static page (`Srv_RecvPlayerChunk`), kind 2
    /// marks the client as in the quadrant.
    fn on_data_ack(&mut self, slot: u16, b: &[u8]) {
        let (kind, page, count) = ev::parse_data_ack(b);
        let Some(c) = self.conn_mut(slot) else { return };
        match kind {
            1 if !c.static_list.is_empty() && c.static_page == page => {
                c.static_acked += count as usize;
                if c.static_acked >= c.static_list.len() {
                    c.static_list.clear();
                    c.static_acked = 0;
                }
                c.static_page = c.static_page.wrapping_add(1) | 1;
            }
            2 => c.in_quadrant = true,
            _ => {}
        }
    }

    /// 0x2028 `{u32 systems, u16 n, {u16 weapon+1, u16 ammo}[n]}`, applied as sent.
    fn on_loadout(&mut self, slot: u16, b: &[u8]) {
        let systems = proto::u32_at(b, 0);
        let n = proto::u16_at(b, 4) as usize;
        let c = self.conn_mut(slot).unwrap();
        c.systems = systems;
        for i in 0..8 {
            c.mounts[i] = None;
            if i < n {
                let (w, ammo) = (proto::u16_at(b, 6 + 4 * i), proto::u16_at(b, 8 + 4 * i));
                if w != 0 {
                    let max = weapons::by_id((w - 1) as u8).map_or(ammo, |d| d.ammo as u16);
                    c.mounts[i] = Some(((w - 1) as u8, max, ammo));
                }
            }
        }
    }

    /// `TachSrv_OnChat 0x460600`. Lines starting with `/` are server commands: echoed to the
    /// sender like any chat, answered, and not relayed (the original's command hook
    /// `Stub_ReturnZeroB` is a stub).
    fn on_chat(&mut self, slot: u16, b: &[u8]) {
        let (mode, target, text) = ev::parse_chat(b);
        let c = self.conn_mut(slot).unwrap();
        let flooded = c.chat_flood > 0;
        c.chat_flood += 50;
        if flooded {
            return;
        }
        let name = c.name.clone();
        let is_command = text.trim_start().starts_with('/') || self.command_text(&text).is_some();
        match mode {
            0 => {
                self.text_to(Delivery::only(slot), ev::COLOR_CHAT, &text);
                if is_command {
                    let reply = crate::console::run_command(self, &text, Some(slot));
                    for line in reply {
                        self.text_to(Delivery::only(slot), ev::COLOR_INFO, &line);
                    }
                } else {
                    self.text_to(Delivery::except(slot), ev::COLOR_CHAT, &text);
                    println!("chat (slot {slot}, {name}) {text}");
                }
            }
            1 => {
                if self.conn(target).is_some() {
                    self.text_to(Delivery::only(target), ev::COLOR_PRIVATE, &text);
                }
            }
            4 => self.text_to(Delivery { budget: BUDGET_RELIABLE, flags: D_TEAM, team: target as i32, ..Default::default() }, ev::COLOR_TEAM, &text),
            8 => self.text_to(Delivery { budget: BUDGET_RELIABLE, flags: D_GROUP, team: target as i32, ..Default::default() }, ev::COLOR_GROUP, &text),
            _ => {}
        }
    }

    /// The client prefixes chat with "NAME: "; a command may follow that prefix.
    fn command_text<'a>(&self, text: &'a str) -> Option<&'a str> {
        text.split_once(": ").map(|(_, t)| t.trim_start()).filter(|t| t.starts_with('/'))
    }

    pub fn strip_chat_prefix<'a>(&self, text: &'a str) -> &'a str {
        self.command_text(text).unwrap_or(text.trim_start())
    }

    /// An entity record as `viewer` (a ship id) receives it. Byte 7 is not the team but the
    /// rules' `+0x1c` hook, which `MpRules_LoadRegion` replaces with the mode's relation
    /// function: `DM_GetRelation` gives every player ship 2 (hostile) and anything else 4;
    /// `BW_GetRelation` gives players 1 (same team) or 2, and others their `race.cfg` relation
    /// to the viewer (2 hostile, else 4; an allied relation, 1, isn't modelled since
    /// `race.cfg` lists only enemies). The client colours by this byte.
    fn record_for(&self, id: u32, viewer: u32, extra_flags: u8, ext: bool) -> RecordOut {
        let e = self.world.get(id).unwrap();
        let mut r = e.record(extra_flags, ext);
        r.team = self.relation(e, viewer);
        r
    }

    fn relation(&self, e: &Entity, viewer: u32) -> u8 {
        let player = matches!(e.owner, Owner::Player(_));
        match self.map.config.game_type {
            GameType::Deathmatch => {
                if player { 2 } else { 4 }
            }
            _ => {
                let mine = self.world.get(viewer).map_or(0, |v| v.team);
                if player {
                    if e.team == mine { 1 } else { 2 }
                } else if self.map.races.hostile(mine as usize, e.team as usize) {
                    2
                } else {
                    4
                }
            }
        }
    }

    // ---- tick ----

    pub fn tick(&mut self) {
        self.round_tick();
        self.regen();
        self.pickups_tick();
        // Per-player think: smoothing filter.
        let ids: Vec<u32> = self.world.order.clone();
        for id in ids {
            if let Some(e) = self.world.get_mut(id)
                && matches!(e.owner, Owner::Player(_))
            {
                e.smooth_step();
            }
        }
        for c in self.conns.iter_mut().flatten() {
            if c.chat_flood > 0 {
                c.chat_flood -= 1;
            }
        }
        // Timeouts.
        let now = Instant::now();
        let dead: Vec<u16> = self.conns.iter().flatten().filter(|c| now.duration_since(c.last_heard) > TIMEOUT).map(|c| c.slot).collect();
        for s in dead {
            self.remove_player(s, "timed out");
        }
        self.peers.retain(|_, p| now.duration_since(p.last_heard) <= TIMEOUT || p.player.is_some());
        self.send_round();
        self.game_frame = self.game_frame.wrapping_add(1);
        self.net_tick = self.net_tick.wrapping_add(1);
    }

    /// `NetSrv_SendRound 0x4b8650`: slots with `(slot & 15) == (tick & 15)`.
    fn send_round(&mut self) {
        let phase = self.net_tick & 15;
        let slots: Vec<u16> = self.conns.iter().flatten().filter(|c| (c.slot as u32 & 15) == phase).map(|c| c.slot).collect();
        let mut kicked = vec![];
        for s in slots {
            let p = self.build_packet(s);
            let c = self.conn_mut(s).unwrap();
            if c.kick.is_some() {
                c.kick_sends += 1;
                if c.kick_sends >= 4 {
                    kicked.push(s);
                }
            }
            let addr = c.addr;
            if let Some(l) = &mut self.log {
                l.packet(false, s, &p);
            }
            self.send_raw(addr, netsock::game_data(&p));
        }
        for s in kicked {
            self.remove_player(s, "kicked");
        }
    }

    fn build_packet(&mut self, slot: u16) -> Vec<u8> {
        let (seq, ack, kick, in_startup, stage_wire, stage_idx, netsock_id) = {
            let c = self.conn(slot).unwrap();
            (c.tx_seq, c.rx_ack, c.kick, c.in_startup, c.stage_wire, c.stage_idx, c.netsock_id)
        };
        let mut w = PacketWriter::new(seq, ack);
        if let Some(r) = kick {
            w.sub(packet::MSG_KICK, &[r, 0, 0, 0]);
        } else if in_startup {
            let mut b = stage_wire.to_le_bytes().to_vec();
            b.extend_from_slice(&self.session.to_le_bytes());
            // `NetSrv_WriteStartupData`: the connection's Netsock player id (`TachConn+8`).
            b.extend_from_slice(&netsock_id.to_le_bytes());
            b.extend_from_slice(&self.startup_blob(slot, stage_idx));
            w.sub(packet::MSG_STARTUPDATA, &b);
        } else {
            let mut info = self.net_tick.to_le_bytes().to_vec();
            info.extend_from_slice(&self.session.to_le_bytes());
            info.extend_from_slice(&self.game_frame.to_le_bytes());
            w.sub(packet::MSG_PLAYERINFO, &info);
            let gd = self.game_data(slot);
            w.sub(packet::MSG_GAMEDATA, &gd);
            let c = self.conn_mut(slot).unwrap();
            c.outbox.write_into(&mut w, seq);
        }
        self.conn_mut(slot).unwrap().tx_seq += 1;
        w.finish()
    }

    fn startup_blob(&self, slot: u16, idx: u32) -> Vec<u8> {
        let c = self.conn(slot).unwrap();
        let rec = match idx {
            0 => startup::model_table(&self.world.tables.model_entries()),
            1 => startup::desc_table(&self.world.tables.desc_entries()),
            2 => {
                let e = self.world.get(c.ship);
                let k1 = startup::rules_info(&startup::RulesInfo {
                    bdf: &self.map.spx.backdrop,
                    region: "NOVAWORLD",
                    entity_id: c.ship,
                    rules_value: 1,
                    region_index: 9,
                    quadrant_id: self.map.spx.sector_id as u16,
                    slot,
                    max_slots: self.conns.len() as u16,
                    game_mode: if self.map.config.game_type == GameType::BaseWars { 0 } else { 1 },
                });
                let mut mounts = [(0u16, 0u16); 8];
                for (i, m) in c.mounts.iter().enumerate() {
                    if let Some((w, max, _)) = m {
                        mounts[i] = (*w as u16 + 1, *max);
                    }
                }
                let k4 = startup::player_init(&startup::PlayerInit {
                    des_name: e.map_or("", |e| e.des.file.as_str()),
                    team: c.team as u16,
                    systems: c.systems,
                    pos: e.map_or([0; 3], |e| e.pos),
                    angles: e.map_or([0; 3], |e| e.angles),
                    mounts,
                });
                return startup::blob(&[k1, k4]);
            }
            3 => startup::portraits(&self.portraits.iter().enumerate().map(|(i, n)| (n.clone(), i as u16)).collect::<Vec<_>>()),
            4 => startup::roster(&self.conns.iter().flatten().map(|c| self.roster_entry(c)).collect::<Vec<_>>()),
            _ => startup::session_info(0, 0, 0),
        };
        startup::blob(&[rec])
    }

    /// `Tach_SrvWriteGameData 0x461a30` + `Tach_SrvWriteSnapshot 0x461870`.
    fn game_data(&mut self, slot: u16) -> Vec<u8> {
        let (camera, ship, state, in_quadrant) = {
            let c = self.conn(slot).unwrap();
            (c.camera, c.ship, c.state, c.in_quadrant)
        };
        let me = self.world.get(ship);
        let mut status = StatusOut { target_meters: [0xff; 3], ..Default::default() };
        let mut target = 0;
        if let Some(e) = me {
            status.shield_front = e.power.shield_front.clamp(0, 0xffff) as u16;
            status.shield_rear = e.power.shield_rear.clamp(0, 0xffff) as u16;
            status.hull = e.power.hull.clamp(0, 0xffff) as u16;
            target = e.target;
            if let Some(t) = self.world.get(target) {
                status.target_meters = [pct(t.power.hull, t.power.hull_max), pct(t.power.shield_front, t.power.shield_max), 0xff];
            }
        }
        let mut recs = vec![];
        let mut page = (0u16, 0u16);
        if state != 1 && in_quadrant {
            let visible = |e: &Entity| e.id != ship && e.net_flags & 1 == 0;
            // Near: within the near radius, at most 8 (2 while loading).
            let mut near: Vec<(i64, u32)> = self.world.order.iter().filter_map(|id| self.world.get(*id)).filter(|e| visible(e)).map(|e| (distance(camera, e.pos), e.id)).filter(|(d, _)| *d < NEAR_RADIUS).collect();
            if !self.opts.original_bugs {
                near.sort_by_key(|(d, _)| *d);
            }
            let cap = if state == 2 { 2 } else { 8 };
            let mut sent: Vec<u32> = near.iter().take(cap).map(|(_, id)| *id).collect();
            // Your target, if not already in.
            if target != 0 && !sent.contains(&target) && self.world.get(target).is_some_and(|e| visible(e)) {
                sent.push(target);
            }
            for id in &sent {
                recs.push(self.record_for(*id, ship, 0, false));
            }
            // Far: two more per snapshot from a cursor over everything within the far radius.
            let far_ids = self.far_records(slot, camera, ship, &sent);
            for id in far_ids {
                recs.push(self.record_for(id, ship, proto::REC_FAR, false));
            }
            // The current static page (`Tach_SrvWritePersistentPage`): 12 records while loading,
            // else 4, repeated until acked. Removed entries count but aren't written.
            let c = self.conn(slot).unwrap();
            if !c.static_list.is_empty() {
                let size = if state == 2 { 12 } else { 4 };
                let n = (c.static_list.len() - c.static_acked).min(size);
                page = (c.static_page as u16, n as u16);
                for id in &c.static_list[c.static_acked..c.static_acked + n] {
                    if self.world.get(*id).is_some() {
                        recs.push(self.record_for(*id, ship, 0, true));
                    }
                }
            }
        }
        proto::write_gamedata(&status, camera, page, &recs)
    }

    /// `Tach_SrvBuildFarList` / `Tach_SrvWriteFarRecords`.
    fn far_records(&mut self, slot: u16, camera: [i32; 3], ship: u32, sent: &[u32]) -> Vec<u32> {
        let rebuild = self.conn(slot).unwrap().far_cursor.is_none();
        if rebuild {
            let list: Vec<u32> = self.world.order.iter().filter_map(|id| self.world.get(*id)).filter(|e| e.id != ship && e.net_flags & 1 == 0 && distance(e.pos, camera) < FAR_RADIUS).map(|e| e.id).collect();
            let c = self.conn_mut(slot).unwrap();
            c.far_cursor = (!list.is_empty()).then_some(0);
            c.far_list = list;
        }
        let mut out = vec![];
        loop {
            let c = self.conn(slot).unwrap();
            let Some(i) = c.far_cursor else { break };
            if out.len() >= 2 {
                break;
            }
            if i >= c.far_list.len() {
                self.conn_mut(slot).unwrap().far_cursor = None;
                break;
            }
            let id = c.far_list[i];
            if !sent.contains(&id) && self.world.get(id).is_some_and(|e| e.net_flags & 1 == 0) {
                out.push(id);
            }
            self.conn_mut(slot).unwrap().far_cursor = Some(i + 1);
        }
        out
    }

    pub fn shutdown(&mut self) {
        let addrs: Vec<SocketAddr> = self.peers.keys().copied().collect();
        for a in addrs {
            self.send_raw(a, netsock::server_closing());
        }
    }

    pub fn status_lines(&self, with_addresses: bool) -> Vec<String> {
        let mut v = vec![format!(
            "{} on {} ({}): {}/{} players, frame {}",
            self.opts.name,
            self.map.config.name,
            self.map.config.game_type_name,
            self.player_count(),
            self.max_players(),
            self.game_frame
        )];
        for c in self.conns.iter().flatten() {
            let ship = self.world.get(c.ship).map_or("-".to_string(), |e| e.des.file.clone());
            let st = if c.in_startup { format!("loading {}/6", c.stage_idx) } else { "in game".into() };
            let addr = if with_addresses { c.addr.to_string() } else { String::new() };
            v.push(format!("  slot {:2} {:<16} {:<14} K{} D{} {st} {addr}", c.slot, c.name, ship, c.kills, c.deaths));
        }
        v
    }
}

/// A number from a `.des` group (0 if missing).
fn des_num(des: &Rc<Des>, group: &str, key: &str) -> f32 {
    des.group(group).and_then(|g| g.get(key)).and_then(|i| i.values.first()).and_then(|v| v.as_f32()).unwrap_or(0.0)
}

fn pct(v: i32, max: i32) -> u8 {
    if max <= 0 { 0xff } else { (v.max(0) as i64 * 100 / max as i64).min(100) as u8 }
}

/// (yaw, pitch, 0) facing from `from` toward `to`, 16.16 angles at 512 per turn. The original
/// (`DM_PickSpawnPos` → `Vec_DirAngles(pos, origin)`: yaw = Math_Atan2(dx, dz), pitch =
/// Math_Atan2(dy, -|dxz|)) depends on `Math_Atan2`'s argument convention, which isn't pinned
/// down yet [?]; this aims the nose at the target with yaw = atan2(dx, dz) and no pitch, which the
/// client then owns anyway.
pub fn face_toward(from: [i32; 3], to: [i32; 3]) -> [i32; 3] {
    let d: [f64; 3] = std::array::from_fn(|k| to[k] as f64 - from[k] as f64);
    let yaw = d[0].atan2(d[2]);
    let pitch = 0.0;
    let unit = |a: f64| ((a / std::f64::consts::TAU * 512.0 * 65536.0) as i64 as i32) & ANGLE_MASK;
    [unit(yaw), unit(pitch), 0]
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use tachyon_core::net::proto::{join_blob, player_data, startup_records, JoinBlob, ShipState, Snapshot};
    use tachyon_core::source::AssetSource;

    fn server(map: &str) -> Option<Server> {
        let src = AssetSource::open(std::path::Path::new("../../pff")).ok()?;
        let cfg = tachyon_core::mpc::find(&src, map).ok()?;
        let map = Map::load(src, cfg).ok()?;
        Some(Server::new(Options { name: "t".into(), password: String::new(), max_players: 8, original_bugs: false, max_kills: 20, time_limit_min: 30, debug_damage: false, powerups: true }, map, None))
    }

    /// A scripted client: acks startup stages, static pages and 0x1007, sends PLAYERDATA at
    /// `pos` and whatever events the test queues, and keeps every event and snapshot it gets.
    struct Client {
        addr: SocketAddr,
        seq: u32,
        tag: u32,
        pos: [i32; 3],
        out: Vec<(u16, Vec<u8>)>,
        seen: HashSet<u32>,
        events: Vec<(u16, Vec<u8>)>,
        snaps: Vec<Snapshot>,
        stage: u32,
        acked_pages: HashSet<u16>,
    }

    struct Harness {
        s: Server,
        c: Vec<Client>,
    }

    impl Harness {
        fn new(map: &str, n: usize) -> Option<Harness> {
            let mut s = server(map)?;
            let mut c = vec![];
            for i in 0..n {
                let addr: SocketAddr = format!("127.0.0.1:{}", 5000 + i).parse().unwrap();
                let blob = join_blob(&JoinBlob { ship: "BORA_CUTLASS", team: 1, portrait: "", game_id: 0, weapon: 6 });
                s.on_datagram(addr, &netsock::join_request(""));
                s.on_datagram(addr, &netsock::create_player(1, &format!("P{i}"), &blob));
                c.push(Client {
                    addr,
                    seq: 1,
                    tag: 0x1000,
                    pos: [0, 625 << 16, (2000 + 100 * i as i32) << 16],
                    out: vec![],
                    seen: HashSet::new(),
                    events: vec![],
                    snaps: vec![],
                    stage: 0,
                    acked_pages: HashSet::new(),
                });
            }
            Some(Harness { s, c })
        }

        /// Client i's connection (slot i + 1; slot 0 is the stock host's own player).
        fn conn(&self, i: usize) -> &Conn {
            self.s.conns[i + 1].as_ref().unwrap()
        }

        fn ship(&self, i: usize) -> u32 {
            self.conn(i).ship
        }

        fn run(&mut self, ticks: usize) {
            for _ in 0..ticks {
                self.s.tick();
                for (a, msg) in std::mem::take(&mut self.s.outgoing) {
                    let Some(i) = self.c.iter().position(|c| c.addr == a) else { continue };
                    let Some(ClientMsg::GameData(p)) = netsock::parse_client(&msg) else { continue };
                    let (seq, _, subs) = packet::parse(p).unwrap();
                    let c = &mut self.c[i];
                    let mut w = PacketWriter::new(c.seq, seq);
                    c.seq += 1;
                    let mut replies: Vec<(u16, Vec<u8>)> = vec![];
                    for sub in subs {
                        match sub.kind {
                            packet::MSG_STARTUPDATA => {
                                let stage = proto::u32_at(sub.body, 0);
                                assert!(!startup_records(&sub.body[12..]).is_empty());
                                c.stage = c.stage.max(stage);
                                let mut b = 1u16.to_le_bytes().to_vec();
                                b.extend_from_slice(&stage.to_le_bytes());
                                w.event(packet::EV_STARTUP_ACK, c.tag, &b);
                                c.tag += 2;
                            }
                            packet::MSG_GAMEDATA => {
                                let snap = proto::parse_gamedata(sub.body).unwrap();
                                if snap.page_count > 0 && c.acked_pages.insert(snap.page_list) {
                                    let mut b = 1u16.to_le_bytes().to_vec();
                                    b.extend_from_slice(&(snap.page_list as u32).to_le_bytes());
                                    b.extend_from_slice(&snap.page_count.to_le_bytes());
                                    replies.push((ev::CL_DATA_ACK, b));
                                }
                                c.snaps.push(snap);
                            }
                            k if k >= 8 => {
                                if c.seen.insert(sub.tag.unwrap_or(0)) {
                                    if k == ev::EV_ENTER_QUADRANT {
                                        replies.push((ev::CL_DATA_ACK, vec![2, 0, 0, 0, 0, 0, 0, 0]));
                                    }
                                    c.events.push((k, sub.body.to_vec()));
                                }
                            }
                            _ => {}
                        }
                    }
                    replies.append(&mut c.out);
                    for (k, b) in replies {
                        w.event(k, c.tag, &b);
                        c.tag += 2;
                    }
                    let mut pd = 0u32.to_le_bytes().to_vec();
                    pd.extend_from_slice(&(c.seq * 16).to_le_bytes());
                    pd.extend_from_slice(&self.s.session.to_le_bytes());
                    pd.extend_from_slice(&player_data(&ShipState { pos: c.pos, valid: true, ..Default::default() }, c.seq * 16));
                    w.sub(packet::MSG_PLAYERDATA, &pd);
                    let addr = c.addr;
                    self.s.on_datagram(addr, &netsock::game_data(&w.finish()));
                }
            }
        }

        fn got(&self, i: usize, kind: u16) -> Vec<&Vec<u8>> {
            self.c[i].events.iter().filter(|(k, _)| *k == kind).map(|(_, b)| b).collect()
        }

        /// Client `from` reports laser hits on client `to` until `to` dies (or 400 hits).
        fn kill(&mut self, from: usize, to: usize) {
            let target = self.ship(to);
            for _ in 0..40 {
                if self.conn(to).dead {
                    return;
                }
                for _ in 0..10 {
                    let h = ev::Hit { pos: self.c[to].pos, target, weapon: 2, kind: 1, scale: 0x10000, part: None };
                    self.c[from].out.push((ev::CL_HIT, ev::hit(&h)));
                }
                self.run(16);
            }
        }
    }

    /// Two fake clients get through join and the six startup stages, and each shows up in the
    /// other's snapshots.
    #[test]
    fn join_handshake_and_snapshots() {
        let Some(mut h) = Harness::new("DM_01", 2) else { return };
        h.run(16 * 10);
        assert_eq!([h.c[0].stage, h.c[1].stage], [6, 6]);
        assert!(h.s.conns.iter().flatten().all(|c| !c.in_startup));
        let b = h.ship(1);
        assert!(h.c[0].snaps.iter().any(|s| s.records.iter().any(|r| r.id == b)), "the second player never appeared in the first one's snapshots");
        // `DM_GetRelation`: other players' records carry 2 (hostile) in the team byte.
        assert!(h.c[0].snaps.iter().flat_map(|s| s.records.iter()).filter(|r| r.id == b).all(|r| r.team == 2));
    }

    /// Slots and Netsock ids line up as on the stock host, and 0x2002 for a player ship gets
    /// the 16-byte class info with that player's slot.
    #[test]
    fn class_info_for_player_ships() {
        let Some(mut h) = Harness::new("DM_01", 2) else { return };
        h.run(16 * 10);
        for i in 0..2 {
            assert_eq!((h.conn(i).slot as usize, h.conn(i).netsock_id), (i + 1, 0x10001 + i as u32));
        }
        let other = h.ship(1);
        h.c[0].out.push((ev::CL_CLASS_INFO, other.to_le_bytes().to_vec()));
        h.run(48);
        let r = h.got(0, ev::EV_CLASS_INFO);
        assert_eq!(r.len(), 1);
        let desc = h.s.world.get(other).unwrap().desc_slot;
        assert_eq!(r[0].as_slice(), ev::class_info_player(other, desc, 2).as_slice());
        assert_eq!((r[0].len(), proto::u16_at(r[0], 0xa)), (16, 2));
        // The roster entry for that player uses the same slot number.
        assert!(h.got(0, ev::EV_ROSTER).iter().any(|b| proto::u16_at(b, 0) == 2));
    }

    /// 0x2001 is relayed as 0x1002 to the other player, not back to the shooter.
    #[test]
    fn fire_is_relayed() {
        let Some(mut h) = Harness::new("DM_01", 2) else { return };
        h.run(16 * 10);
        let f = ev::Fire { pos: h.c[0].pos, angles: [1, 2, 3], client_frame: 9, slot: 0, weapon: 6, volume: 0x80 };
        h.c[0].out.push((ev::CL_FIRE, ev::fire(&f)));
        h.run(48);
        let shots = h.got(1, ev::EV_FIRED);
        assert_eq!(shots.len(), 1);
        assert_eq!((proto::u32_at(shots[0], 0x18), shots[0][0x16]), (h.ship(0), 6));
        assert!(h.got(0, ev::EV_FIRED).is_empty());
    }

    /// Hit reports drain shields then hull; the kill sends 0x1006 to the victim, credits the
    /// killer, prints a kill line; 0x2011 respawns the victim with full power.
    #[test]
    fn kill_and_respawn() {
        let Some(mut h) = Harness::new("DM_01", 2) else { return };
        h.s.opts.debug_damage = true; // exercises the debug lines (shown with --nocapture)
        h.run(16 * 10);
        h.kill(0, 1);
        h.run(32);
        assert!(h.conn(1).dead, "400 heavy laser hits didn't kill");
        assert_eq!(h.got(1, ev::EV_YOU_DIED).len(), 1);
        assert!(!h.got(1, ev::EV_YOU_WERE_HIT).is_empty());
        assert_eq!((h.conn(0).kills, h.conn(1).deaths), (1, 1));
        let texts: Vec<String> = h.got(0, ev::EV_TEXT).iter().map(|b| ev::parse_text(b).2).collect();
        assert!(texts.iter().any(|t| t.contains("P0") && t.contains("P1")), "{texts:?}");
        // The dead ship drops out of the killer's snapshots.
        let victim = h.ship(1);
        assert!(!h.c[0].snaps.last().unwrap().records.iter().any(|r| r.id == victim));

        h.c[1].out.push((ev::CL_RESPAWN, vec![0, 0]));
        h.run(48);
        assert_eq!(h.got(1, ev::EV_TELEPORT).len(), 1);
        assert!(!h.conn(1).dead);
        let e = h.s.world.get(victim).unwrap();
        assert_eq!((e.power.hull, e.power.shield_front, e.net_flags), (e.power.hull_max, e.power.shield_max, 0));
    }

    /// At the kill limit everyone gets 0x1119; after the freeze the round resets (session bump,
    /// 0x1007 to each player, scores cleared) and resumes with 0x1120.
    #[test]
    fn kill_limit_ends_and_resets_the_round() {
        let Some(mut h) = Harness::new("DM_01", 2) else { return };
        h.s.opts.max_kills = 1;
        h.run(16 * 10);
        // Client 0 also carries an ammo weapon (21) with 3 of 20 left.
        assert!(weapons::by_id(21).is_some_and(|d| d.ammo != 0));
        h.s.conns[1].as_mut().unwrap().mounts[1] = Some((21, 20, 3));
        let session = h.s.session;
        h.kill(0, 1);
        h.run(32);
        for i in 0..2 {
            assert_eq!(h.got(i, ev::EV_ROUND_FREEZE).len(), 1, "client {i}");
        }
        h.run(rules::FREEZE_TICKS_SHORT as usize + 64);
        assert_eq!(h.s.session, session + 1);
        for i in 0..2 {
            assert_eq!(h.got(i, ev::EV_ENTER_QUADRANT).len(), 1, "client {i}");
            assert_eq!(h.got(i, ev::EV_ROUND_RESUME).len(), 1, "client {i}");
            let c = h.conn(i);
            assert_eq!((c.kills, c.deaths, c.dead, c.in_quadrant), (0, 0, false, true));
        }
        // The client's ammo is refilled too (weapon 6 has none, so only mount 1 is sent).
        assert_eq!(h.got(0, ev::EV_AMMO), vec![&ev::ammo(1, 20)]);
        assert!(h.got(1, ev::EV_AMMO).is_empty());
    }

    /// 0x2011 from a living player is ignored; the round timer pauses during a freeze; the
    /// minutes-left line arrives on a whole minute.
    #[test]
    fn respawn_guard_timer_pause_and_minutes_left() {
        let Some(mut h) = Harness::new("DM_01", 2) else { return };
        h.run(16 * 10);
        h.c[0].out.push((ev::CL_RESPAWN, vec![0, 0]));
        h.run(48);
        assert!(h.got(0, ev::EV_TELEPORT).is_empty(), "a living player respawned");
        // Freeze with the timer about to run out: it must not end the round a second time.
        h.s.opts.max_kills = 1;
        h.s.round_ticks = 100;
        h.kill(0, 1);
        h.run(200);
        assert_eq!(h.got(0, ev::EV_ROUND_FREEZE).len(), 1);
        assert!(h.s.round_ticks > 0 && h.s.round_ticks <= 100);
        // A dead player's request during the freeze is ignored too.
        h.c[1].out.push((ev::CL_RESPAWN, vec![0, 0]));
        h.run(48);
        assert!(h.got(1, ev::EV_TELEPORT).is_empty());
        h.run(rules::FREEZE_TICKS_SHORT as usize);
        // New round: 5 minutes plus a little.
        h.s.round_ticks = 5 * 3000 + 20;
        h.run(64);
        let texts: Vec<String> = h.got(0, ev::EV_TEXT).iter().map(|b| ev::parse_text(b).2).collect();
        assert!(texts.iter().any(|t| t == "5 MINUTES LEFT IN THIS ROUND"), "{texts:?}");
    }

    /// DM_01's three starbases: a root record with state flag 0x04 in the static pages, the
    /// 0x1100 base layout, and a damageable part (casarray's TYPE 12 comm array) destroyed by
    /// part hit reports (0x1003 effect 3, 0x1126, then listed as removed).
    #[test]
    fn starbases_parts_and_class_info() {
        let Some(mut h) = Harness::new("DM_01", 1) else { return };
        h.run(16 * 10);
        assert_eq!(h.s.bases.len(), 3);
        let roots: Vec<u32> = h.s.bases.keys().copied().collect();
        for r in &roots {
            let e = h.s.world.get(*r).unwrap();
            assert_eq!((e.state_flags & 4, e.net_flags), (4, 3));
        }
        // Ids are reserved for every part: nothing else lands inside a base's block.
        for (r, b) in &h.s.bases {
            let end = r + 2 * b.parts.len() as u32;
            assert!(h.s.world.entities.keys().all(|id| id == r || *id < *r || *id >= end));
        }
        let (root, part) = h
            .s
            .bases
            .iter()
            .find_map(|(r, b)| b.parts.iter().find(|p| p.flags & 0x800 != 0).map(|p| (*r, p.index)))
            .expect("casarray has a TYPE 12 part");
        h.c[0].out.push((ev::CL_CLASS_INFO, root.to_le_bytes().to_vec()));
        h.run(32);
        let info = h.got(0, ev::EV_CLASS_INFO);
        let desc = h.s.world.get(root).unwrap().desc_slot;
        assert_eq!(info.last().unwrap().as_slice(), ev::class_info_base(root, desc, 0, 0, &[]).as_slice());
        // A hull part can't be damaged.
        let root_pos = h.s.world.get(root).unwrap().pos;
        let hit = |part: u16| ev::hit(&ev::Hit { pos: root_pos, target: root, weapon: 2, kind: 2, scale: 0x10000, part: Some(part) });
        let hull_hit = hit(0);
        h.c[0].out.push((ev::CL_PART_HIT, hull_hit));
        h.run(16);
        assert!(h.s.bases[&root].parts[0].alive);
        // Fly to the part so its explosion (radius 1000) reaches us.
        h.c[0].pos = h.s.bases[&root].parts[part as usize].pos;
        h.run(32);
        // Heavy laser: 7500 x 153/256 = 4482 a hit against 20000 hp -> 5 hits.
        for _ in 0..5 {
            let b = hit(part);
            h.c[0].out.push((ev::CL_PART_HIT, b));
        }
        h.run(32);
        assert!(!h.s.bases[&root].parts[part as usize].alive);
        assert_eq!(h.got(0, ev::EV_SUBPART_DESTROY), vec![&ev::subpart_destroy(root, part)]);
        assert!(h.got(0, ev::EV_DESTROYED).iter().any(|b| b[0x14] == 3));
        h.c[0].out.push((ev::CL_CLASS_INFO, root.to_le_bytes().to_vec()));
        h.run(32);
        assert_eq!(h.got(0, ev::EV_CLASS_INFO).last().unwrap().as_slice(), ev::class_info_base(root, desc, 0, 0, &[part]).as_slice());
    }

    /// Powerups: a full-shield ship doesn't use a shield powerup; a damaged one does (text,
    /// 0x1117, 0x1011, no collision damage), and it comes back after its delay with 0x1106.
    #[test]
    fn powerups_pickup_and_respawn() {
        let Some(mut h) = Harness::new("DM_01", 1) else { return };
        assert_eq!(h.s.pickups.len(), 43);
        h.run(16 * 10);
        let (id, pk) = h.s.pickups.iter().find(|(_, p)| p.kind == objects::PUP_SHIELDS).map(|(i, p)| (*i, p.clone())).unwrap();
        assert_eq!(pk.respawn, 60 * 50);
        let touch = |id: u32| ev::collision(500, id);
        h.c[0].out.push((ev::CL_COLLISION, touch(id)));
        h.run(32);
        assert!(!h.s.pickups[&id].taken(), "full shields still took it");
        let ship = h.ship(0);
        let hull = h.s.world.get(ship).unwrap().power.hull;
        h.s.world.get_mut(ship).unwrap().power.shield_front = 100;
        h.c[0].out.push((ev::CL_COLLISION, touch(id)));
        h.run(32);
        assert!(h.s.pickups[&id].taken());
        let p = h.s.world.get(ship).unwrap().power;
        assert_eq!((p.shield_front, p.hull), ((100 + pk.amount).min(p.shield_max), hull), "touching a powerup did damage");
        assert_eq!(h.got(0, ev::EV_RESUPPLY), vec![&ev::resupply(1, pk.amount as u32)]);
        assert!(h.got(0, ev::EV_TEXT).iter().any(|b| ev::parse_text(b).2 == "SHIELDS REPLENISHED"));
        assert_eq!(h.got(0, ev::EV_DESPAWN), vec![&ev::despawn(id)]);
        // Taken: a second touch does nothing; it's back after 60 s.
        h.c[0].out.push((ev::CL_COLLISION, touch(id)));
        h.run(60 * 50 + 32);
        assert_eq!(h.got(0, ev::EV_RESUPPLY).len(), 1);
        assert!(!h.s.pickups[&id].taken());
        let spawn = h.got(0, ev::EV_SPAWN_ENTITY);
        assert_eq!(spawn.len(), 1);
        assert_eq!(proto::u32_at(spawn[0], 0), id);
        assert_eq!(spawn[0].len(), 0x28);
    }

    /// Gates, mines, beacons and speed rings exist with the original's state flags and answer
    /// 0x2002 with their class info (sizes 0x24 / 0xc / 8 / 0xc).
    #[test]
    fn markers_spawn_with_class_info() {
        let cases: [(&str, &str, usize, usize, u8); 4] =
            [("BW_02", "HYPER_GATE", 2, 0x24, 0x45), ("BW_02", "MINE", 8, 0xc, 5), ("DM_04", "BEACON", 1, 8, 5), ("basewar1", "SPEED_RING", 6, 0xc, 4)];
        for (map, kind, count, size, flags) in cases {
            let Some(mut h) = Harness::new(map, 1) else { return };
            h.run(16 * 10);
            let ids: Vec<u32> = h.s.world.order.iter().copied().filter(|id| h.s.world.get(*id).is_some_and(|e| e.des.desc_type == kind)).collect();
            assert_eq!(ids.len(), count, "{map} {kind}");
            for &id in &ids {
                let e = h.s.world.get(id).unwrap();
                assert_eq!((e.state_flags, e.net_flags), (flags, 3), "{map} {kind}");
                h.c[0].out.push((ev::CL_CLASS_INFO, id.to_le_bytes().to_vec()));
            }
            h.run(32);
            let infos = h.got(0, ev::EV_CLASS_INFO);
            for &id in &ids {
                let b = infos.iter().find(|b| proto::u32_at(b, 0) == id).unwrap_or_else(|| panic!("{map} {kind}: no class info"));
                assert_eq!(b.len(), size, "{map} {kind}");
                if kind == "HYPER_GATE" {
                    let name: String = b[0xc..].iter().take_while(|&&c| c != 0).map(|&c| c as char).collect();
                    println!("{map} gate {id:#x}: open {} sector {} flags {} name {name:?}", proto::u16_at(b, 6), proto::u16_at(b, 8), proto::u16_at(b, 10));
                    assert!(!name.is_empty());
                }
                if kind == "SPEED_RING" {
                    assert_eq!(proto::u32_at(b, 8), 245_760); // SPEED_BOOST 1500 x 163.84
                }
            }
        }
    }

    /// Asteroids reach clients as static pages: 4 extended records per snapshot (12 while
    /// loading), repeated until acked, nearest first, with the descriptor slot.
    #[test]
    fn asteroids_arrive_in_static_pages() {
        let Some(mut h) = Harness::new("DM_02", 1) else { return };
        let total = h.s.world.entities.values().filter(|e| e.net_flags & 3 == 3).count();
        assert!(total > 800, "{total}");
        h.run(16 * 40);
        let paged: Vec<_> = h.c[0].snaps.iter().flat_map(|s| s.records.iter()).filter(|r| r.flags & proto::REC_PAGED != 0).collect();
        assert!(paged.len() >= 4 * 20, "{}", paged.len());
        let asteroid = |id: u32| h.s.world.get(id).is_some_and(|e| e.des.desc_type == "ASTEROID");
        assert!(paged.iter().filter(|r| asteroid(r.id)).all(|r| r.desc.is_some_and(|d| d >= 0)));
        // Starbases and powerups go in the pages too, without a descriptor (net flag 4 clear).
        let odd: Vec<_> = paged.iter().filter(|r| !asteroid(r.id) && !(r.desc.is_none() && r.flags & 4 != 0)).map(|r| (r.id, r.desc, r.flags, h.s.world.get(r.id).map(|e| e.des.desc_type.clone()))).collect();
        assert!(odd.is_empty(), "{odd:?}");
        assert!(h.c[0].snaps.iter().all(|s| s.page_count <= 4));
        let c = h.conn(0);
        assert!(c.static_acked >= 4 * 20 && c.static_acked < total, "{}", c.static_acked);
    }
}
