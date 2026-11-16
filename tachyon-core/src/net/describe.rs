//! Human-readable interpretations of engine sub-messages and app events, for packet logs.
//! Layouts follow `decomp/notes/multiplayer.md` (engine messages, the event catalogue and
//! "Server-side startup details"). Decoding is best effort: a short body gives zeros, never a
//! panic, and anything not understood is named (if the catalogue knows it) and left to the hex
//! dump.

use crate::net::packet::{EV_STARTUP_ACK, MSG_GAMEDATA, MSG_KICK, MSG_PLAYERDATA, MSG_PLAYERINFO, MSG_STARTUPDATA};
use crate::net::proto::{
    f16_decode, i32_at, name_table, parse_gamedata, parse_player_data, race, startup_records, str_at, u16_at, u32_at,
    UNIT,
};

/// Catalogue name of an engine message or app event.
pub fn name(kind: u16) -> Option<&'static str> {
    Some(match kind {
        MSG_GAMEDATA => "GAMEDATA",
        MSG_PLAYERDATA => "PLAYERDATA",
        MSG_STARTUPDATA => "STARTUPDATA",
        MSG_PLAYERINFO => "PLAYERINFO",
        MSG_KICK => "KICK",
        EV_STARTUP_ACK => "StartupAck",
        // Server -> client.
        0x1001 => "TextMessage",
        0x1002 => "ProjectileFired",
        0x1003 => "EntityDestroyed",
        0x1004 => "ProjectileImpact",
        0x1005 => "YouTookDamage",
        0x1006 => "YouWereDestroyed",
        0x1007 => "EnterQuadrant",
        0x1008 => "MinePlaced",
        0x1009 => "GateState",
        0x1010 => "PlayScene",
        0x1011 => "Despawn",
        0x1012 => "Teleport",
        0x1013 => "PlaySound",
        0x1014 => "EntityStateOp",
        0x1015 => "NavTarget",
        0x1016 => "AddOrder",
        0x1017 => "OrderUpdate",
        0x1020 => "EntityFiredAt",
        0x1028 => "NavTarget2",
        0x1029 => "RosterCleared",
        0x1031 => "MissionEnd",
        0x1032 => "RoundResult",
        0x1033 => "SessionBump",
        0x1034 => "UiSound",
        0x1100 => "EntityClassInfo",
        0x1101 => "DropPredicted",
        0x1102 => "DeployedRemoved",
        0x1103 => "EntityFired",
        0x1104 => "DecoyDropped",
        0x1105 => "LockThreat",
        0x1106 => "SpawnEntity",
        0x1107 => "TeamShip",
        0x1108 => "Portrait",
        0x1110 => "RosterEntry",
        0x1111 => "StartupComplete",
        0x1112 => "Scoreboard",
        0x1113 => "EntityLabel",
        0x1114 => "Allegiance",
        0x1115 => "VoiceWav",
        0x1116 => "LaunchMission",
        0x1117 => "Resupply",
        0x1118 => "BeamImpact",
        0x1119 => "RoundOver",
        0x1120 => "NewRound",
        0x1121 => "CampaignCheck",
        0x1122 => "RankProgress",
        0x1123 => "FactionProgress",
        0x1124 => "Ammo",
        0x1125 => "AttachSubpart",
        0x1126 => "DestroySubpart",
        0x1127 => "RankChange",
        // Client -> server.
        0x2001 => "FireBolt",
        0x2002 => "ClassInfoRequest",
        0x2003 => "HitReport",
        0x2004 => "FireMissile",
        0x2005 => "PowerOp",
        0x2006 => "UseGate",
        0x2007 => "EnterWorld",
        0x2008 => "DataAck",
        0x2009 => "Collision",
        0x2010 => "ClientReady",
        0x2011 => "RespawnRequest",
        0x2012 => "QuadrantEntered",
        0x2013 => "Docked",
        0x2014 => "Interact",
        0x2015 => "Undocking",
        0x2016 => "DockTrigger",
        0x2017 => "SapperAttached",
        0x2018 => "MineHit",
        0x2019 => "MineFired",
        0x2020 => "DecoyDropped",
        0x2021 => "PostEvent",
        0x2022 => "Chat",
        0x2023 => "Tractor",
        0x2024 => "WingFormOnMe",
        0x2025 => "WingCommand",
        0x2026 => "RulesEvent",
        0x2027 => "PartHitReport",
        0x2028 => "Loadout",
        _ => return None,
    })
}

