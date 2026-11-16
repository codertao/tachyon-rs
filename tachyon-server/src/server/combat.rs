//! Player-vs-player combat as the stock host does it: the fire relay, client hit reports,
//! shield/hull damage, death, respawn, shield regen, power transfers and collisions.
//! References: `decomp/notes/shields.md`, `damage.md`, `collision.md`, `multiplayer.md`
//! "Weapons and projectiles" / "Death and respawn".

use super::*;
use tachyon_core::net::events::Hit;

/// `g_PlayerDamageScale 0x5a663c`: every share is ×166/256 before the shields see it.
const PLAYER_DAMAGE_SCALE: i64 = 166;
/// Weapon id the collision path stages (`TachSrv_DamageEntityById`); no impact effect.
pub const WEAPON_COLLISION: u8 = 0x26;
/// `[SPEED] MAX_SPEED` is stored ×163.84 (16.16 units per frame), the unit of the reported
/// impact speed.
const MAX_SPEED_SCALE: f32 = 163.84;

/// Kill kinds (`DAT_00565410`): 0 weapon, 1 rammed, 2 crashed.
pub const KILL_WEAPON: u8 = 0;
pub const KILL_RAMMED: u8 = 1;
pub const KILL_CRASHED: u8 = 2;

/// Kill texts (`ingame.bin` 0x70..0x77, picked at random; 0x78 rammed). `%s` = killer, victim.
const KILL_TEXTS: [&str; 8] = [
    "%s vaporized %s",
    "%s made %s eat plasma",
    "%s showed %s how it's really done",
    "%s blasted %s into another galaxy",
    "%s showed no mercy on %s",
    "%s turned %s's ship into space dust",
    "%s sent %s home in a space pod",
    "%s added a few new holes to %s's starship",
];
const RAM_TEXT: &str = "%s used his ship as a ramming device on %s";
/// `Kill_SendMsgCrashed` formats this literal (`0x5493f4`), not ingame.bin 0x79.
const CRASH_TEXT: &str = "%s CRASHED";
/// Kill texts are colour 12 (`DAT_005a632c`).
const COLOR_KILL: u16 = 12;

/// What `Weapon_StageDamage` stages for a target's damage handler.
#[derive(Clone, Copy, Debug)]
pub struct Damage {
    pub weapon: u8,
    pub amount: i32,
    /// Weapon factors out of 256 (`def+0x18/+0x1c/+0x20`).
    pub shield_factor: i32,
    pub hull_factor: i32,
    pub hit_pos: [i32; 3],
    /// The shooter's ship entity, if a player gets the credit.
    pub shooter: Option<u32>,
    pub kind: u8,
}

/// The engine's `Math_Atan2(x, z)` (`Math_Atan2Core 0x4e0740`): 16.16 angle, 512 units per
/// turn, with 0 toward −z, 128 toward −x, 256 toward +z and 384 toward +x. So the direction
/// of angle a is −(sin a, cos a).
pub fn engine_atan2(x: f64, z: f64) -> i32 {
    if x == 0.0 && z == 0.0 {
        return 0;
    }
    let t = (-x).atan2(-z);
    ((t / std::f64::consts::TAU * 512.0 * 65536.0).round() as i64 as i32) & ANGLE_MASK
}

/// `Vec_DirAngles(a, b)`: yaw and pitch of a − b, roll 0.
pub fn dir_angles(a: [i32; 3], b: [i32; 3]) -> [i32; 3] {
    let d: [f64; 3] = std::array::from_fn(|k| (a[k] as f64 - b[k] as f64) / 2.0);
    let xz = (d[0] * d[0] + d[2] * d[2]).sqrt();
    [engine_atan2(d[0], d[2]), engine_atan2(d[1], -xz), 0]
}

/// `Shield_SplitFrontRear`: fold the bearing (0..512 units) to 0..256; rear share = dmg·b/256.
/// Returns (front, rear).
pub fn split_front_rear(bearing: i32, dmg: i32) -> (i32, i32) {
    let mut b = ((bearing as u32) >> 16) & 0x1ff;
    if b > 0x100 {
        b = 0x200 - b;
    }
    let f = b as f32 / 256.0;
    let rear = (dmg as f32 * f).round() as i32;
    let front = ((1.0 - f) * dmg as f32).round() as i32;
    (front, rear)
}

