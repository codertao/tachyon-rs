//! App event bodies, server → client (`0x10xx/0x11xx`) and the client → server ones the server
//! reads. Layouts: `decomp/notes/multiplayer.md` "App event catalogue".

use crate::net::netsock::{cstr, fixed_str};
use crate::net::proto::{i32_at, u16_at, u32_at};
use crate::net::startup::{roster_entry, RosterEntry};

pub const EV_TEXT: u16 = 0x1001;
pub const EV_FIRED: u16 = 0x1002;
pub const EV_DESTROYED: u16 = 0x1003;
pub const EV_IMPACT: u16 = 0x1004;
pub const EV_YOU_WERE_HIT: u16 = 0x1005;
pub const EV_YOU_DIED: u16 = 0x1006;
pub const EV_ENTER_QUADRANT: u16 = 0x1007;
pub const EV_DESPAWN: u16 = 0x1011;
pub const EV_TELEPORT: u16 = 0x1012;
pub const EV_ENTITY_OP: u16 = 0x1014;
pub const EV_ROSTER_CLEARED: u16 = 0x1029;
pub const EV_SESSION_BUMP: u16 = 0x1033;
pub const EV_TEAM_SHIP: u16 = 0x1107;
pub const EV_PORTRAIT: u16 = 0x1108;
pub const EV_ROSTER: u16 = 0x1110;
pub const EV_STARTUP_DONE: u16 = 0x1111;
pub const EV_SCOREBOARD: u16 = 0x1112;
pub const EV_AMMO: u16 = 0x1124;

/// 0x1124 `{u16 mount, u16 count}` (`SrvSend_Ammo`; client `CliOn_Ammo` sets that mount's count).
pub fn ammo(mount: u16, count: u16) -> Vec<u8> {
    let mut b = mount.to_le_bytes().to_vec();
    b.extend_from_slice(&count.to_le_bytes());
    b
}
pub const EV_CLASS_INFO: u16 = 0x1100;
pub const EV_ROUND_FREEZE: u16 = 0x1119;
pub const EV_ROUND_RESUME: u16 = 0x1120;

pub const CL_FIRE: u16 = 0x2001;
pub const CL_CLASS_INFO: u16 = 0x2002;
pub const CL_HIT: u16 = 0x2003;
pub const CL_FIRE_GUIDED: u16 = 0x2004;
pub const CL_POWER: u16 = 0x2005;
pub const CL_ENTER_WORLD: u16 = 0x2007;
pub const CL_DATA_ACK: u16 = 0x2008;
pub const CL_COLLISION: u16 = 0x2009;
pub const CL_READY: u16 = 0x2010;
pub const CL_RESPAWN: u16 = 0x2011;
pub const CL_QUADRANT_ENTERED: u16 = 0x2012;
pub const CL_CHAT: u16 = 0x2022;
pub const CL_PART_HIT: u16 = 0x2027;
pub const CL_LOADOUT: u16 = 0x2028;

/// Text colours (plain palette indices; `Gfx_DefineColor` stores `i` at `0x5a6320 + i`).
pub const COLOR_CHAT: u16 = 0;
pub const COLOR_INFO: u16 = 1;
pub const COLOR_ALERT: u16 = 2;
pub const COLOR_JOIN: u16 = 4;
pub const COLOR_GROUP: u16 = 9;
pub const COLOR_PRIVATE: u16 = 11;
pub const COLOR_TEAM: u16 = 12;

/// 0x1001 `{u16 kind, u16 color, char text[]}`; size `(strlen + 7) & !1` (`SrvSend_TextMsg`).
/// kind 0 = HUD line.
pub fn text(kind: u16, color: u16, s: &str) -> Vec<u8> {
    let s: String = s.chars().filter(|c| c.is_ascii()).take(254).collect();
    let mut b = kind.to_le_bytes().to_vec();
    b.extend_from_slice(&color.to_le_bytes());
    b.extend_from_slice(s.as_bytes());
    b.push(0);
    b.resize((s.len() + 7) & !1, 0);
    b
}

pub fn parse_text(b: &[u8]) -> (u16, u16, String) {
    (u16_at(b, 0), u16_at(b, 2), b.get(4..).map(cstr).unwrap_or_default())
}