/// A 16.16 position as whole units.
fn pos(p: [i32; 3]) -> String {
    format!("({:.0}, {:.0}, {:.0})", p[0] as f64 / UNIT, p[1] as f64 / UNIT, p[2] as f64 / UNIT)
}
/// A 16.16 velocity (units per 50 Hz tick).
fn vel(p: [i32; 3]) -> String {
    format!("({:.1}, {:.1}, {:.1})", p[0] as f64 / UNIT, p[1] as f64 / UNIT, p[2] as f64 / UNIT)
}
fn velf(p: [f64; 3]) -> String {
    format!("({:.1}, {:.1}, {:.1})", p[0], p[1], p[2])
}
fn posf(p: [f64; 3]) -> String {
    format!("({:.0}, {:.0}, {:.0})", p[0], p[1], p[2])
}
fn vec3(b: &[u8], o: usize) -> [i32; 3] {
    [0, 4, 8].map(|k| i32_at(b, o + k))
}
/// 16.16 engine angles (512 units per turn) as degrees.
fn deg(a: [i32; 3]) -> String {
    let d = |v: i32| (v as f64 / UNIT) * 360.0 / 512.0;
    format!("[{:.0}°, {:.0}°, {:.0}°]", d(a[0]), d(a[1]), d(a[2]))
}
/// u16 wire angles (16.16 >> 9: 65536 per turn) as degrees.
fn deg16(b: &[u8], o: usize) -> String {
    let d = |k: usize| u16_at(b, o + 2 * k) as f64 * 360.0 / 65536.0;
    format!("[{:.0}°, {:.0}°, {:.0}°]", d(0), d(1), d(2))
}
fn id(v: u32) -> String {
    format!("{v:#x}")
}
fn speed(v: [i32; 3]) -> f64 {
    let f = v.map(|x| x as f64 / UNIT);
    (f[0] * f[0] + f[1] * f[1] + f[2] * f[2]).sqrt()
}

/// Interpretation lines for one sub-message (`body` excludes the sub-header and any tag).
/// `from_client` picks the direction where a type means different things each way.
pub fn describe(kind: u16, body: &[u8], from_client: bool) -> Vec<String> {
    let b = body;
    let one = |s: String| vec![s];
    match kind {
        MSG_PLAYERDATA => {
            let head = format!("player {}, client frame {}, session {}", id(u32_at(b, 0)), u32_at(b, 4), u32_at(b, 8) as i32);
            match b.get(12..).and_then(parse_player_data) {
                None => one(format!("{head}; short body")),
                Some(p) if !p.has_ship => one(format!("{head}; no ship (camera at {})", pos(p.camera))),
                Some(p) => {
                    let thr = if p.throttle < 0 { "afterburner".to_string() } else { format!("throttle {}/1024", p.throttle) };
                    let mut l = vec![head, format!(
                        "ship at {} vel {} ({:.1} u/tick) angles {} {thr}",
                        pos(p.pos), vel(p.vel), speed(p.vel), deg(p.angles)
                    )];
                    let mut extra = vec![];
                    if p.target != 0 {
                        extra.push(format!("target {} sub {:?}", id(p.target), p.subtarget));
                    }
                    if p.shield_rate != 0 {
                        extra.push(format!("shield rate {}", p.shield_rate));
                    }
                    if p.interact[0] != 0 {
                        extra.push(format!("interact {} action {}", id(p.interact[0]), p.interact[1]));
                    }
                    if !extra.is_empty() {
                        l.push(extra.join(", "));
                    }
                    l
                }
            }
        }
        MSG_STARTUPDATA => {
            let mut l = vec![format!("stage {}, session {}, player {}", u32_at(b, 0), u32_at(b, 4) as i32, id(u32_at(b, 8)))];
            if let Some(blob) = b.get(12..) {
                for (k, rec) in startup_records(blob) {
                    l.push(startup_record(k, rec));
                }
            }
            l
        }
        MSG_PLAYERINFO => one(format!("net tick {}, session {}, game frame {}", u32_at(b, 0), u32_at(b, 4) as i32, u32_at(b, 8))),
        MSG_KICK => one(format!(
            "kicked, reason {}{}",
            b.first().unwrap_or(&0),
            match b.first() {
                Some(4) => " (wrong game id)",
                Some(5) => " (protocol magic mismatch)",
                _ => "",
            }
        )),
        MSG_GAMEDATA => gamedata(b),
        EV_STARTUP_ACK => one(format!("ack stage {}", u32_at(b, 2))),
        _ => event(kind, b, from_client).map(|s| vec![s]).unwrap_or_default(),
    }
}

