//! Server-owned weapons: guided missiles, torpedoes, the Helios rocket, blast spheres and
//! decoys. Reference: `decomp/notes/multiplayer.md` "Server weapons", `weapons.md` "Missile
//! and torpedo guidance".
//!
//! A 0x2004 from a client creates a real entity (`Proj_CreateMissile`) that rides in the normal
//! snapshots (state flags 6: projectile replica + class info, throttle byte = weapon id, model =
//! the weapon's pool slot, descriptor slot 0 = `weapon_desc`). The server flies it, hit-tests it
//! every 8 frames from where it was at the last test, applies the damage through the same
//! handlers as client hit reports, and removes it with 0x1014 op 8. Blast torpedoes turn into
//! an invisible damage sphere announced with 0x1008.
//!
//! Hit tests use spheres: the target's model radius × the weapon's cast scale (3.0 for every
//! missile and torpedo, 1.0 for targets without a hit scale). The original uses that sphere
//! only as a broad phase before a mesh test at the same scale (`DEVIATIONS.md` "Missile hit
//! tests").

use super::*;
use tachyon_core::math::Mat3;
use tachyon_core::weapons::{MissileRaw, LAUNCH_FRAMES};

/// `Missile_CheckDecoys`: decoys within 162.5 units ...
const DECOY_RANGE: i64 = 0xa2_8000;
/// ... and within ≈19.5° of the nose on yaw and pitch.
const DECOY_CONE: i32 = 0x1c_71c6;
/// More than 45° off on yaw or pitch loses the lock (`Proj_ThinkMissile`).
const LOCK_CONE: i32 = 0x40_0000;
/// Decoy life (`Decoy_Spawn`).
const DECOY_TICKS: u32 = 250;
/// 0x1104 goes to everyone within 1250 units except the dropper.
const DECOY_RELAY_RADIUS: i64 = 1250;
/// 0x1008 radius.
const BLAST_NOTICE_RADIUS: i64 = 625;
/// Hunter torpedo re-acquire radius (250 units) and the Blast torpedo's proximity fuse
/// (43.75), Adv Blast's target fuse (6.25).
const HUNTER_RANGE: i64 = 0xfa_0000;
const PROXIMITY_FUSE: i64 = 0x2b_c000;
const ADV_BLAST_FUSE: i64 = 0x6_4000;
/// `Blast_Init` / `Blast_HoldThink` / `Blast_Think`.
const BLAST_HOLD: i32 = 7;
const BLAST_RESERVOIR: i32 = 0xa2_8000;

const W_HUNTER: u8 = 8;
const W_BLAST: u8 = 9;
const W_ADV_BLAST: u8 = 10;

#[derive(Debug, Clone)]
pub struct Blast {
    /// Entity scale (+0x1c, 16.16), starts at 2.0.
    pub scale: i32,
    /// +0x2c, 0..255; also the damage scale (<< 8).
    pub intensity: i32,
    /// +0xcc: scale growth per frame.
    pub growth: i32,
    /// +0xec: reservoir decay per frame.
    pub decay: i32,
    /// +0xd4: hold frames, then the reservoir.
    pub counter: i32,
    pub holding: bool,
}

#[derive(Debug, Clone)]
pub struct Missile {
    pub weapon: u8,
    pub raw: &'static MissileRaw,
    /// The shooter's ship (+200), 0 once it's gone.
    pub shooter: u32,
    /// +0x90.
    pub target: u32,
    /// +0xec: current max turn per frame.
    pub turn: i32,
    /// +0xd4: launch frames, then life.
    pub counter: i32,
    pub launching: bool,
    /// +0xd0: staggers the 8- and 32-frame checks.
    pub serial: u32,
    /// +0xe0: position at the last hit test.
    pub last_cast: [i32; 3],
    pub blast: Option<Blast>,
}

#[derive(Debug, Clone)]
pub struct Decoy {
    pub pos: [i32; 3],
    pub ticks: u32,
}

fn wrap25(a: i32) -> i32 {
    a.wrapping_mul(0x80) >> 7
}

fn angle_deg(a: i32) -> f32 {
    (a & ANGLE_MASK) as f32 / 65536.0 * 360.0 / 512.0
}