/// 0x2022 `{u16 mode, u16 target, char text[]}`: 0 all, 1 one player, 4 team, 8 group.
pub fn parse_chat(b: &[u8]) -> (u16, u16, String) {
    parse_text(b)
}

pub fn chat(mode: u16, target: u16, s: &str) -> Vec<u8> {
    text(mode, target, s)
}

/// 0x1107 `{char desName[14], u16 descSlot}`.
pub fn team_ship(des_name: &str, slot: u16) -> Vec<u8> {
    let mut b = fixed_str(des_name, 14);
    b.extend_from_slice(&slot.to_le_bytes());
    b
}

/// 0x1108 `{char name[14], u16 portraitSlot}`.
pub fn portrait(name: &str, slot: u16) -> Vec<u8> {
    team_ship(name, slot)
}

pub fn roster(e: &RosterEntry) -> Vec<u8> {
    roster_entry(e)
}

pub fn roster_cleared(slot: u16) -> Vec<u8> {
    slot.to_le_bytes().to_vec()
}

/// 0x1112 `u16 n (≤ 8), {u16 slot, u16 kills, u16 deaths}[n]`, best first.
pub fn scoreboard(rows: &[(u16, u16, u16)]) -> Vec<u8> {
    let n = rows.len().min(8);
    let mut b = (n as u16).to_le_bytes().to_vec();
    for (s, k, d) in &rows[..n] {
        for v in [s, k, d] {
            b.extend_from_slice(&v.to_le_bytes());
        }
    }
    b
}

pub fn startup_done() -> Vec<u8> {
    vec![0, 0]
}

pub fn session_bump(session: u32) -> Vec<u8> {
    session.to_le_bytes().to_vec()
}

/// 0x2008: kind 1 = static page ack `{u16 1, u32 pageId, u16 count}`, kind 2 = ready.
pub fn parse_data_ack(b: &[u8]) -> (u16, u32, u16) {
    (u16_at(b, 0), u32_at(b, 2), u16_at(b, 6))
}


fn put_vec(b: &mut Vec<u8>, v: [i32; 3]) {
    for x in v {
        b.extend_from_slice(&x.to_le_bytes());
    }
}

fn vec_at(b: &[u8], o: usize) -> [i32; 3] {
    [i32_at(b, o), i32_at(b, o + 4), i32_at(b, o + 8)]
}

/// 0x2001 fire bolt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fire {
    pub pos: [i32; 3],
    /// High 16 bits of the 16.16 angles.
    pub angles: [u16; 3],
    pub client_frame: u16,
    pub slot: u8,
    pub weapon: u8,
    pub volume: u8,
}

pub fn parse_fire(b: &[u8]) -> Fire {
    Fire {
        pos: vec_at(b, 0),
        angles: [u16_at(b, 0xc), u16_at(b, 0xe), u16_at(b, 0x10)],
        client_frame: u16_at(b, 0x12),
        slot: b.get(0x14).copied().unwrap_or(0),
        weapon: b.get(0x16).copied().unwrap_or(0),
        volume: b.get(0x17).copied().unwrap_or(0),
    }
}

pub fn fire(f: &Fire) -> Vec<u8> {
    let mut b = Vec::with_capacity(0x1c);
    put_vec(&mut b, f.pos);
    for a in f.angles {
        b.extend_from_slice(&a.to_le_bytes());
    }
    b.extend_from_slice(&f.client_frame.to_le_bytes());
    b.extend_from_slice(&[f.slot, 0, f.weapon, f.volume]);
    b.resize(0x1c, 0);
    b
}

/// 0x1002 `{vec3 pos, u16 angles[3], u16 serverFrame @0x12, u8 weapon @0x16, u8 volume @0x17,
/// u32 shooterId @0x18}` (`TachSrv_OnFire` copies the 0x2001 fields).
pub fn fired(f: &Fire, server_frame: u16, shooter: u32) -> Vec<u8> {
    let mut b = Vec::with_capacity(0x1c);
    put_vec(&mut b, f.pos);
    for a in f.angles {
        b.extend_from_slice(&a.to_le_bytes());
    }
    b.extend_from_slice(&server_frame.to_le_bytes());
    b.extend_from_slice(&[0, 0, f.weapon, f.volume]);
    b.extend_from_slice(&shooter.to_le_bytes());
    b
}