fn startup_record(k: u16, rec: &[u8]) -> String {
    match k {
        3 => {
            let t = name_table(rec, true);
            format!("  [3] model table, {} entries: {}", t.len(), list(t.iter().map(|(s, n)| format!("{s}={n}"))))
        }
        2 => {
            let t = name_table(rec, false);
            format!("  [2] descriptor table, {} entries: {}", t.len(), list(t.iter().map(|(s, n)| format!("{s}={n}"))))
        }
        1 => format!(
            "  [1] rules: {} / {} (region {}), you are entity {}, quadrant {}, slot {} of {}, game mode {} ({})",
            str_at(rec, 4),
            str_at(rec, 0x12),
            u16_at(rec, 0x2a),
            id(u32_at(rec, 0x22)),
            u16_at(rec, 0x2c),
            u16_at(rec, 0x2e),
            u16_at(rec, 0x30),
            u16_at(rec, 0x32),
            match u16_at(rec, 0x32) {
                0 => "base wars",
                1 => "deathmatch",
                _ => "?",
            }
        ),
        4 => {
            let mounts: Vec<String> = (0..8)
                .filter_map(|i| {
                    let w = u16_at(rec, 0x30 + 4 * i);
                    (w != 0).then(|| format!("{i}:w{}x{}", w - 1, u16_at(rec, 0x32 + 4 * i)))
                })
                .collect();
            format!(
                "  [4] your ship: {} team {} systems {:#x} at {} angles {} mounts {}",
                str_at(rec, 4),
                race(u16_at(rec, 0x12) as u32),
                u32_at(rec, 0x14),
                pos(vec3(rec, 0x18)),
                deg(vec3(rec, 0x24)),
                if mounts.is_empty() { "none".into() } else { mounts.join(" ") }
            )
        }
        5 => {
            let mut o = 6;
            let mut names = vec![];
            for _ in 0..u16_at(rec, 4) {
                if o + 8 >= rec.len() {
                    break;
                }
                let name = str_at(rec, o + 8);
                names.push(format!("{}={name} (team {})", u16_at(rec, o), rec.get(o + 6).copied().unwrap_or(0)));
                o += (name.len() + 11) & !1;
            }
            format!("  [5] roster: {}", list(names.into_iter()))
        }
        6 => {
            let n = (u32_at(rec, 4) as usize).min(rec.len().saturating_sub(8) / 16);
            let p = (0..n).map(|i| format!("{}={}", u16_at(rec, 8 + i * 16 + 14), str_at(rec, 8 + i * 16)));
            format!("  [6] portraits: {}", list(p))
        }
        7 => {
            let ip = u32_at(rec, 4);
            if ip == 0 {
                "  [7] session: no voice server".into()
            } else {
                format!("  [7] session: voice server {}:{}", std::net::Ipv4Addr::from(ip.to_le_bytes()), u16_at(rec, 8))
            }
        }
        k => format!("  [{k}] {} bytes", rec.len()),
    }
}

fn list(it: impl Iterator<Item = String>) -> String {
    let v: Vec<String> = it.collect();
    if v.is_empty() {
        "none".into()
    } else if v.len() > 12 {
        format!("{} … (+{})", v[..12].join(", "), v.len() - 12)
    } else {
        v.join(", ")
    }
}

