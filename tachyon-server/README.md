# tachyon-server

A console dedicated multiplayer server for *Tachyon: The Fringe*, speaking the original Netsock
protocol so stock `space.exe` clients can join. Deathmatch and Base Wars maps, assets from
`Tachyon.pff` or an unpacked directory.

Status: **M3 in progress (server weapons)**; M2 (PvP Deathmatch) is done. Players join, load, fly, see each other, chat and
leave (M1). Combat follows the stock host: shots are relayed, hit reports drain shields and hull,
kills are credited with the original kill lines, players respawn, shields regenerate and power
transfers work; collisions hurt both sides. Rounds end at the kill limit or time limit, freeze
for 10 s and restart (ammo refilled). Asteroids, starbases and powerups are built from the map and sent the way
the stock host sends them. Starbase subsystems (generators, comm arrays) can be shot off; hull
sections can't. Powerups are picked up by flying into them and come back after the map's delay
(60 s on the stock maps). Hyperspace gates, beacons, mines and speed rings are drawn, but gates
don't take you anywhere and mines don't explode yet (speed rings are run by the client and
may already work). Not built yet: SPX ships; credit crystals spawn but can't be picked up until
Base Wars credits exist. Missiles, torpedoes and the Helios rocket are flown by the server: they
home with the original turn rates and lock rules, warn their target, can be decoyed or shot down,
and hit with sphere tests (meshes come in M7); Blast torpedoes leave a growing damage sphere that
also hurts the shooter, as in the original. Turrets, AI pilots and Base Wars rules come in later
milestones (see the plan).
Deliberate differences from the original host: `DEVIATIONS.md`.

## Running

```
cargo run --release -p tachyon-server -- --assets /path/to/Tachyon.pff --list-maps
cargo run --release -p tachyon-server -- --assets /path/to/Tachyon.pff --map DM_01 --log-packets packets.log
```

Options: `--port` (default 2766, the port the game's Network screen uses), `--name`,
`--max-players`, `--password`, `--max-kills <n>` (Deathmatch kill limit, default 20, 0 = none),
`--time-limit <minutes>` (round length, default 30, 0 = untimed), `--no-powerups` (the
Deathmatch POWERUPS option off), `--debug-damage` (print each
hit's damage calculation, how it splits between front and rear shields, and the shield and hull
values before and after; also collisions, power transfers, starbase part damage, powerup
pickups, missile launches, hits and blasts. Toggle at runtime with the console
command `/debug damage [on|off]`), `--original-bugs`,
`--log-packets <file>`, `--log-detail`.

After a round ends the same map restarts; the original moves on to the next map in its list,
which this server doesn't do yet.

`--log-packets` writes every engine packet in and out with a hex dump, plus the Netsock session
messages (enumerate, join, create player, leave and their replies, interpreted) and datagrams
that fail the CRC check. Adding `--log-detail`
also writes what each message means, under the message, and dumps bodies in full. For example:

```
    2.150 <- slot 0: seq 4 ack 3, 114 bytes
            0x0003 PLAYERDATA [88]
              = player 0x10001, client frame 156, session 41547
              = ship at (-1943, 625, -1500) vel (-0.1, 0.0, 4.7) (4.7 u/tick) angles [356°, 0°, 0°] throttle 512/1024
```

Console: `/status`, `/kick <slot>`, `/say <text>` (or just type a line), `/quit`. Players can use
`/status` and `/help` from in-game chat.

## Joining from a stock client

The game joins a specific host only through its Network (NovaWorld) browser: a page whose
`<title>` holds an encoded `host:port` makes the client connect. The server writes that page:

```
tachyon-server --join-page 192.168.1.20 --port 2766 > Startup.htm
```

1. Back up the game's `Startup.htm` (in the game folder) and replace it with the generated one.
2. Start the game, Multiplayer → **Network**. Wait for the NovaWorld ping to time out (about
   10 s) and press **Continue**. The browser opens the page and the game starts joining.
   **Don't press Skip** (it appears after a couple of seconds, in the same place). Skipping
   leaves Netsock's ping thread running. That thread then competes with the join for the
   server's replies, and it crashes the client 30 s later (`page fault on write access to
   0x00000008` in `netsock+0x3b9b`). This is a stock-client bug; see `multiplayer.md`
   "NovaWorld (online lobby)".

`host:port` must be at most 21 characters (a limit of the game's decoder).

## Testing without the game

`tachyon-bot` is a headless client: `tachyon-bot 127.0.0.1:2766 --name Bot --ship BORA_CUTLASS
--team 1 --say "/status"`.