/// 0x2003 hit report, and 0x2027 (capital ship part) with the part id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hit {
    pub pos: [i32; 3],
    pub target: u32,
    pub weapon: u8,
    /// 1 shield hit, 2 hull hit (picks the impact effect only).
    pub kind: u8,
    /// Damage scale, 16.16 (the wire carries it `>> 2`).
    pub scale: u32,
    pub part: Option<u16>,
}

pub fn parse_hit(b: &[u8]) -> Hit {
    Hit {
        pos: vec_at(b, 0),
        target: u32_at(b, 0xc),
        weapon: b.get(0x10).copied().unwrap_or(0),
        kind: b.get(0x11).copied().unwrap_or(0),
        scale: (u16_at(b, 0x12) as u32) << 2,
        part: None,
    }
}

/// 0x2027 `{vec3 hitPos, u32 targetId, u16 partId @0x10, u8 weapon @0x12, u8 kind @0x13,
/// u16 scale>>2 @0x14}` (`TachSrv_OnPartHit`).
pub fn parse_part_hit(b: &[u8]) -> Hit {
    Hit {
        pos: vec_at(b, 0),
        target: u32_at(b, 0xc),
        part: Some(u16_at(b, 0x10)),
        weapon: b.get(0x12).copied().unwrap_or(0),
        kind: b.get(0x13).copied().unwrap_or(0),
        scale: (u16_at(b, 0x14) as u32) << 2,
    }
}

pub fn hit(h: &Hit) -> Vec<u8> {
    let mut b = Vec::with_capacity(0x16);
    put_vec(&mut b, h.pos);
    b.extend_from_slice(&h.target.to_le_bytes());
    match h.part {
        None => {
            b.extend_from_slice(&[h.weapon, h.kind]);
            b.extend_from_slice(&((h.scale >> 2) as u16).to_le_bytes());
        }
        Some(p) => {
            b.extend_from_slice(&p.to_le_bytes());
            b.extend_from_slice(&[h.weapon, h.kind]);
            b.extend_from_slice(&((h.scale >> 2) as u16).to_le_bytes());
        }
    }
    b
}

/// 0x1003 `{vec3 pos, u32 deadId @0xc, u32 @0x10, u8 effect @0x14, u8 size @0x15}`.
pub fn destroyed(pos: [i32; 3], dead: u32, effect: u8, size: u8) -> Vec<u8> {
    let mut b = Vec::with_capacity(0x16);
    put_vec(&mut b, pos);
    b.extend_from_slice(&dead.to_le_bytes());
    b.extend_from_slice(&0u32.to_le_bytes());
    b.extend_from_slice(&[effect, size]);
    b
}

/// 0x1004 `{vec3 pos, u32 hitId @0xc, u32 projectileId @0x10, u16 @0x14, u16 dir[3] @0x16}`.
pub fn impact(pos: [i32; 3], hit_id: u32, projectile: u32, dir: [i32; 3]) -> Vec<u8> {
    let mut b = Vec::with_capacity(0x1c);
    put_vec(&mut b, pos);
    b.extend_from_slice(&hit_id.to_le_bytes());
    b.extend_from_slice(&projectile.to_le_bytes());
    b.extend_from_slice(&0u16.to_le_bytes());
    for a in dir {
        b.extend_from_slice(&((a as u32 >> 16) as u16).to_le_bytes());
    }
    b
}

/// 0x1014 `{u16 op, u16 value, u32 entityId}`; op 8 = remove.
pub fn entity_op(op: u16, value: u16, id: u32) -> Vec<u8> {
    let mut b = op.to_le_bytes().to_vec();
    b.extend_from_slice(&value.to_le_bytes());
    b.extend_from_slice(&id.to_le_bytes());
    b
}

/// 0x1011 `{u32 entityId}`.
pub fn despawn(id: u32) -> Vec<u8> {
    id.to_le_bytes().to_vec()
}

/// 0x1012 `{vec3 pos, vec3 angles}` (16.16).
pub fn teleport(pos: [i32; 3], angles: [i32; 3]) -> Vec<u8> {
    let mut b = Vec::with_capacity(0x18);
    put_vec(&mut b, pos);
    put_vec(&mut b, angles);
    b
}