/// `Rot_FromAngles`: engine angles → rotation (nose along +z at 0).
fn rot(angles: [i32; 3]) -> Mat3 {
    Mat3::from_engine_euler_deg(angles.map(angle_deg))
}

fn rotate(angles: [i32; 3], v: [i32; 3]) -> [i32; 3] {
    let r = rot(angles).mul_vec(v.map(|x| x as f32));
    r.map(|x| x.round() as i32)
}

fn add(a: [i32; 3], b: [i32; 3]) -> [i32; 3] {
    std::array::from_fn(|k| a[k].wrapping_add(b[k]))
}

/// Segment `a → b` vs a sphere: the fraction along the segment where it first comes within
/// `r` of `c`, if it does.
fn segment_sphere(a: [i32; 3], b: [i32; 3], c: [i32; 3], r: f64) -> Option<f64> {
    let d: [f64; 3] = std::array::from_fn(|k| b[k] as f64 - a[k] as f64);
    let m: [f64; 3] = std::array::from_fn(|k| a[k] as f64 - c[k] as f64);
    let dd: f64 = d.iter().map(|x| x * x).sum();
    let md: f64 = (0..3).map(|k| m[k] * d[k]).sum();
    let mm: f64 = m.iter().map(|x| x * x).sum::<f64>() - r * r;
    if mm <= 0.0 {
        return Some(0.0);
    }
    if dd == 0.0 {
        return None;
    }
    let disc = md * md - dd * mm;
    if disc < 0.0 {
        return None;
    }
    let t = (-md - disc.sqrt()) / dd;
    (0.0..=1.0).contains(&t).then_some(t)
}

/// What a missile can hit or a blast can reach.
#[derive(Debug, Clone, Copy)]
enum Victim {
    Entity(u32),
    Part(u32, u16),
}

impl Server {
    /// Model radius in 16.16 for a pool model slot (`model +0x30`), from its pak.
    fn model_radius(&mut self, slot: u8) -> i64 {
        if let Some(&r) = self.model_radii.get(&slot) {
            return r;
        }
        let r = self.world.tables.models.get(slot as usize).cloned().flatten().and_then(|(pak, _)| self.map.src.read(&pak).ok()).and_then(|raw| tachyon_core::pak::parse(&raw).ok()).map_or(0.0, |p| p.radius());
        let r = (r as f64 * 65536.0) as i64;
        self.model_radii.insert(slot, r);
        r
    }

    /// Everything a server weapon can touch: centre, model radius (16.16) and its hit-scale
    /// field `+0xb8` (server player DCBs 2.0 `Srv_CreatePlayerDcb`, missiles 2.0
    /// `Dcb_InitMissile`, powerups 2.5 `Powerup_Init`; asteroids and base parts 0).
    /// Gates, beacons, mines and speed rings aren't included: how the host links them for
    /// collision isn't researched yet.
    fn weapon_victims(&mut self) -> Vec<(Victim, [i32; 3], i64, i32)> {
        let ids: Vec<u32> = self.world.order.clone();
        let mut out = vec![];
        for id in ids {
            let Some(e) = self.world.get(id) else { continue };
            let (owner, model, pos, scale) = (e.owner, e.model, e.pos, e.scale);
            match owner {
                Owner::Player(s) => {
                    if self.conn(s).is_some_and(|c| c.dead) || self.world.get(id).is_some_and(|e| e.net_flags & 1 != 0) {
                        continue;
                    }
                    let r = self.model_radius(model);
                    out.push((Victim::Entity(id), pos, r, 0x20000));
                }
                Owner::Missile => {
                    if self.missiles.get(&id).is_some_and(|m| m.blast.is_none()) {
                        let r = self.model_radius(model);
                        out.push((Victim::Entity(id), pos, r, 0x20000));
                    }
                }
                Owner::Static => {
                    if let Some(b) = self.bases.get(&id) {
                        for p in b.parts.iter().filter(|p| p.alive) {
                            out.push((Victim::Part(id, p.index), p.pos, (p.radius as f64 * 65536.0) as i64, 0));
                        }
                        continue;
                    }
                    let hit_scale = match self.pickups.get(&id) {
                        Some(p) if p.taken() => continue,
                        Some(_) => 0x28000,
                        None if self.class_tails.contains_key(&id) => continue,
                        None => 0,
                    };
                    let r = self.model_radius(model) * scale as i64 / FX as i64;
                    out.push((Victim::Entity(id), pos, r, hit_scale));
                }
            }
        }
        out
    }