/// `Shield_ApplyDamageFR 0x4191f0`. Returns 1 if only shields took it, 2 if the hull did.
pub fn apply_damage_fr(p: &mut Power, front: i32, rear: i32, shf: i32, hf: i32) -> u8 {
    let threshold = p.shield_max / 100;
    let mut r = 1;
    let mul = |a: i32, f: i32| ((a as i64 * f as i64) as u32 >> 8) as i32;
    p.shield_front = (p.shield_front - mul(front, shf)).max(0);
    if p.shield_front < threshold {
        p.hull -= mul(front, hf);
        r = 2;
    }
    p.shield_rear = (p.shield_rear - mul(rear, shf)).max(0);
    if p.shield_rear < threshold {
        p.hull -= mul(rear, hf);
        r = 2;
    }
    p.hull = p.hull.max(0);
    r
}

/// `Shield_ApplyDamage 0x419170` with factors (0x100, 0x100): the front pool at half rate, then
/// the hull once that pool is at or below 1%.
pub fn apply_collision_damage(p: &mut Power, dmg: i32) {
    let threshold = p.shield_max / 100;
    if p.shield_front != 0 {
        p.shield_front = (p.shield_front - ((dmg as i64 * 0x100) as u32 >> 9) as i32).max(0);
    }
    if p.shield_front <= threshold {
        p.hull = (p.hull - ((dmg as i64 * 0x100) as u32 >> 8) as i32).max(0);
    }
}

/// `TachSrv_OnCollision`'s damage factor: min(v, 1.25·max)/max × 0.95, clamped to [0.2, 1.25].
pub fn collision_factor(speed: i32, max_speed_raw: f32) -> f32 {
    if max_speed_raw <= 0.0 {
        return 0.2;
    }
    let v = (speed as f32).min(max_speed_raw * 1.25);
    (v / max_speed_raw * 0.95).clamp(0.2, 1.25)
}

/// Engine angle (16.16, 512 units per turn) in degrees.
fn deg(a: i32) -> f64 {
    (a & ANGLE_MASK) as f64 / FX as f64 * 360.0 / 512.0
}

fn change(what: &str, a: i32, b: i32) -> String {
    if a == b { format!("{what} {a}") } else { format!("{what} {a}->{b} ({:+})", b - a) }
}

/// Before → after for the pools that matter.
fn power_change(a: &Power, b: &Power) -> String {
    format!("{}  {}  {}", change("front", a.shield_front, b.shield_front), change("rear", a.shield_rear, b.shield_rear), change("hull", a.hull, b.hull))
}

impl Server {
    /// Who an entity is, for debug lines.
    fn debug_name(&self, id: Option<u32>) -> String {
        let Some(id) = id else { return "nobody".into() };
        match self.world.get(id).map(|e| e.owner) {
            Some(Owner::Player(s)) => self.conn(s).map_or(format!("{id:#x}"), |c| format!("{} (slot {s})", c.name)),
            Some(Owner::Static) => format!("{} {id:#x}", self.world.get(id).map_or("?", |e| e.des.desc_type.as_str()).to_ascii_lowercase()),
            None => format!("{id:#x}"),
        }
    }

    fn ship_of(&self, slot: u16) -> u32 {
        self.conn(slot).map_or(0, |c| c.ship)
    }

    fn slot_of_ship(&self, id: u32) -> Option<u16> {
        match self.world.get(id)?.owner {
            Owner::Player(s) => Some(s),
            Owner::Static => None,
        }
    }

    /// 0x2001 → 0x1002 (`TachSrv_OnFire`): an empty mount or an ammo weapon at 0 drops the
    /// shot silently; otherwise relay to everyone within 4000 units except the shooter.
    pub(super) fn on_fire(&mut self, slot: u16, b: &[u8]) {
        let f = ev::parse_fire(b);
        let ship = self.ship_of(slot);
        let Some(c) = self.conn_mut(slot) else { return };
        let Some(Some((w, _max, ammo))) = c.mounts.get_mut(f.slot as usize) else { return };
        if weapons::by_id(*w).is_some_and(|d| d.ammo != 0) {
            if *ammo == 0 {
                return;
            }
            *ammo -= 1;
        }
        let body = ev::fired(&f, self.game_frame as u16, ship);
        self.queue(
            ev::EV_FIRED,
            body,
            Delivery { budget: 1, flags: D_RADIUS | D_EXCEPT | D_QUADRANT | D_SYNCED, radius: 4000 * FX as i64, pos: f.pos, player: Some(slot), team: 0 },
        );
    }