fn gamedata(b: &[u8]) -> Vec<String> {
    let f32_at = |o: usize| f32::from_le_bytes(u32_at(b, o).to_le_bytes());
    let meter = |v: u8| if v == 0xff { "-".to_string() } else { v.to_string() };
    let mut l = vec![format!(
        "you: shields {}/{} hull {}; target meters {}/{}/{} speed {:.1}; flags {:#x}",
        u16_at(b, 8),
        u16_at(b, 0xa),
        u16_at(b, 0xc),
        meter(*b.get(4).unwrap_or(&0xff)),
        meter(*b.get(5).unwrap_or(&0xff)),
        meter(*b.get(6).unwrap_or(&0xff)),
        f32_at(0),
        u32_at(b, 0x14)
    )];
    let Some(snap) = parse_gamedata(b) else {
        l.push("snapshot: unparseable".into());
        return l;
    };
    let page = if snap.page_list != 0 { format!(", static page {} ({} objects)", snap.page_list, snap.page_count) } else { String::new() };
    l.push(format!("{} records{page}", snap.records.len()));
    for r in &snap.records {
        let mut tags = vec![];
        if r.flags & 0x10 != 0 {
            tags.push("far");
        }
        if r.flags & 0x08 != 0 {
            tags.push("ext");
        }
        let thr = match r.throttle {
            255 => "afterburner".to_string(),
            t => format!("thr {t}"),
        };
        let desc = r.desc.map_or(String::new(), |d| format!(" desc {d}"));
        l.push(format!(
            "  entity {} model {} team {} flags {:#04x}{} at {} vel {} yaw/pitch/roll {:?} {thr}{desc}",
            id(r.id),
            r.model,
            r.team,
            r.flags,
            if tags.is_empty() { String::new() } else { format!(" ({})", tags.join(",")) },
            posf(r.pos),
            velf(r.vel),
            r.angles
        ));
    }
    l
}