/// 0x1007 `{vec3 pos, u16 angles[3] @0xc, u16 quadrantId @0x12, u8 @0x14, u8 @0x15,
/// char sectorBdf[14] @0x16, char name[] @0x24}` (`SrvSend_EnterQuadrant`).
pub fn enter_quadrant(pos: [i32; 3], angles: [i32; 3], quadrant: u16, b14: u8, b15: u8, bdf: &str) -> Vec<u8> {
    let mut b = Vec::with_capacity(0x34);
    put_vec(&mut b, pos);
    for a in angles {
        b.extend_from_slice(&((a as u32 >> 16) as u16).to_le_bytes());
    }
    b.extend_from_slice(&quadrant.to_le_bytes());
    b.extend_from_slice(&[b14, b15]);
    b.extend_from_slice(&fixed_str(bdf, 14));
    b.resize(0x34, 0);
    b
}

/// 0x1100 for a player ship (`SrvSend_EntityClassInfo` + the SHIP class writer `LAB_00463690`):
/// `{u32 id, u16 descSlot, u8 type 1, u8 flags 0, u16 0xffff, u16 slot, u16 0xffff, u8 0, u8 0}`.
/// The client (`ShipReplica_Init`) keeps type at `+0x76`, the slot at `+0xf2` (roster slot for
/// the pilot's name) and the 0xffff fields at `+0x114` / `+0x11c`.
pub fn class_info_player(id: u32, desc_slot: u16, slot: u16) -> Vec<u8> {
    let mut b = id.to_le_bytes().to_vec();
    b.extend_from_slice(&desc_slot.to_le_bytes());
    b.extend_from_slice(&[1, 0]);
    b.extend_from_slice(&0xffffu16.to_le_bytes());
    b.extend_from_slice(&slot.to_le_bytes());
    b.extend_from_slice(&0xffffu16.to_le_bytes());
    b.extend_from_slice(&[0, 0]);
    b
}

/// 0x2005 `{u16 op, u16 amount}`.
pub fn parse_power(b: &[u8]) -> (u16, u16) {
    (u16_at(b, 0), u16_at(b, 2))
}

/// 0x2009 `{u32 impactSpeed, u32 otherId}`.
pub fn parse_collision(b: &[u8]) -> (i32, u32) {
    (i32_at(b, 0), u32_at(b, 4))
}

pub fn collision(speed: i32, other: u32) -> Vec<u8> {
    let mut b = speed.to_le_bytes().to_vec();
    b.extend_from_slice(&other.to_le_bytes());
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_sizes() {
        assert_eq!(text(0, 0, "abcd").len(), 10);
        assert_eq!(text(0, 0, "abcde").len(), 12);
        assert_eq!(parse_text(&text(0, 11, "hi there")), (0, 11, "hi there".into()));
        assert_eq!(scoreboard(&[(1, 2, 3)]), [1, 0, 1, 0, 2, 0, 3, 0]);
    }

    #[test]
    fn combat_bodies() {
        let f = Fire { pos: [1, -2, 3], angles: [4, 5, 6], client_frame: 7, slot: 1, weapon: 9, volume: 0x80 };
        assert_eq!(fire(&f).len(), 0x1c);
        assert_eq!(parse_fire(&fire(&f)), f);
        let r = fired(&f, 77, 0x123);
        assert_eq!((r.len(), u16_at(&r, 0x12), r[0x16], r[0x17], u32_at(&r, 0x18)), (0x1c, 77, 9, 0x80, 0x123));
        let h = Hit { pos: [1, 2, 3], target: 0x101, weapon: 3, kind: 1, scale: 0x10000, part: None };
        assert_eq!(hit(&h).len(), 0x14);
        assert_eq!(parse_hit(&hit(&h)), h);
        let p = Hit { part: Some(4), ..h };
        assert_eq!(hit(&p).len(), 0x16);
        assert_eq!(parse_part_hit(&hit(&p)), p);
        assert_eq!(destroyed([0; 3], 5, 3, 0).len(), 0x16);
        assert_eq!(impact([0; 3], 0, 0, [0; 3]).len(), 0x1c);
        assert_eq!(enter_quadrant([0; 3], [0; 3], 1, 1, 1, "x.bdf").len(), 0x34);
        assert_eq!(parse_collision(&collision(-5, 9)), (-5, 9));
    }
}