    /// 0x2004 (`TachSrv_OnFireGuided` → `Proj_SpawnFromClientMissile` → `Proj_CreateMissile`).
    pub(super) fn on_fire_guided(&mut self, slot: u16, b: &[u8]) {
        let f = ev::parse_fire_guided(b);
        let Some(c) = self.conn(slot) else { return };
        let (ship, clock) = (c.ship, c.client_frame);
        let Some(raw) = weapons::missile(f.weapon) else { return };
        let Some(def) = weapons::by_id(f.weapon) else { return };
        // Slot and ammo as for 0x2001: an empty mount or an ammo weapon at 0 drops it.
        let Some(c) = self.conn_mut(slot) else { return };
        let Some(Some((_, _, ammo))) = c.mounts.get_mut(f.slot as usize) else { return };
        if def.ammo != 0 {
            if *ammo == 0 {
                return;
            }
            *ammo -= 1;
        }
        let Some(me) = self.world.get(ship) else { return };
        let team = me.team;
        let target = if f.lock == 0 {
            0
        } else if f.target != 0 {
            if self.world.get(f.target).is_some() { f.target } else { 0 }
        } else {
            me.target
        };
        // age = the latest PLAYERDATA frame − the packet's (unclamped, as in the original).
        let age = clock.wrapping_sub(f.client_frame) as i32;
        self.spawn_missile(raw, Some(ship), team, target, f.pos, f.angles, Some(f.offset), age);
        if self.opts.debug_damage {
            println!("MISSILE {} fired {} ({:#04x}) at {}, age {age}", self.debug_name(Some(ship)), def.name, f.weapon, if target == 0 { "nothing".into() } else { self.debug_name(Some(target)) });
        }
    }

    /// `Proj_CreateMissile` + `Proj_Launch`. Returns the new id.
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_missile(&mut self, raw: &'static MissileRaw, shooter: Option<u32>, team: u8, target: u32, pos: [i32; 3], angles: [i32; 3], offset: Option<[i32; 3]>, age: i32) -> u32 {
        let def = weapons::by_id(raw.id).unwrap();
        let model = self.world.tables.add_model(def.model, "").unwrap_or(0);
        let radius = self.model_radius(model) as i32;
        let angles = angles.map(|a| a & ANGLE_MASK);
        let vel = rotate(angles, [0, 0, raw.speed]);
        // `Proj_Launch`: + R·offset, + R·(0, 0, model radius + speed), + vel × age. The original
        // runs it twice (create callback and `Proj_SpawnFromClientMissile`); DEVIATIONS.md
        // "Missile launch".
        let launches = if self.opts.original_bugs && offset.is_some() { 2 } else { 1 };
        let mut p = pos;
        for _ in 0..launches {
            if let Some(off) = offset {
                p = add(p, rotate(angles, off));
                p = add(p, rotate(angles, [0, 0, radius.wrapping_add(raw.speed)]));
            }
            p = add(p, vel.map(|v| v.wrapping_mul(age)));
        }
        let id = self.world.alloc_id();
        self.world.spawn(Entity {
            id,
            owner: Owner::Missile,
            des: self.weapon_des.clone(),
            desc_slot: 0,
            model,
            pos: p,
            vel,
            angles,
            net_pos: p,
            net_angles: angles,
            state_flags: 6,
            throttle: raw.id,
            team,
            net_flags: 0,
            scale: FX,
            power: Power::default(),
            target,
            hp: 1,
        });
        let serial = self.missile_serial;
        self.missile_serial = self.missile_serial.wrapping_add(1);
        self.missiles.insert(
            id,
            Missile {
                weapon: raw.id,
                raw,
                shooter: shooter.unwrap_or(0),
                target,
                turn: 0,
                counter: LAUNCH_FRAMES[raw.guidance as usize] as i32,
                launching: true,
                serial,
                last_cast: p,
                blast: None,
            },
        );
        id
    }