    /// 0x2003 / 0x2027 (`TachSrv_OnHitReport`): trust the client, recompute the damage from the
    /// weapon def and the reported scale, hand it to the target, then play the impact.
    pub(super) fn on_hit(&mut self, slot: u16, h: Hit) {
        let Some(c) = self.conn(slot) else { return };
        let (ship, dead) = (c.ship, c.dead);
        if dead && !self.opts.original_bugs {
            return;
        }
        let Some(target) = self.world.get(h.target) else { return };
        let target_pos = target.pos;
        let Some(def) = weapons::by_id(h.weapon) else { return };
        if h.part.is_some() {
            // Capital ship / station parts (`CapShip_DamagePartById`) arrive with the part
            // entities (starbases and M4 turrets). Until then only the impact plays.
        }
        let amount = ((def.damage as i64 * h.scale as i64) >> 16) as i32;
        if self.opts.debug_damage {
            println!(
                "DMG {} hit {} with {} ({:#04x}){}: base {} x scale {:.3} = {amount}; factors shield {}/256 hull {}/256",
                self.debug_name(Some(ship)),
                self.debug_name(Some(h.target)),
                def.name,
                h.weapon,
                h.part.map_or(String::new(), |p| format!(" on part {p}")),
                def.damage,
                h.scale as f64 / 65536.0,
                def.shield_factor,
                def.hull_factor
            );
        }
        let d = Damage {
            weapon: h.weapon,
            amount,
            shield_factor: def.shield_factor,
            hull_factor: def.hull_factor,
            hit_pos: h.pos,
            shooter: Some(ship),
            kind: KILL_WEAPON,
        };
        // The impact excludes the shooter, unless a surviving player victim took the hit, in
        // which case `TachSrv_DamagePlayer` re-staged the exclusion to the victim.
        let except = if h.part.is_none() { self.damage_entity(h.target, &d).unwrap_or(slot) } else { slot };
        self.play_impact(h.weapon, h.kind, h.pos, target_pos, except);
    }

    /// The target's `+0x80` damage handler. Returns the connection a following impact should
    /// exclude, when the handler changed it.
    pub fn damage_entity(&mut self, id: u32, d: &Damage) -> Option<u16> {
        let e = self.world.get(id)?;
        match e.owner {
            Owner::Player(_) => self.damage_player(id, d),
            Owner::Static if e.des.desc_type == "ASTEROID" => {
                self.damage_asteroid(id, d);
                None
            }
            Owner::Static => None,
        }
    }

