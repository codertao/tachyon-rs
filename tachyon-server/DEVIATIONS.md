# Deliberate differences from the original host

The server ports the original host's rules, combat and AI as closely as is reasonable, but fixes
obvious bugs. Every such difference is listed here. `--original-bugs` restores the original
behaviour where that is practical.

| Area | Original | This server | `--original-bugs` |
|---|---|---|---|
| Shield facing | Front/rear split uses the hit's world bearing only: a hit from world −z always drains the front pool, so it is right only for a ship facing −z (and backwards for one facing +z) | Bearing measured from the victim's nose (yaw only, as the original ignores pitch) | restores |
| Ramming | `TachSrv_OnCollision` stages "rammed by this ship", but `TachSrv_DamageEntityById` then clears the shooter and leaves the previous hit's weapon factors and hit position staged: rams are never credited and the "used his ship as a ramming device" line never appears | The rammed ship takes the damage at factors 256/256 from the rammer's position; a ram kill credits the rammer and prints that line | restores credit/text only (the stale factors aren't reproduced) |
| Near-entity cap | The 8 near snapshot records are the first 8 in quadrant-list order | The 8 nearest | restores |
| Hit reports | Accepted from shooters who are already dead | Dropped | restores |
| NovaWorld | Heartbeats to the master server; checks the join blob's game ID | Neither | n/a |
| Model / descriptor slots | Filled as ships first appear, so a ship type that joins later gets slots the earlier clients never received in their startup tables | The 11 `vars.cfg` player ships are registered at startup, so every client gets the same table | no |
| Chat commands | Command hook is a stub; every line is relayed | Lines starting with `/` (after the client's `NAME: ` prefix) are answered by the server and not relayed to other players | no |
| Ammo at round start | `MpRules_ResetRound` → `Ship_InitPower` refills only the server's ammo counts; the client keeps what it had left | The server also sends 0x1124 for each ammo mount, so the client's counts match | restores |
| Shield regen rate | PLAYERDATA `+0x2c` used as sent; a modified client can regenerate shields at any speed | Clamped to 0..2.0, the range the stock client cycles through | restores |
| Respawn requests | 0x2011 honoured at any time: alive players teleport to a fresh spawn with full shields and ammo, and a player can leave the between-round freeze early | Ignored unless the player is dead and the round isn't frozen | restores |
| Round timer | Keeps counting through a kill-limit freeze; if it runs out then, the round is frozen a second time | Paused during the freeze | restores |
| Minutes left | `DM_MsgMinutesLeft` tests minutes % 3000 instead of ticks % 3000, so "%d MINUTES LEFT IN THIS ROUND" never appears | Announced on each whole minute while 5 or more minutes are left (the evident intent) | restores (silent) |
| Round freeze | 1000 ticks (20 s) between rounds | 500 ticks (10 s), at the user's request | restores |
| Spawn pitch | `Vec_DirAngles(pos, origin)` also pitches the nose toward the origin | Same yaw (checked against `Math_Atan2Core`), pitch 0; the client owns its pose from then on | no |

Further fixes are proposed to the user before they are added.
