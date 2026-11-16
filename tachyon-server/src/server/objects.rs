//! Map objects beyond asteroids: starbases (OCF part entities) and powerups.
//!
//! References: `decomp/notes/multiplayer.md` "Starbases and powerups", `capships.md` (part
//! records), `shields.md` (resupply).
//!
//! **Starbases** (`Base_SpawnFromPlacement` → `Base_SpawnParts` → `BasePart_Spawn`): one server
//! entity per OCF part, ids `root + 2·index` (`NetId_Reserve`). Only the root goes on the wire: a
//! static paged record with state flag 0x04, so each client asks with 0x2002 and builds the parts
//! itself from the 0x1100 reply (`Base_InitClientParts`), skipping the removed ones. The other
//! parts exist only on the server (net flags 1). Parts flagged `0x6918` (weapon/shield/power
//! generators, TYPE 12, credit powerplant, life support) take damage through 0x2027 hit reports;
//! the hull and decorative parts don't.
//!
//! **Powerups** (`Powerup_Spawn`, `Powerup_OnPickup`, `Powerup_Apply`): a static paged record with
//! state flag 0x04 (6-byte class info). A player touching one reports a collision (0x2009) with it;
//! the server applies it, despawns it, and respawns it after the placement's delay.

use super::*;
use tachyon_core::math::Mat3;
use tachyon_core::ocf::Ocf;
use tachyon_core::source::AssetSource;

/// `Ocf_ParsePart` flags (`1 << (TYPE − 1)`) that make a part damageable (`BasePart_InitDamageable`).
const PART_DAMAGEABLE: u32 = 0x6918;
/// TYPE 15 (life support) parts use a different handler (`LAB_0040a400`, Base Wars).
const PART_LIFE_SUPPORT: u32 = 0x4000;
/// Credit powerplant (`CREDIT_POWERPLANT`, TYPE 14).
const PART_CREDIT_PLANT: u32 = 0x2000;
/// Placement flag 0x400: the root gets state flag 0x20 (`Sped_GetCollFlags`).
const PLACEMENT_COLL_FLAG: u32 = 0x400;
/// Powerup types (`Powerup_TypeFromName`, table `0x548c18`).
pub const PUP_SHIELDS: u32 = 1;
pub const PUP_HULL: u32 = 2;
pub const PUP_LASER: u32 = 3;
pub const PUP_BURNER: u32 = 4;
pub const PUP_ROCKETS: u32 = 5;
pub const PUP_MISSILES: u32 = 6;
pub const PUP_SAPPERS: u32 = 7;
pub const PUP_SUPER: u32 = 8;
pub const PUP_PARTIAL: u32 = 9;
pub const PUP_CREDITS: u32 = 10;
const PUP_NAMES: [&str; 10] = ["SHIELDS", "HULL", "LASER_ENERGY", "BURNER_ENERGY", "ROCKET_AMMO", "MISSILE_AMMO", "SAPPER_AMMO", "SUPER_POWERUP", "PARTIAL_ENERGY", "CREDIT_CRYSTAL"];
/// Resupply lines, `ingame.bin` 0x66..0x6f, sent with `Resupply_SendText` (kind 1, colour −1).
const TXT_SHIELDS: &str = "SHIELDS REPLENISHED";
const TXT_LASER: &str = "WEAPON ENERGY REPLENISHED";
const TXT_BURNER: &str = "AFTERBURNER ENERGY REPLENISHED";
const TXT_HULL: &str = "HULL REPAIRED";
const TXT_ROCKETS: &str = "ROCKET STORES REPLENISHED";
const TXT_SAPPERS: &str = "SAPPER STORES REPLENISHED";
const TXT_MISSILES: &str = "MISSILE STORES REPLENISHED";
const TXT_PARTIAL: &str = "PARTIAL ENERGY RESTORED TO ALL SYSTEMS";
const TXT_SUPER: &str = "ALL SYSTEMS REPLENISHED TO FULL";

/// One OCF part of a starbase, server side.
#[derive(Debug, Clone)]
pub struct BasePart {
    /// Part record `+0x44`: its position in the OCF.
    pub index: u16,
    pub flags: u32,
    /// `+0xbc`; only meaningful for damageable parts.
    pub hp: i32,
    pub alive: bool,
    /// World position (16.16), for the explosion when it dies.
    pub pos: [i32; 3],
}