    /// `TachSrv_DamagePlayer`.
    fn damage_player(&mut self, id: u32, d: &Damage) -> Option<u16> {
        let vslot = self.slot_of_ship(id)?;
        if self.conn(vslot)?.dead {
            return None;
        }
        let original = self.opts.original_bugs;
        let debug = self.opts.debug_damage;
        let e = self.world.get_mut(id)?;
        if e.power.hull <= 0 {
            return None;
        }
        let world_bearing = engine_atan2((d.hit_pos[0] as f64 - e.pos[0] as f64) / 2.0, (d.hit_pos[2] as f64 - e.pos[2] as f64) / 2.0);
        // DEVIATIONS.md "Shield facing": measure the bearing from the nose. A ship at yaw y
        // points along (sin y, cos y), whose engine bearing is y + 256 units.
        let nose_bearing = world_bearing.wrapping_sub(e.angles[0].wrapping_add(256 * FX)) & ANGLE_MASK;
        let bearing = if original { world_bearing } else { nose_bearing };
        let (front, rear) = split_front_rear(bearing, d.amount);
        let scale = |v: i32| ((PLAYER_DAMAGE_SCALE * v as i64) as u32 >> 8) as i32;
        let before = e.power;
        let result = apply_damage_fr(&mut e.power, scale(front), scale(rear), d.shield_factor, d.hull_factor);
        let after = e.power;
        let killed = e.power.hull < 1;
        let target_pos = e.pos;
        if debug {
            let rel: Vec<String> = (0..3).map(|k| format!("{:.0}", ((d.hit_pos[k] as f64 - target_pos[k] as f64) / FX as f64).round() + 0.0)).collect();
            let (sf, sr) = (scale(front), scale(rear));
            let shield = |v: i32| ((v as i64 * d.shield_factor as i64) as u32 >> 8) as i32;
            let hull = |v: i32| ((v as i64 * d.hull_factor as i64) as u32 >> 8) as i32;
            let threshold = before.shield_max / 100;
            let mut b = ((bearing as u32) >> 16) & 0x1ff;
            if b > 0x100 {
                b = 0x200 - b;
            }
            println!(
                "    hit point ({}) from the ship; world bearing {:.1}, yaw {:.1}, off the nose {:.1} deg (0 ahead, 180 behind); split uses {} -> rear share {}/256",
                rel.join(", "),
                deg(world_bearing),
                deg(e.angles[0]),
                deg(nose_bearing),
                if original { "world bearing (--original-bugs)" } else { "nose bearing" },
                b
            );
            println!(
                "    {} -> front {front} / rear {rear}; x{PLAYER_DAMAGE_SCALE}/256 -> front {sf} / rear {sr}; shield drain front {} rear {}; hull share front {} rear {} (taken while that pool < {threshold})",
                d.amount,
                shield(sf),
                shield(sr),
                hull(sf),
                hull(sr)
            );
            println!(
                "    {}  [{}]  regen +{} each per 16 ticks",
                power_change(&before, &after),
                if killed { "KILLED" } else if result == 2 { "hull hit" } else { "shields held" },
                (256 * (after.shield_rate >> 11)) >> 8
            );
        }
        let mut except = None;
        if killed {
            self.ship_killed(id, d.shooter, d.kind);
        } else {
            self.play_impact(d.weapon, result, d.hit_pos, target_pos, vslot);
            except = Some(vslot);
        }
        self.queue(ev::EV_YOU_WERE_HIT, vec![0; 0x14], Delivery::only(vslot));
        except
    }

    /// `Asteroid_OnDamage`: hitpoints take the hull share; at 0 the asteroid blows up (0x1003
    /// effect 5) and is despawned.
    fn damage_asteroid(&mut self, id: u32, d: &Damage) {
        let Some(e) = self.world.get_mut(id) else { return };
        if e.hp <= 0 {
            return;
        }
        let hp = e.hp;
        e.hp -= ((d.hull_factor as i64 * d.amount as i64) as u32 >> 8) as i32;
        if self.opts.debug_damage {
            let e = &self.world.entities[&id];
            println!("    asteroid {id:#x}: {} x hull {}/256 -> {}", d.amount, d.hull_factor, change("hp", hp, e.hp));
        }
        let Some(e) = self.world.get_mut(id) else { return };
        if e.hp >= 1 {
            return;
        }
        let (pos, scale) = (e.pos, e.scale);
        // The size byte is the model radius in units (`model+0x30`); without pak geometry
        // here, the placement scale stands in for it.
        let size = ((scale >> 16).clamp(1, 255)) as u8;
        self.queue(ev::EV_DESTROYED, ev::destroyed(pos, 0, 5, size), Delivery { budget: 4, flags: D_RADIUS | D_QUADRANT | D_SYNCED, radius: EFFECT_RADIUS[5] * FX as i64, pos, ..Default::default() });
        self.queue(ev::EV_DESPAWN, ev::despawn(id), Delivery { budget: BUDGET_RELIABLE, flags: D_QUADRANT, ..Default::default() });
        self.remove_static(id);
    }

    /// The weapon's impact callback on the server (`Impact_Bolt`): kind 1 (shield) sends the
    /// spark 0x1004, anything else a small explosion 0x1003 effect 0. Collisions have none.
    fn play_impact(&mut self, weapon: u8, kind: u8, hit_pos: [i32; 3], target_pos: [i32; 3], except: u16) {
        if weapon == WEAPON_COLLISION {
            return;
        }
        if kind == 1 {
            // The original's hit id, projectile id and direction come from stale staging
            // globals (0 for hit reports); the direction here points from the hit to the
            // target's centre.
            let dir = dir_angles(target_pos, hit_pos);
            self.queue(
                ev::EV_IMPACT,
                ev::impact(hit_pos, 0, 0, dir),
                Delivery { budget: 4, flags: D_RADIUS | D_EXCEPT | D_QUADRANT | D_SYNCED, radius: 375 * FX as i64, pos: hit_pos, player: Some(except), team: 0 },
            );
        } else {
            self.queue(
                ev::EV_DESTROYED,
                ev::destroyed(hit_pos, 0, 0, 0),
                Delivery { budget: 4, flags: D_RADIUS | D_EXCEPT | D_QUADRANT | D_SYNCED, radius: EFFECT_RADIUS[0] * FX as i64, pos: hit_pos, player: Some(except), team: 0 },
            );
        }
    }