    /// Per tick: think, hit test, blasts, decoys.
    pub(super) fn missiles_tick(&mut self) {
        self.decoys.retain_mut(|d| {
            d.ticks -= 1;
            d.ticks > 0
        });
        let mut ids: Vec<u32> = self.missiles.keys().copied().collect();
        ids.sort_unstable();
        for id in ids {
            if !self.missiles.contains_key(&id) {
                continue;
            }
            if self.missiles[&id].blast.is_some() {
                self.blast_tick(id);
                continue;
            }
            if !self.missile_think(id) {
                continue;
            }
            if self.missiles.get(&id).is_some_and(|m| m.blast.is_none()) {
                self.missile_cast(id);
            }
        }
    }

    /// Homing (`Proj_SteerToward`): returns the yaw/pitch error before clamping.
    fn steer(&mut self, id: u32, target_pos: [i32; 3]) -> (i32, i32) {
        let m = &self.missiles[&id];
        let (turn, speed) = (m.turn, m.raw.speed);
        let e = self.world.get_mut(id).unwrap();
        let want = combat::dir_angles(e.pos, target_pos);
        let ey = wrap25(want[0].wrapping_sub(e.angles[0]));
        let ep = wrap25(want[1].wrapping_sub(e.angles[1]));
        e.angles[0] = e.angles[0].wrapping_add(ey.clamp(-turn, turn)) & ANGLE_MASK;
        e.angles[1] = e.angles[1].wrapping_add(ep.clamp(-turn, turn)) & ANGLE_MASK;
        e.angles[2] = 0;
        e.vel = rotate(e.angles, [0, 0, speed]);
        (ey, ep)
    }

    fn release_target(&mut self, id: u32) {
        if let Some(m) = self.missiles.get_mut(&id) {
            m.target = 0;
        }
        if let Some(e) = self.world.get_mut(id) {
            e.target = 0;
        }
    }

    /// One frame of the weapon's think. Returns false if the missile is gone.
    fn missile_think(&mut self, id: u32) -> bool {
        let m = self.missiles[&id].clone();
        let original = self.opts.original_bugs;
        if m.launching {
            // `Proj_ThinkMissileLaunch`. A 0-frame launch (Helios) never reaches 0 in the
            // original (DEVIATIONS.md "Helios rocket").
            self.move_missile(id);
            let mm = self.missiles.get_mut(&id).unwrap();
            mm.counter -= 1;
            if mm.counter == 0 || (mm.counter < 0 && !original) {
                mm.counter = mm.raw.life as i32;
                mm.launching = false;
            }
            return true;
        }
        // A target that left the world counts as a lost lock (`Missile_LoseTargetIf`).
        let target_pos = self.world.get(m.target).map(|e| e.pos);
        if m.target != 0 && target_pos.is_none() {
            self.release_target(id);
        }
        match m.weapon {
            W_BLAST => {
                // `Torp_ThinkProximity`: no steering; after 50 frames a proximity fuse.
                self.move_missile(id);
                let mm = self.missiles.get_mut(&id).unwrap();
                mm.counter -= 1;
                let counter = mm.counter;
                let mut near = false;
                if counter < m.raw.life as i32 - 50 && (self.game_frame ^ m.serial) & 7 == 0 {
                    let pos = self.world.get(id).unwrap().pos;
                    near = self.nearest_hostile(pos, m.shooter, PROXIMITY_FUSE, true).is_some();
                }
                if counter == 0 || near {
                    self.start_blast(id);
                }
                true
            }
            W_ADV_BLAST => {
                // `Torp_ThinkAdvBlast`: homes with no 45° cutoff; decoys still apply.
                let mut close = false;
                if let Some(tp) = target_pos.filter(|_| m.target != 0) {
                    self.steer(id, tp);
                    close = distance(self.world.get(id).unwrap().pos, tp) < ADV_BLAST_FUSE;
                    self.check_decoys(id);
                }
                self.move_missile(id);
                let mm = self.missiles.get_mut(&id).unwrap();
                mm.counter -= 1;
                if mm.counter == 0 || close {
                    self.start_blast(id);
                } else {
                    mm.turn = (mm.turn + mm.raw.turn_ramp).min(mm.raw.turn_max);
                }
                true
            }
            _ => {
                // `Proj_ThinkMissile` (+ `Torp_ThinkHunter`).
                if let Some(tp) = target_pos.filter(|_| m.target != 0) {
                    let (ey, ep) = self.steer(id, tp);
                    if let Some(Owner::Player(s)) = self.world.get(m.target).map(|e| e.owner)
                        && let Some(c) = self.conn_mut(s)
                    {
                        c.one_shot |= 1;
                    }
                    if ey.abs() > LOCK_CONE || ep.abs() > LOCK_CONE {
                        self.release_target(id);
                    }
                }
                if self.missiles[&id].target != 0 {
                    self.check_decoys(id);
                }
                self.move_missile(id);
                let mm = self.missiles.get_mut(&id).unwrap();
                mm.counter -= 1;
                if mm.counter == 0 {
                    self.remove_missile(id);
                    return false;
                }
                mm.turn = (mm.turn + mm.raw.turn_ramp).min(mm.raw.turn_max);
                if m.weapon == W_HUNTER && self.missiles[&id].target == 0 && m.shooter != 0 && (self.game_frame ^ m.serial) & 7 == 0 {
                    let pos = self.world.get(id).unwrap().pos;
                    if let Some(t) = self.nearest_hostile(pos, m.shooter, HUNTER_RANGE, false) {
                        self.missiles.get_mut(&id).unwrap().target = t;
                        self.world.get_mut(id).unwrap().target = t;
                    }
                }
                true
            }
        }
    }