#[derive(Debug, Clone)]
pub struct Base {
    pub parts: Vec<BasePart>,
    /// `BaseRemovedList`: part indices the client must not build.
    pub removed: Vec<u16>,
    /// `+0xfe` (`Starbase_SetDockState`); 0 until docking exists.
    pub dock_state: u16,
}

#[derive(Debug, Clone)]
pub struct Pickup {
    pub kind: u32,
    pub amount: i32,
    /// Placement position and jitter (`+0x3c`, half-width in units).
    pub home: [i32; 3],
    pub jitter: u32,
    /// Respawn delay in ticks (placement `+0x38` seconds × 50).
    pub respawn: u32,
    /// Ticks until it comes back; 0 while it's there.
    pub timer: u32,
}

impl Pickup {
    pub fn taken(&self) -> bool {
        self.timer != 0
    }
}

/// Every part's offset from the root in model units, root orientation not applied
/// (`BaseFile_ComputePartTransforms`).
fn part_offsets(src: &AssetSource, ocf: &Ocf) -> Vec<[f32; 3]> {
    let mut paks = std::collections::HashMap::new();
    for p in &ocf.parts {
        let key = p.pak_file.to_uppercase();
        if !paks.contains_key(&key)
            && let Ok(pak) = src.read(&p.pak_file).and_then(|raw| tachyon_core::pak::parse(&raw))
        {
            paks.insert(key, pak);
        }
    }
    // `ocf::place` skips parts whose pak is missing; rebuild per-index offsets from it.
    let (placed, _) = tachyon_core::ocf::place(ocf, &paks);
    let mut it = placed.into_iter();
    ocf.parts
        .iter()
        .map(|p| if paks.contains_key(&p.pak_file.to_uppercase()) { it.next().map_or([0.0; 3], |pl| pl.offset) } else { [0.0; 3] })
        .collect()
}

fn engine_deg(a: i32) -> f32 {
    a as f32 * 360.0 / 512.0
}

impl Server {
    /// `Base_SpawnFromPlacement`: the root entity, the server-side part list, and the id block.
    pub(super) fn spawn_starbase(&mut self, p: &tachyon_core::spx::Placement, des: Rc<Des>) {
        let Some(ocf) = self.map.ocf_for(&des).cloned() else { return };
        if ocf.parts.is_empty() {
            return;
        }
        let Some((desc_slot, _)) = self.world.tables.add_des(&des) else { return };
        let key = des.ocf_file.clone().unwrap_or_default().to_uppercase();
        if !self.part_offsets.contains_key(&key) {
            let offs = part_offsets(&self.map.src, &ocf);
            self.part_offsets.insert(key.clone(), offs);
        }
        let offsets = &self.part_offsets[&key];
        let pos = p.pos.map(|v| (v * 65536.0) as i32);
        let angles = p.orient.map(|a| (a << 16) & ANGLE_MASK);
        let rot = Mat3::from_engine_euler_deg(p.orient.map(engine_deg));
        let parts: Vec<BasePart> = ocf
            .parts
            .iter()
            .enumerate()
            .map(|(i, op)| {
                let flags = if op.part_type != 0 { 1u32 << ((op.part_type - 1) & 31) } else { 0 };
                let hp = op.hitpoints.unwrap_or(if flags & PART_CREDIT_PLANT != 0 {
                    400_000
                } else if flags & PART_LIFE_SUPPORT != 0 {
                    20_000 + 0x1e3660
                } else {
                    20_000
                });
                let off = rot.mul_vec(offsets.get(i).copied().unwrap_or([0.0; 3]));
                BasePart { index: i as u16, flags, hp, alive: true, pos: std::array::from_fn(|k| pos[k].wrapping_add((off[k] * 65536.0) as i32)) }
            })
            .collect();
        // Team: placement +0x38 low byte − 1, or the `.des` race when 0.
        let team = match p.extra[0] & 0xff {
            0 => self.map.races.index(&des.race).unwrap_or(0) as u8,
            t => (t - 1) as u8,
        };
        let id = self.world.alloc_id();
        // `NetId_Reserve`: the parts take root + 2·index.
        for _ in 1..parts.len() {
            self.world.alloc_id();
        }
        let scale = (ocf.parts[0].scale * 65536.0) as i32;
        self.world.spawn(Entity {
            id,
            owner: Owner::Static,
            des,
            desc_slot,
            // Starbase paks live in the client's own base pak pool, not the model table; the
            // client replaces the model when the class info arrives (`CapPart_InitReplica`).
            model: 0,
            pos,
            vel: [0; 3],
            angles,
            net_pos: pos,
            net_angles: angles,
            state_flags: 4 | if p.flags & PLACEMENT_COLL_FLAG != 0 { 0x20 } else { 0 },
            throttle: 0,
            team,
            net_flags: 3,
            scale,
            power: Power::default(),
            target: 0,
            hp: 0,
        });
        self.bases.insert(id, Base { parts, removed: vec![], dock_state: 0 });
    }