fn event(kind: u16, b: &[u8], from_client: bool) -> Option<String> {
    let _ = from_client; // Server and client event ranges don't overlap; kept for future use.
    let s = match kind {
        0x1001 => format!("kind {} colour {}: {:?}", u16_at(b, 0), u16_at(b, 2), str_at(b, 4)),
        0x1002 => format!(
            "shot weapon {} by {} from {} angles {} server frame {}",
            b.get(0x16).unwrap_or(&0),
            id(u32_at(b, 0x18)),
            pos(vec3(b, 0)),
            deg16(b, 0xc),
            u16_at(b, 0x12)
        ),
        0x1003 => format!(
            "{} destroyed at {}, related {}, effect {}",
            id(u32_at(b, 0xc)),
            pos(vec3(b, 0)),
            id(u32_at(b, 0x10)),
            b.get(0x14).unwrap_or(&0)
        ),
        0x1004 => format!("projectile {} hit {} at {}", id(u32_at(b, 0x10)), id(u32_at(b, 0xc)), pos(vec3(b, 0))),
        0x1007 => format!(
            "enter quadrant {} {:?} at {} angles {}",
            u16_at(b, 0x12),
            str_at(b, 0x16),
            pos(vec3(b, 0)),
            deg16(b, 0xc)
        ),
        0x1008 => format!("mine weapon {} at {}", u16_at(b, 0xc), pos(vec3(b, 0))),
        0x1009 => format!("gate {} state {} param {}", id(u32_at(b, 0)), u16_at(b, 4), u16_at(b, 6)),
        0x1010 | 0x1115 => format!("{:?}", str_at(b, 0)),
        0x1011 | 0x1101 | 0x1102 => format!("entity {}", id(u32_at(b, 0))),
        0x1012 => format!("to {} orient {}", pos(vec3(b, 0)), deg(vec3(b, 0xc))),
        0x1013 => format!("mode {} {:?} alt {:?} param {}", u16_at(b, 0), str_at(b, 2), str_at(b, 0x10), u32_at(b, 0x30)),
        0x1014 => format!("op {:#x} value {} on {}", u16_at(b, 0), u16_at(b, 2), id(u32_at(b, 4))),
        0x1015 | 0x1028 => format!("target {} route {} {:?}", id(u32_at(b, 0)), u32_at(b, 4), str_at(b, 8)),
        0x1016 => format!("order {} kind {} param {}: {:?}", u16_at(b, 0), u16_at(b, 2), u32_at(b, 4), str_at(b, 8)),
        0x1017 => format!("order {} op {}", u16_at(b, 0), u16_at(b, 2)),
        0x1020 => format!(
            "weapon {} from {} at {} target {}",
            b.first().unwrap_or(&0),
            id(u32_at(b, 0xe)),
            pos([2, 4, 6].map(|o| f16_decode(u16_at(b, o)))),
            id(u32_at(b, 0x12))
        ),
        0x1029 => format!("slot {}", u16_at(b, 0)),
        0x1032 => format!("{} vs {}: {}", str_at(b, 0), str_at(b, 16), u16_at(b, 0x20)),
        0x1033 => format!("session now {}", u32_at(b, 0)),
        0x1034 => format!("bank {} id {} param {}", b.first().unwrap_or(&0), b.get(1).unwrap_or(&0), b.get(2).unwrap_or(&0)),
        0x1100 => format!("entity {} class {}", id(u32_at(b, 0)), u16_at(b, 4)),
        0x1103 => format!("{} fired weapon {} at {}", id(u32_at(b, 0)), u16_at(b, 8), id(u32_at(b, 4))),
        0x1104 => format!("at {}", pos(vec3(b, 0))),
        0x1105 => format!("entity {} code {:#x} ({})", id(u32_at(b, 0)), u16_at(b, 4), if u16_at(b, 4) == 0x13 { "add" } else { "remove" }),
        0x1106 => {
            let origin = vec3(b, 0x1c);
            match crate::net::proto::parse_record(b, origin) {
                Some((r, _)) => format!(
                    "entity {} model {} team {} flags {:#04x} at {}{}",
                    id(r.id),
                    r.model,
                    r.team,
                    r.flags,
                    posf(r.pos),
                    r.desc.map_or(String::new(), |d| format!(" desc {d}"))
                ),
                None => "short record".into(),
            }
        }
        0x1107 => format!("{:?} desc slot {}", str_at(b, 0), u16_at(b, 14)),
        0x1108 => format!("{:?} portrait slot {}", str_at(b, 0), u16_at(b, 14)),
        0x1110 => format!(
            "slot {} {:?} portrait {} team {}",
            u16_at(b, 0),
            str_at(b, 8),
            u16_at(b, 2) as i16,
            b.get(6).map_or("?".to_string(), |&t| if t == 0xff { "none".into() } else { race(t as u32) })
        ),
        0x1112 => {
            let n = u16_at(b, 0) as usize;
            let rows = (0..n.min(8)).map(|i| {
                let o = 2 + i * 6;
                format!("slot {} {}/{}", u16_at(b, o), u16_at(b, o + 2), u16_at(b, o + 4))
            });
            format!("kills/deaths: {}", list(rows))
        }
        0x1113 => format!("entity {} {:?}", id(u32_at(b, 0)), str_at(b, 4)),
        0x1114 => format!("entity {} team {}", id(u32_at(b, 0)), race(u16_at(b, 4) as u32)),
        0x1117 => format!("code {} amount {}", u16_at(b, 0), u32_at(b, 2)),
        0x1118 => format!("{} hit {} subpart {}", id(u32_at(b, 4)), id(u32_at(b, 0)), u16_at(b, 0xa)),
        0x1122 => format!("points {} next rank at {}", u32_at(b, 0), u32_at(b, 4)),
        0x1124 => format!("slot {} ammo {}", u16_at(b, 0), u16_at(b, 2)),
        0x1125 => format!("entity {} subpart {} data {:#x}", id(u32_at(b, 0)), u16_at(b, 8), u32_at(b, 4)),
        0x1126 => format!("entity {} subpart {}", id(u32_at(b, 0)), u16_at(b, 4)),
        0x1127 => format!("slot {} rank {} faction {}", u16_at(b, 0), b.get(2).unwrap_or(&0), b.get(3).unwrap_or(&0)),
        0x2001 => format!(
            "slot {} weapon {} from {} angles {} client frame {} volume {}",
            b.get(0x14).unwrap_or(&0),
            b.get(0x16).unwrap_or(&0),
            pos(vec3(b, 0)),
            deg16(b, 0xc),
            u16_at(b, 0x12),
            b.get(0x17).unwrap_or(&0)
        ),
        0x2002 | 0x2006 | 0x2013 | 0x2014 | 0x2015 | 0x2023 => format!("entity {}", id(u32_at(b, 0))),
        0x2003 | 0x2027 => format!(
            "weapon {} kind {} hit {} at {} scale {}{}",
            b.get(0x10).unwrap_or(&0),
            b.get(0x11).unwrap_or(&0),
            id(u32_at(b, 0xc)),
            pos(vec3(b, 0)),
            u16_at(b, 0x12) as u32 * 4,
            if kind == 0x2027 { format!(" part {}", u16_at(b, 0x14)) } else { String::new() }
        ),
        0x2004 => format!(
            "slot {} weapon {} at {} target {} lock {} client frame {}",
            b.get(0x23).unwrap_or(&0),
            b.get(0x20).unwrap_or(&0),
            pos(vec3(b, 0)),
            id(u32_at(b, 0x1c)),
            b.get(0x22).unwrap_or(&0),
            u32_at(b, 0x18)
        ),
        0x2005 => format!(
            "{} amount {}",
            match u16_at(b, 0) {
                1 => "shields front->rear",
                2 => "shields rear->front",
                3 => "laser->shields",
                4 => "burner->shields",
                5 => "shields->laser",
                6 => "shields->burner",
                _ => "op ?",
            },
            u16_at(b, 2)
        ),
        0x2008 => match u16_at(b, 0) {
            1 => format!("static page {} ({} objects) received", u32_at(b, 2), u16_at(b, 6)),
            2 => "ready for snapshots".into(),
            k => format!("kind {k}"),
        },
        0x2009 => format!("impact speed {} with {}", u32_at(b, 0), id(u32_at(b, 4))),
        0x2017 => format!("weapon {} on {} part {} local id {}", b.first().unwrap_or(&0), id(u32_at(b, 0x12)), u16_at(b, 0x16), id(u32_at(b, 0xe))),
        0x2022 => format!(
            "{}: {:?}",
            match u16_at(b, 0) {
                0 => "all".to_string(),
                1 => format!("to slot {}", u16_at(b, 2)),
                4 => "team".to_string(),
                m => format!("mode {m}"),
            },
            str_at(b, 4)
        ),
        0x2028 => {
            let n = u16_at(b, 4) as usize;
            let slots = (0..n.min(8)).filter_map(|i| {
                let w = u16_at(b, 6 + 4 * i);
                (w != 0).then(|| format!("{i}:w{}x{}", w - 1, u16_at(b, 8 + 4 * i)))
            });
            format!("systems {:#x} mounts {}", u32_at(b, 0), list(slots))
        }
        _ => return None,
    };
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::proto::{player_data, ShipState};

    #[test]
    fn describes_player_data_and_tolerates_junk() {
        let s = ShipState { pos: [100 << 16, -5 << 16, 7 << 16], vel: [1 << 16, 0, 0], angles: [0; 3], valid: true };
        let pd = player_data(&s, 42);
        let mut body = vec![0u8; 12];
        body.extend_from_slice(&pd);
        let l = describe(MSG_PLAYERDATA, &body, true);
        assert!(l.iter().any(|x| x.contains("ship at (100, -5, 7)")), "{l:?}");
        // Every kind on short or empty bodies: no panic.
        for k in (0..0x130u16).chain(0x1000..0x1130).chain(0x2000..0x2030) {
            for n in [0, 1, 7, 40] {
                let _ = describe(k, &vec![0xff; n], k >= 0x2000);
            }
        }
    }
}