    fn move_missile(&mut self, id: u32) {
        if let Some(e) = self.world.get_mut(id) {
            e.pos = add(e.pos, e.vel);
            e.net_pos = e.pos;
        }
    }

    /// `Ent_FindNearestHostile` over the players: the nearest live ship other than the
    /// shooter within `range`; hostile by team in Base Wars unless `any`.
    fn nearest_hostile(&self, pos: [i32; 3], shooter: u32, range: i64, any: bool) -> Option<u32> {
        let team = self.world.get(shooter).map(|e| e.team);
        let bw = self.map.config.game_type != GameType::Deathmatch;
        self.conns
            .iter()
            .flatten()
            .filter(|c| !c.dead && c.ship != shooter)
            .filter_map(|c| self.world.get(c.ship))
            .filter(|e| any || !bw || team.is_none_or(|t| t != e.team))
            .map(|e| (distance(pos, e.pos), e.id))
            .filter(|(d, _)| *d < range)
            .min()
            .map(|(_, id)| id)
    }

    /// `Missile_CheckDecoys`: every 32 frames, a decoy close in front breaks the lock.
    fn check_decoys(&mut self, id: u32) {
        let m = &self.missiles[&id];
        if (m.serial ^ self.game_frame) & 31 != 0 {
            return;
        }
        let e = self.world.get(id).unwrap();
        let fooled = self.decoys.iter().any(|d| {
            if distance(e.pos, d.pos) >= DECOY_RANGE {
                return false;
            }
            let a = combat::dir_angles(e.pos, d.pos);
            wrap25(a[0].wrapping_sub(e.angles[0])).abs() < DECOY_CONE && wrap25(a[1].wrapping_sub(e.angles[1])).abs() < DECOY_CONE
        });
        if fooled {
            if self.opts.debug_damage {
                println!("MISSILE {id:#x} lost its lock to a decoy");
            }
            self.release_target(id);
        }
    }