    /// `Powerup_Spawn`: only credit crystals unless the mode's POWERUPS option is on.
    pub(super) fn spawn_powerup(&mut self, p: &tachyon_core::spx::Placement, des: Rc<Des>) {
        let (kind, amount) = des
            .group("MAIN")
            .and_then(|g| g.get("TYPE"))
            .map(|it| {
                let name = it.values.first().and_then(|v| v.as_str()).unwrap_or("");
                let kind = PUP_NAMES.iter().position(|n| n.eq_ignore_ascii_case(name)).map_or(0, |i| i as u32 + 1);
                (kind, it.values.get(1).and_then(|v| v.as_i32()).unwrap_or(0))
            })
            .unwrap_or((0, 0));
        if kind != PUP_CREDITS && !self.opts.powerups {
            return;
        }
        let Some((desc_slot, model)) = self.world.tables.add_des(&des) else { return };
        let home = p.pos.map(|v| (v * 65536.0) as i32);
        let jitter = p.extra[1];
        // `Powerup_IsAllowed`: credit crystals scale with `spacesav[0x6d]` (1 = as placed).
        let respawn = p.extra[0].wrapping_mul(50);
        let angles = p.orient.map(|a| (a << 16) & ANGLE_MASK);
        let id = self.world.alloc_id();
        let mut pk = Pickup { kind, amount, home, jitter, respawn, timer: 0 };
        let pos = self.pickup_position(&mut pk);
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
            state_flags: 4,
            throttle: 0,
            team: 0,
            net_flags: 3,
            scale: FX,
            power: Power::default(),
            target: 0,
            hp: 0,
        });
        self.pickups.insert(id, pk);
    }

    /// `Powerup_PickPosition`: the placement position ± rand(jitter·0x2000) − jitter·0x1000 per axis.
    fn pickup_position(&mut self, pk: &mut Pickup) -> [i32; 3] {
        if pk.jitter == 0 {
            return pk.home;
        }
        let span = pk.jitter << 13;
        let half = ((pk.jitter & 0x7ffff) as i32).wrapping_mul(0x1000);
        std::array::from_fn(|k| pk.home[k].wrapping_add((self.rand() % span) as i32).wrapping_sub(half))
    }

    /// 0x2002 for a starbase root or a powerup; false if `id` is neither.
    pub(super) fn object_class_info(&mut self, slot: u16, id: u32) -> bool {
        let Some(e) = self.world.get(id) else { return false };
        let desc_slot = e.desc_slot;
        let body = if let Some(b) = self.bases.get(&id) {
            ev::class_info_base(id, desc_slot, 0, b.dock_state, &b.removed)
        } else if self.pickups.contains_key(&id) {
            ev::class_info_plain(id, desc_slot)
        } else if let Some(tail) = self.class_tails.get(&id) {
            let mut b = ev::class_info_plain(id, desc_slot);
            b.extend_from_slice(tail);
            b
        } else {
            return false;
        };
        self.queue(ev::EV_CLASS_INFO, body, Delivery::only(slot));
        true
    }

    /// 0x2027 on a starbase (`CapShip_DamagePartById` → the part's `+0x80`): only parts flagged
    /// 0x6918 take damage (`BasePart_OnDamage`): hp −= hull factor × damage >> 8; at 0 the part
    /// explodes (0x1003 effect 3), joins the removed list and every client drops it (0x1126).
    /// Returns true if `root` is a starbase.
    pub(super) fn damage_base_part(&mut self, root: u32, part: u16, d: &combat::Damage, shooter: u16) -> bool {
        let debug = self.opts.debug_damage;
        let Some(b) = self.bases.get_mut(&root) else { return false };
        let Some(p) = b.parts.iter_mut().find(|p| p.index == part && p.alive) else { return true };
        if p.flags & PART_DAMAGEABLE == 0 || p.flags & PART_LIFE_SUPPORT != 0 || p.hp <= 0 {
            if debug {
                println!("    starbase {root:#x} part {part} (flags {:#x}) can't be damaged", p.flags);
            }
            return true;
        }
        let before = p.hp;
        p.hp -= ((d.hull_factor as i64 * d.amount as i64) as u32 >> 8) as i32;
        let (hp, pos) = (p.hp, p.pos);
        if debug {
            println!("    starbase {root:#x} part {part}: {} x hull {}/256 -> hp {before}->{hp}", d.amount, d.hull_factor);
        }
        if hp >= 1 {
            // `Weapon_PlayImpact(weapon, 0, 2, …)`: a small explosion at the hit, on top of the
            // hit report's own impact.
            self.play_impact(d.weapon, 2, d.hit_pos, d.hit_pos, shooter);
            return true;
        }
        p.alive = false;
        b.removed.push(part);
        self.queue(ev::EV_DESTROYED, ev::destroyed(pos, 0, 3, 0), Delivery { budget: 4, flags: D_RADIUS | D_QUADRANT | D_SYNCED, radius: EFFECT_RADIUS[3] * FX as i64, pos, ..Default::default() });
        self.queue(ev::EV_SUBPART_DESTROY, ev::subpart_destroy(root, part), Delivery { budget: BUDGET_RELIABLE, flags: D_QUADRANT, ..Default::default() });
        if debug {
            println!("    starbase {root:#x} part {part} destroyed");
        }
        true
    }

    /// The collision path's desc `+0x70` hook (`Powerup_OnPickup`). Returns true if `id` is a
    /// powerup, in which case the collision does no damage.
    pub(super) fn touch_pickup(&mut self, slot: u16, id: u32) -> bool {
        let Some(pk) = self.pickups.get(&id).cloned() else { return false };
        if pk.taken() {
            return true;
        }
        if self.apply_pickup(slot, &pk) {
            self.queue(ev::EV_DESPAWN, ev::despawn(id), Delivery { budget: BUDGET_RELIABLE, flags: D_QUADRANT, ..Default::default() });
            self.hide_static(id);
            if let Some(e) = self.world.get_mut(id) {
                e.net_flags = 1;
            }
            if let Some(p) = self.pickups.get_mut(&id) {
                // Respawns after `respawn` more ticks (`Powerup_Respawn` counts down to 0 first).
                p.timer = p.respawn + 1;
            }
            if self.opts.debug_damage {
                let name = self.conn(slot).map_or("?".to_string(), |c| c.name.clone());
                println!("PICKUP {name} took {} ({}), back in {} s", PUP_NAMES.get(pk.kind as usize - 1).unwrap_or(&"?"), pk.amount, pk.respawn / 50);
            }
        }
        true
    }

    /// `Powerup_Apply`. Returns whether the powerup was used up.
    fn apply_pickup(&mut self, slot: u16, pk: &Pickup) -> bool {
        let Some(c) = self.conn(slot) else { return false };
        let ship = c.ship;
        let Some(e) = self.world.get_mut(ship) else { return false };
        let p = &mut e.power;
        let give_shields = |p: &mut Power, n: i32| {
            p.shield_front = p.shield_front.saturating_add(n).min(p.shield_max);
            p.shield_rear = p.shield_rear.saturating_add(n).min(p.shield_max);
        };
        let give_hull = |p: &mut Power, n: i32| p.hull = p.hull.saturating_add(n).min(p.hull_max);
        let text = match pk.kind {
            PUP_SHIELDS => {
                if p.shield_front >= p.shield_max && p.shield_rear >= p.shield_max {
                    return false;
                }
                give_shields(p, pk.amount);
                TXT_SHIELDS
            }
            PUP_HULL => {
                if p.hull >= p.hull_max {
                    return false;
                }
                give_hull(p, pk.amount);
                TXT_HULL
            }
            PUP_LASER => TXT_LASER,
            PUP_BURNER => TXT_BURNER,
            PUP_ROCKETS | PUP_MISSILES | PUP_SAPPERS => return self.give_ammo(slot, pk),
            PUP_SUPER => {
                give_shields(p, 100_000);
                give_hull(p, 100_000);
                TXT_SUPER
            }
            PUP_PARTIAL => {
                give_shields(p, pk.amount);
                give_hull(p, pk.amount);
                TXT_PARTIAL
            }
            // Credit crystals (`Resupply_GiveCredits`) need Base Wars credits (M6).
            _ => return false,
        };
        self.resupply_text(slot, text);
        self.queue(ev::EV_RESUPPLY, ev::resupply(pk.kind as u16, pk.amount as u32), Delivery::only(slot));
        true
    }

    /// `Resupply_GiveAmmo`: if any mount with the powerup's AI usage bits (rockets 0x10,
    /// missiles 2, sappers 8) is below max, add the amount to every such mount, capped, and tell
    /// the client each count (0x1124).
    fn give_ammo(&mut self, slot: u16, pk: &Pickup) -> bool {
        let bits = match pk.kind {
            PUP_ROCKETS => 0x10,
            PUP_MISSILES => 2,
            _ => 8,
        };
        let Some(c) = self.conn_mut(slot) else { return false };
        let matches = |w: u8| weapons::ai_usage(w) == bits;
        if !c.mounts.iter().flatten().any(|&(w, max, ammo)| matches(w) && ammo < max) {
            return false;
        }
        let mut sends = vec![];
        for (i, m) in c.mounts.iter_mut().enumerate() {
            if let Some((w, max, ammo)) = m
                && matches(*w)
            {
                *ammo = (*ammo as i32 + pk.amount).clamp(0, *max as i32) as u16;
                sends.push((i as u16, *ammo));
            }
        }
        for (i, n) in sends {
            self.queue(ev::EV_AMMO, ev::ammo(i, n), Delivery::only(slot));
        }
        let text = match pk.kind {
            PUP_ROCKETS => TXT_ROCKETS,
            PUP_MISSILES => TXT_MISSILES,
            _ => TXT_SAPPERS,
        };
        self.resupply_text(slot, text);
        self.queue(ev::EV_RESUPPLY, ev::resupply(pk.kind as u16, pk.amount as u32), Delivery::only(slot));
        true
    }

    fn resupply_text(&mut self, slot: u16, text: &str) {
        self.queue(ev::EV_TEXT, ev::text(1, 0xffff, text), Delivery::only(slot));
    }

    /// `Powerup_Respawn`, every tick: count down, then pick a position and send 0x1106.
    pub(super) fn pickups_tick(&mut self) {
        let due: Vec<u32> = self
            .pickups
            .iter_mut()
            .filter_map(|(id, p)| {
                if p.timer == 0 {
                    return None;
                }
                p.timer -= 1;
                (p.timer == 0).then_some(*id)
            })
            .collect();
        for id in due {
            let mut pk = self.pickups[&id].clone();
            let pos = self.pickup_position(&mut pk);
            let Some(e) = self.world.get_mut(id) else { continue };
            e.pos = pos;
            e.net_pos = pos;
            e.net_flags = 3;
            let mut rec = vec![];
            proto::write_record(&mut rec, &self.relation_record(id), pos);
            self.queue(ev::EV_SPAWN_ENTITY, ev::spawn_entity(&rec, pos), Delivery { budget: BUDGET_RELIABLE, flags: D_QUADRANT, ..Default::default() });
        }
    }

    /// The extended record a static sends in 0x1106. It goes to the whole quadrant at once, so
    /// the relation byte can't be per viewer; statics aren't players, so Deathmatch gives 4.
    fn relation_record(&self, id: u32) -> RecordOut {
        let e = self.world.get(id).unwrap();
        let mut r = e.record(0, true);
        r.team = self.relation(e, 0);
        r
    }
}