    /// `TachSrv_OnShipKilled`.
    pub fn ship_killed(&mut self, id: u32, shooter: Option<u32>, kind: u8) {
        let Some(vslot) = self.slot_of_ship(id) else { return };
        self.queue(ev::EV_YOU_DIED, vec![0, 0], Delivery::only(vslot));
        let pos = {
            let Some(e) = self.world.get_mut(id) else { return };
            e.net_flags = 1;
            e.target = 0;
            e.pos
        };
        let victim_name = {
            let c = self.conn_mut(vslot).unwrap();
            c.deaths = c.deaths.saturating_add(1);
            c.dead = true;
            c.name.clone()
        };
        self.queue(
            ev::EV_DESTROYED,
            ev::destroyed(pos, 0, 3, 0),
            Delivery { budget: 4, flags: D_RADIUS | D_EXCEPT | D_QUADRANT | D_SYNCED, radius: EFFECT_RADIUS[3] * FX as i64, pos, player: Some(vslot), team: 0 },
        );
        self.queue(ev::EV_ENTITY_OP, ev::entity_op(8, 0, id), Delivery { budget: BUDGET_RELIABLE, flags: D_QUADRANT | D_EXCEPT, player: Some(vslot), ..Default::default() });

        let killer = shooter.and_then(|s| self.slot_of_ship(s));
        let mut killer_kills = None;
        match killer {
            Some(kslot) => {
                let killer_name = {
                    let c = self.conn_mut(kslot).unwrap();
                    if kslot != vslot {
                        c.kills = c.kills.saturating_add(1);
                    }
                    killer_kills = Some(c.kills);
                    c.name.clone()
                };
                if kslot != vslot {
                    let text = match kind {
                        KILL_WEAPON => Some(KILL_TEXTS[(self.rand() & 7) as usize]),
                        KILL_RAMMED => Some(RAM_TEXT),
                        _ => None,
                    };
                    if let Some(t) = text {
                        let line = t.replacen("%s", &killer_name, 1).replacen("%s", &victim_name, 1);
                        self.text_to(Delivery { budget: BUDGET_RELIABLE, flags: D_QUADRANT, ..Default::default() }, COLOR_KILL, &line);
                        println!("{line}");
                    }
                }
            }
            None if shooter.is_none() && kind == KILL_CRASHED => {
                let line = CRASH_TEXT.replacen("%s", &victim_name, 1);
                self.text_to(Delivery { budget: BUDGET_RELIABLE, flags: D_QUADRANT, ..Default::default() }, COLOR_KILL, &line);
                println!("{line}");
            }
            None => println!("{victim_name} was destroyed"),
        }
        // The mode's kill hook (`DM_OnPlayerKilled`).
        if let Some(k) = killer_kills {
            self.rules_on_kill(k);
        }
        self.scoreboard();
    }

    /// 0x2011 → `Srv_RespawnPlayer(conn, 1)`: new spawn point, alive, 0x1012 to the player,
    /// `Ship_InitPower`.
    pub(super) fn on_respawn(&mut self, slot: u16) {
        // DEVIATIONS.md "Respawn requests": the original honours 0x2011 at any time, so a
        // modified client can teleport to a fresh spawn with full shields, or leave the freeze.
        if !self.opts.original_bugs && (self.freeze_ticks != 0 || self.conn(slot).is_none_or(|c| !c.dead)) {
            return;
        }
        let (pos, angles) = self.dm_spawn_pos();
        let ship = {
            let Some(c) = self.conn_mut(slot) else { return };
            c.state = 0;
            c.dead = false;
            c.camera = pos;
            c.ship
        };
        self.place_ship(ship, pos, angles);
        self.queue(ev::EV_TELEPORT, ev::teleport(pos, angles), Delivery::only(slot));
        self.init_ship_power(slot);
    }