    /// `Proj_PostThinkOwned`: every 8 frames, a segment from the last test to now against
    /// everything but the shooter and other missiles. Takes the nearest hit along the segment
    /// (the original takes the first in list order).
    fn missile_cast(&mut self, id: u32) {
        let m = self.missiles[&id].clone();
        if (self.game_frame ^ m.serial) & 7 != 0 {
            return;
        }
        let pos = self.world.get(id).unwrap().pos;
        let from = m.last_cast;
        self.missiles.get_mut(&id).unwrap().last_cast = pos;
        let mut best: Option<(f64, Victim, [i32; 3], i64, i64)> = None;
        for (v, c, r, hit_scale) in self.weapon_victims() {
            match v {
                Victim::Entity(e) if e == m.shooter || e == id || self.missiles.contains_key(&e) => continue,
                _ => {}
            }
            // `Coll_SegmentVsList`: the weapon's scale (3.0) for targets with a hit scale, 1.0
            // for those without; the Helios (scale 0) uses the target's own, so it passes
            // through asteroids and bases.
            let scale = match (m.raw.cast_scale, hit_scale) {
                (0, h) => h as i64,
                (_, 0) => FX as i64,
                (s, _) => s as i64,
            };
            let rr = (r * scale / FX as i64) as f64;
            if rr <= 0.0 {
                continue;
            }
            if let Some(t) = segment_sphere(from, pos, c, rr)
                && best.as_ref().is_none_or(|b| t < b.0)
            {
                best = Some((t, v, c, r, scale));
            }
        }
        let Some((t, victim, centre, r, scale)) = best else { return };
        if self.opts.debug_damage {
            let name = match victim {
                Victim::Entity(e) => self.debug_name(Some(e)),
                Victim::Part(root, p) => format!("starbase {root:#x} part {p}"),
            };
            println!("    {id:#x} cast hit {name}: centre {:?}, model radius {:.1} x cast scale {:.1}", centre.map(|v| v >> 16), r as f64 / 65536.0, scale as f64 / 65536.0);
        }
        // Hit point: where the segment meets the sphere, pulled 2 units back toward the start
        // (`Vel_AddKnockback`).
        let mut hit: [i32; 3] = std::array::from_fn(|k| (from[k] as f64 + (pos[k] as f64 - from[k] as f64) * t) as i32);
        let back: [f64; 3] = std::array::from_fn(|k| from[k] as f64 - pos[k] as f64);
        let len = back.iter().map(|x| x * x).sum::<f64>().sqrt();
        if len > 0.0 {
            hit = std::array::from_fn(|k| hit[k] + (back[k] / len * 2.0 * 65536.0) as i32);
        }
        if matches!(m.weapon, W_BLAST | W_ADV_BLAST) {
            // `Proj_OnHitSplash`.
            self.start_blast(id);
            return;
        }
        // `Proj_OnHitDirect`.
        let d = self.missile_damage(id, &m, hit, 0x10000, victim);
        self.apply_weapon_damage(victim, &d, true);
        self.remove_missile(id);
    }

    fn missile_damage(&self, id: u32, m: &Missile, hit: [i32; 3], scale: i32, victim: Victim) -> combat::Damage {
        let def = weapons::by_id(m.weapon).unwrap();
        let hit_id = match victim {
            Victim::Entity(e) => e,
            Victim::Part(root, part) => root.wrapping_add(2 * part as u32),
        };
        combat::Damage {
            weapon: m.weapon,
            amount: ((def.damage as i64 * scale as i64) >> 16) as i32,
            shield_factor: def.shield_factor,
            hull_factor: def.hull_factor,
            hit_pos: hit,
            shooter: (m.shooter != 0 && self.world.get(m.shooter).is_some()).then_some(m.shooter),
            kind: combat::KILL_WEAPON,
            from: self.world.get(id).map(|e| e.pos),
            hit_id,
            except: None,
        }
    }

    /// `impact`: play `Impact_Missile` for non-players (players get theirs from the damage
    /// handler; a blast's sphere plays none).
    fn apply_weapon_damage(&mut self, victim: Victim, d: &combat::Damage, impact: bool) {
        if self.opts.debug_damage {
            let name = match victim {
                Victim::Entity(e) => self.debug_name(Some(e)),
                Victim::Part(r, p) => format!("starbase {r:#x} part {p}"),
            };
            println!(
                "DMG {} hit {name} with {} ({:#04x}) [server]: {}; factors shield {}/256 hull {}/256",
                self.debug_name(d.shooter),
                weapons::by_id(d.weapon).map_or("?", |w| w.name),
                d.weapon,
                d.amount,
                d.shield_factor,
                d.hull_factor
            );
        }
        match victim {
            Victim::Entity(e) => {
                let player = self.world.get(e).is_some_and(|e| matches!(e.owner, Owner::Player(_)));
                self.damage_entity(e, d);
                if impact && !player && !self.missiles.contains_key(&e) {
                    self.play_impact(d.weapon, 2, 0, d.hit_pos, d.from.unwrap_or(d.hit_pos), d.hit_id, None);
                }
            }
            Victim::Part(root, part) => {
                self.damage_base_part(root, part, d);
                if impact {
                    self.play_impact(d.weapon, 2, 0, d.hit_pos, d.from.unwrap_or(d.hit_pos), d.hit_id, None);
                }
            }
        }
    }