/// `Speed_HudToRaw`: HUD speed units → raw per-frame (× 65536 / 400).
fn speed_hud_to_raw(v: i32) -> u32 {
    (v as f32 * 163.84).round() as i32 as u32
}

impl Server {
    /// Hyperspace gates, beacons, mines and speed rings, built for drawing only: the entity and
    /// its class info (the client asks for it because of state flag 0x04). Gate travel, armed
    /// mines and speed boosts come later.
    ///
    /// - **Gate** (`Gate_SpawnAndPreload`): state flags 0x44, | 1 unless regional (+0x58). Class
    ///   info (`LAB_004268a0`, 0x24 bytes): `u16 open @6` (1: the gate variable is unset in a
    ///   session), `u16 destination quadrant id @8`, `u16 flags @10` (1 always,
    ///   `Gate_UpdateCollideState`; 2 regional), `char name[0x18] @0xc` = the destination's
    ///   quadrant name from the region table (`Gate_InitLocal` → `Sector_GetQuadrantName`,
    ///   NOVAWORL.BIN), "GATE" if unresolved.
    /// - **Beacon** (`Ent_SpawnAiDcb`): state 4 | 1 unless placement flag 0x10 | 0x20 with 0x400.
    ///   Class info (`LAB_0040e8c0`, 8 bytes): `u16 @6` = 1 unless flag 0x10 (the same `+0x10c`).
    /// - **Mine** (`Mine_Spawn`): state 4 | 1 unless flag 0x10; team as starbases. Class info
    ///   (`LAB_00433d30`, 0xc bytes): `u16 @6` = placement +0x60 if flag 0x80 else 0xffff,
    ///   `u16 @8` = placement +0x54, `u8 @10` = entity +0xf4 (0).
    /// - **Speed ring** (`SpawnPoint_SpawnAi`): state 4. Class info (`SpeedRing_GetInfo`, 0xc
    ///   bytes): `u32 @8` = `SPEED_BOOST` in raw speed units.
    ///
    /// Beacons, mines and rings spawn only when the placement's condition variable (+0x3c) is
    /// unset (`Spx_TestVar`; sessions have no mission variables).
    pub(super) fn spawn_marker(&mut self, p: &tachyon_core::spx::Placement, des: Rc<Des>) {
        let kind = des.desc_type.clone();
        if kind != "HYPER_GATE" && p.u16_at(0x3c) != 0xffff {
            return;
        }
        let Some((desc_slot, model)) = self.world.tables.add_des(&des) else { return };
        let no_fx = p.flags & 0x10 == 0;
        let mut team = 0u8;
        let (state_flags, tail): (u8, Vec<u8>) = match kind.as_str() {
            "HYPER_GATE" => {
                let regional = p.u32_at(0x58) != 0;
                let dest = p.str_at(0x38, 0x10);
                let (sector, name) = if dest.is_empty() || regional { (0xffffu16, String::new()) } else { self.gate_destination(&dest) };
                let mut t = vec![0u8; 0x24 - 6];
                t[0..2].copy_from_slice(&1u16.to_le_bytes());
                t[2..4].copy_from_slice(&sector.to_le_bytes());
                t[4..6].copy_from_slice(&(1u16 | if regional { 2 } else { 0 }).to_le_bytes());
                for (i, b) in name.bytes().take(0x17).enumerate() {
                    t[6 + i] = b;
                }
                (0x44 | if regional { 0 } else { 1 }, t)
            }
            "BEACON" => (4 | no_fx as u8 | if p.flags & 0x400 != 0 { 0x20 } else { 0 }, (no_fx as u16).to_le_bytes().to_vec()),
            "MINE" => {
                team = match p.extra[0] & 0xff {
                    0 => self.map.races.index(&des.race).unwrap_or(0) as u8,
                    t => (t - 1) as u8,
                };
                let mut t = vec![0u8; 6];
                let group = if p.flags & 0x80 != 0 { p.u16_at(0x60) } else { 0xffff };
                t[0..2].copy_from_slice(&group.to_le_bytes());
                t[2..4].copy_from_slice(&p.u16_at(0x54).to_le_bytes());
                (4 | no_fx as u8, t)
            }
            "SPEED_RING" => {
                let boost = des_num(&des, "MAIN", "SPEED_BOOST") as i32;
                let mut t = vec![0u8; 2];
                t.extend_from_slice(&speed_hud_to_raw(boost).to_le_bytes());
                (4, t)
            }
            _ => return,
        };
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
            state_flags,
            throttle: 0,
            team,
            net_flags: 3,
            scale: FX,
            power: Power::default(),
            target: 0,
            hp: 0,
        });
        self.class_tails.insert(id, tail);
    }

    /// A local gate's destination (`Spx_LoadSector` + `Gate_InitLocal`): that sector's quadrant
    /// id and its display name from NOVAWORL.BIN (`MpRules_LoadRegion` loads the NOVAWORLD
    /// region table), falling back to "GATE".
    fn gate_destination(&self, spx: &str) -> (u16, String) {
        let Ok(sector) = self.map.src.read(spx).and_then(|d| tachyon_core::spx::parse(&d)) else {
            return (0, "GATE".into());
        };
        let name = self
            .map
            .src
            .read("NOVAWORL.BIN")
            .and_then(|d| tachyon_core::rtxt::Rtxt::parse(&d))
            .ok()
            .and_then(|r| r.find_key(&sector.quad).map(str::to_string))
            .unwrap_or_else(|| "GATE".into());
        (sector.sector_id as u16, name)
    }
}