    pub(super) fn place_ship(&mut self, ship: u32, pos: [i32; 3], angles: [i32; 3]) {
        if let Some(e) = self.world.get_mut(ship) {
            e.pos = pos;
            e.net_pos = pos;
            e.angles = angles;
            e.net_angles = angles;
            e.vel = [0; 3];
        }
    }

    /// `Ship_InitPower` (mode hook +0x60): `Power_Init` fills every pool, the ship becomes
    /// visible again, and every mount is refilled to max (`Ship_CopyWeaponLoadout`).
    pub(super) fn init_ship_power(&mut self, slot: u16) {
        let Some(c) = self.conn_mut(slot) else { return };
        for (_, max, ammo) in c.mounts.iter_mut().flatten() {
            *ammo = *max;
        }
        let ship = c.ship;
        if let Some(e) = self.world.get_mut(ship) {
            let rate = e.power.shield_rate;
            e.power = Power::for_des(&e.des);
            e.power.shield_rate = rate;
            e.net_flags = 0;
        }
    }

    /// `TachSrv_PlayerThink`'s regen: every 16 frames, `Power_Regen(flags 0x12)` on both shield
    /// pools with the client-sent rate (PLAYERDATA +0x2c, no clamp), level 0 (x256/256).
    pub(super) fn regen(&mut self) {
        let phase = self.game_frame;
        for c in self.conns.iter().flatten() {
            if (phase ^ c.slot as u32) & 15 != 0 {
                continue;
            }
            let Some(e) = self.world.entities.get_mut(&c.ship) else { continue };
            let p = &mut e.power;
            let add = (256 * (p.shield_rate >> 11)) >> 8;
            p.shield_front = (p.shield_front + add).min(p.shield_max);
            p.shield_rear = (p.shield_rear + add).min(p.shield_max);
        }
    }

    /// 0x2005 (`TachSrv_OnPowerOp`). Op 2 falls through to op 3 with the client's amount, as
    /// in the original's switch.
    pub(super) fn on_power_op(&mut self, slot: u16, b: &[u8]) {
        let (op, amount) = ev::parse_power(b);
        let ship = self.ship_of(slot);
        let debug = self.opts.debug_damage.then(|| self.debug_name(Some(ship)));
        let Some(e) = self.world.get_mut(ship) else { return };
        let before = e.power;
        let p = &mut e.power;
        let max = p.shield_max;
        // `Power_TransferPool`: move min(src/8, room) into dst.
        let transfer = |src: &mut i32, dst: &mut i32| {
            if *dst < max && *src != 0 {
                let n = (*src >> 3).min(max - *dst);
                *dst += n;
                *src -= n;
            }
        };
        let srv = |p: &mut Power, op: u16, amount: i32| {
            if amount == 0 {
                return;
            }
            match op {
                3 | 4 => {
                    if p.shield_front != max || p.shield_rear != max {
                        p.shield_front = (p.shield_front + (amount >> 2)).min(max);
                        p.shield_rear = (p.shield_rear + (amount >> 2)).min(max);
                    }
                }
                5 | 6 => {
                    if p.shield_front != 0 || p.shield_rear != 0 {
                        p.shield_front = (p.shield_front - amount).max(0);
                        p.shield_rear = (p.shield_rear - amount).max(0);
                    }
                }
                _ => {}
            }
        };
        match op {
            1 => transfer(&mut p.shield_front, &mut p.shield_rear),
            2 => {
                transfer(&mut p.shield_rear, &mut p.shield_front);
                srv(p, 3, amount as i32);
            }
            3..=6 => srv(p, op, amount as i32),
            _ => {}
        }
        if let Some(name) = debug {
            const OPS: [&str; 7] = ["?", "front->rear", "rear->front", "recharge", "recharge", "drain", "drain"];
            println!("POWER {name} op {op} ({}) amount {amount}: {}", OPS.get(op as usize).unwrap_or(&"?"), power_change(&before, &e.power));
        }
    }