    /// `Missile_OnDamage` (hp 1): any damage destroys it: 0x1003 effect 1, then removal.
    pub(super) fn damage_missile(&mut self, id: u32) {
        let Some(m) = self.missiles.get(&id) else { return };
        if m.blast.is_some() {
            return;
        }
        let Some(e) = self.world.get_mut(id) else { return };
        if e.hp <= 0 {
            return;
        }
        e.hp = 0;
        let pos = e.pos;
        self.queue(ev::EV_DESTROYED, ev::destroyed(pos, 0, 1, 0), Delivery { budget: 4, flags: D_RADIUS | D_QUADRANT | D_SYNCED, radius: EFFECT_RADIUS[1] * FX as i64, pos, ..Default::default() });
        if self.opts.debug_damage {
            println!("    missile {id:#x} shot down");
        }
        self.remove_missile(id);
    }

    /// `Proj_Remove`: 0x1014 op 8 to the quadrant (not for a blast, which is already off the
    /// wire), then gone.
    fn remove_missile(&mut self, id: u32) {
        let Some(m) = self.missiles.remove(&id) else { return };
        if m.blast.is_none() {
            self.queue(ev::EV_ENTITY_OP, ev::entity_op(ev::OP_REMOVE, 0, id), Delivery { budget: BUDGET_RELIABLE, flags: D_QUADRANT, ..Default::default() });
        }
        self.world.remove(id);
    }

    /// `Proj_OnHitSplash` + `Blast_Init`: stays put, leaves the snapshots, 0x1008.
    fn start_blast(&mut self, id: u32) {
        let Some(m) = self.missiles.get_mut(&id) else { return };
        m.blast = Some(Blast { scale: 0x20000, intensity: 0xff, growth: m.raw.blast_growth, decay: 0x30000, counter: BLAST_HOLD, holding: true });
        let weapon = m.weapon;
        let Some(e) = self.world.get_mut(id) else { return };
        e.net_flags |= 1;
        e.vel = [0; 3];
        let pos = e.pos;
        self.queue(ev::EV_BLAST, ev::blast(pos, weapon as u16), Delivery { budget: 4, flags: D_RADIUS | D_QUADRANT | D_SYNCED, radius: BLAST_NOTICE_RADIUS * FX as i64, pos, ..Default::default() });
        if self.opts.debug_damage {
            println!("BLAST {id:#x} ({}) at {:?}", weapons::by_id(weapon).map_or("?", |w| w.name), pos.map(|v| v >> 16));
        }
    }

    /// The blast's think (`Blast_HoldThink`, `Blast_Think`) and every 16 frames its damage
    /// (`Blast_PostThink`).
    fn blast_tick(&mut self, id: u32) {
        let m = self.missiles.get_mut(&id).unwrap();
        let b = m.blast.as_mut().unwrap();
        if b.holding {
            b.counter -= 1;
            if b.counter == 0 {
                b.counter = BLAST_RESERVOIR;
                b.holding = false;
            }
        } else {
            b.scale += b.growth;
            b.counter -= b.decay;
            b.decay = (b.decay - 0x80).max(0x10000);
            b.growth = (b.growth - 0x4e2).max(0);
            b.counter = b.counter.max(0);
            b.intensity = (b.counter >> 16).min(0xff);
            if b.intensity == 0 {
                self.missiles.remove(&id);
                self.world.remove(id);
                return;
            }
        }
        let (serial, intensity, scale) = (m.serial, b.intensity, b.scale);
        let m = m.clone();
        if (serial ^ self.game_frame) & 0xf != 0 || intensity <= 15 {
            return;
        }
        let Some(e) = self.world.get(id) else { return };
        let (pos, model) = (e.pos, e.model);
        let radius = self.model_radius(model) * scale as i64 / FX as i64;
        let spare = if m.raw.flags_d8 & 1 != 0 { m.shooter } else { 0 };
        for (v, c, r, _) in self.weapon_victims() {
            if let Victim::Entity(e) = v
                && (e == id || (spare != 0 && e == spare))
            {
                continue;
            }
            if (distance(pos, c) - r).max(0) >= radius {
                continue;
            }
            // Hit point: on the victim's sphere toward the blast (the original casts against its
            // mesh, else uses the centre).
            let dir: [f64; 3] = std::array::from_fn(|k| pos[k] as f64 - c[k] as f64);
            let len = dir.iter().map(|x| x * x).sum::<f64>().sqrt();
            let hit = if len > r as f64 { std::array::from_fn(|k| c[k] + (dir[k] / len * r as f64) as i32) } else { c };
            let d = self.missile_damage(id, &m, hit, intensity << 8, v);
            self.apply_weapon_damage(v, &d, false);
        }
    }

    /// 0x2020 (`TachSrv_OnDecoyDropped`): a 250-frame server decoy and the 0x1104 relay.
    pub(super) fn on_decoy(&mut self, slot: u16, b: &[u8]) {
        let pos = ev::parse_vec3(b);
        self.decoys.push(Decoy { pos, ticks: DECOY_TICKS });
        self.queue(
            ev::EV_DECOY,
            ev::decoy(pos),
            Delivery { budget: BUDGET_RELIABLE, flags: D_RADIUS | D_EXCEPT | D_QUADRANT | D_SYNCED, radius: DECOY_RELAY_RADIUS * FX as i64, pos, player: Some(slot), team: 0 },
        );
    }

    /// Round reset / map change: every missile, blast and decoy goes (the clients' quadrants
    /// are cleared too).
    pub(super) fn clear_missiles(&mut self) {
        let ids: Vec<u32> = self.missiles.keys().copied().collect();
        for id in ids {
            self.world.remove(id);
        }
        self.missiles.clear();
        self.decoys.clear();
    }

    /// A missile's class info: `{id, 0, targetId}`.
    pub(super) fn missile_class_info(&self, id: u32) -> Option<Vec<u8>> {
        let m = self.missiles.get(&id)?;
        Some(ev::class_info_missile(id, 0, m.target))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `dir_angles(from, to)` fed through the launch rotation points the nose at `to`.
    #[test]
    fn steering_angles_point_at_the_target() {
        for t in [[0, 0, 100], [100, 0, 0], [-100, 0, 0], [0, 0, -100], [0, 100, 0], [0, -100, 10], [30, 40, -50], [-70, -20, 15]] {
            let to = t.map(|v: i32| v << 16);
            let a = combat::dir_angles([0; 3], to);
            let n = rotate(a, [0, 0, 1 << 16]).map(|v| v as f64);
            let len = (t.iter().map(|&v| (v * v) as f64).sum::<f64>()).sqrt();
            let dot: f64 = (0..3).map(|k| n[k] / 65536.0 * t[k] as f64 / len).sum();
            assert!(dot > 0.999, "{t:?}: angles {a:?} nose {n:?} dot {dot}");
        }
    }

    #[test]
    fn segment_sphere_hits() {
        let u = |v: [i32; 3]| v.map(|x| x << 16);
        assert!(segment_sphere(u([0, 0, 0]), u([0, 0, 100]), u([0, 5, 50]), 10.0 * 65536.0).is_some());
        assert!(segment_sphere(u([0, 0, 0]), u([0, 0, 100]), u([0, 15, 50]), 10.0 * 65536.0).is_none());
        assert!(segment_sphere(u([0, 0, 0]), u([0, 0, 100]), u([0, 0, 150]), 10.0 * 65536.0).is_none());
        let t = segment_sphere(u([0, 0, 0]), u([0, 0, 100]), u([0, 0, 50]), 10.0 * 65536.0).unwrap();
        assert!((t - 0.4).abs() < 1e-6);
    }
}