    /// 0x2009 (`TachSrv_OnCollision`). The client reports its speed and what it hit; the
    /// server trusts both.
    pub(super) fn on_collision(&mut self, slot: u16, b: &[u8]) {
        let (speed, other) = ev::parse_collision(b);
        let Some(c) = self.conn(slot) else { return };
        if c.dead {
            return;
        }
        let ship = c.ship;
        let original = self.opts.original_bugs;
        let Some(e) = self.world.get_mut(ship) else { return };
        if e.power.hull <= 0 {
            return;
        }
        let max_raw = e.des.ship.as_ref().map_or(0.0, |s| s.max_speed * MAX_SPEED_SCALE);
        let f = collision_factor(speed, max_raw);
        let dmg = (e.power.shield_max as f32 * f).round() as i32;
        let before = e.power;
        apply_collision_damage(&mut e.power, dmg);
        let after = e.power;
        let my_pos = e.pos;
        if self.opts.debug_damage {
            println!(
                "DMG {} collided with {}: speed {speed} (max {max_raw:.0}) -> factor {f:.3} x shield max {} = {dmg}; front pool at half rate, hull once front <= {}",
                self.debug_name(Some(ship)),
                if other == 0 { "scenery".into() } else { self.debug_name(Some(other)) },
                before.shield_max,
                before.shield_max / 100
            );
            println!("    {}", power_change(&before, &after));
            if other != 0 && other != ship {
                println!("    {} takes {dmg} (factors 256/256){}", self.debug_name(Some(other)), if original { " with no ram credit (--original-bugs)" } else { "" });
            }
        }
        if other != 0 && other != ship {
            // The original stages shooter = this ship and kind 1, but `TachSrv_DamageEntityById`
            // then clears the shooter and leaves the previous hit's factors and position staged,
            // so rams are never credited. DEVIATIONS.md "Ramming".
            let d = if original {
                Damage { weapon: WEAPON_COLLISION, amount: dmg, shield_factor: 0x100, hull_factor: 0x100, hit_pos: my_pos, shooter: None, kind: KILL_RAMMED }
            } else {
                Damage { weapon: WEAPON_COLLISION, amount: dmg, shield_factor: 0x100, hull_factor: 0x100, hit_pos: my_pos, shooter: Some(ship), kind: KILL_RAMMED }
            };
            self.damage_entity(other, &d);
        }
        let still_alive = self.conn(slot).is_some_and(|c| !c.dead);
        if still_alive && self.world.get(ship).is_some_and(|e| e.power.hull < 1) {
            self.ship_killed(ship, None, KILL_CRASHED);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atan2_matches_the_engine_quadrants() {
        let u = |a: i32| a >> 16;
        assert_eq!(u(engine_atan2(0.0, -1.0)), 0);
        assert_eq!(u(engine_atan2(-1.0, 0.0)), 128);
        assert_eq!(u(engine_atan2(0.0, 1.0)), 256);
        assert_eq!(u(engine_atan2(1.0, 0.0)), 384);
        // `DM_PickSpawnPos` faces the origin with `Vec_DirAngles(pos, origin)`: from +z that is
        // yaw 256, whose nose (sin, cos)(256 units) = −z points at the origin.
        assert_eq!(u(dir_angles([0, 0, 100], [0, 0, 0])[0]), 256);
        // `face_toward` (std atan2 of to − from) agrees with it.
        assert_eq!(face_toward([0, 0, 100], [0; 3])[0], dir_angles([0, 0, 100], [0; 3])[0]);
    }

    #[test]
    fn front_rear_split() {
        assert_eq!(split_front_rear(0, 100), (100, 0));
        assert_eq!(split_front_rear(256 << 16, 100), (0, 100));
        assert_eq!(split_front_rear(128 << 16, 100), (50, 50));
        assert_eq!(split_front_rear(384 << 16, 100), (50, 50));
    }

    #[test]
    fn shields_then_hull() {
        let mut p = Power { shield_front: 100, shield_rear: 100, shield_max: 100, hull: 50, hull_max: 50, ..Default::default() };
        assert_eq!(apply_damage_fr(&mut p, 80, 0, 256, 256), 1);
        assert_eq!((p.shield_front, p.hull), (20, 50));
        assert_eq!(apply_damage_fr(&mut p, 40, 0, 256, 128), 2);
        assert_eq!((p.shield_front, p.hull), (0, 30));
        apply_collision_damage(&mut p, 1000);
        assert_eq!(p.hull, 0);
    }

    #[test]
    fn collision_factor_clamps() {
        assert_eq!(collision_factor(0, 100.0), 0.2);
        assert!((collision_factor(100, 100.0) - 0.95).abs() < 1e-6);
        assert!((collision_factor(1000, 100.0) - 1.1875).abs() < 1e-6);
    }
}
