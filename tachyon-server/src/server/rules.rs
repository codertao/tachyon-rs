//! Deathmatch round rules (mode table `0x534b50`): the max-kills check, the round timer, the
//! freeze between rounds and the round reset. Base Wars uses the same freeze/reset path; its
//! own hooks arrive in M6.

use super::*;

/// `Round_FreezeTicks` with `DAT_005b2b18` unset: 1000 ticks (20 s).
pub const FREEZE_TICKS: u32 = 1000;
/// Ticks per minute at 50 Hz (`spacesav[0x61]` × 3000).
const TICKS_PER_MINUTE: u32 = 3000;

impl Server {
    /// `DM_InitRules`: the round timer from the time limit.
    pub(super) fn start_round_timer(&mut self) {
        self.round_ticks = self.opts.time_limit_min * TICKS_PER_MINUTE;
    }

    /// `DM_OnPlayerKilled`: the killer reaching the kill limit ends the round.
    pub(super) fn rules_on_kill(&mut self, killer_kills: u16) {
        if self.map.config.game_type == GameType::Deathmatch
            && self.opts.max_kills > 0
            && u32::from(killer_kills) >= self.opts.max_kills
            && self.freeze_ticks == 0
        {
            self.round_freeze();
        }
    }

    /// `SrvSend_RoundFreeze` + `Srv_ForAllConns`: 0x1119 to everyone and every player counts as
    /// dead (no damage) until the next round.
    fn round_freeze(&mut self) {
        self.queue(ev::EV_ROUND_FREEZE, vec![0, 0], Delivery { budget: BUDGET_RELIABLE, ..Default::default() });
        for c in self.conns.iter_mut().flatten() {
            c.dead = true;
        }
        self.freeze_ticks = FREEZE_TICKS;
        println!("round over");
        for line in self.status_lines(false).into_iter().skip(1) {
            println!("{line}");
        }
    }

    /// `DM_RoundTimerTick`, once per tick.
    pub(super) fn round_tick(&mut self) {
        if self.freeze_ticks != 0 {
            self.freeze_ticks -= 1;
            if self.freeze_ticks == 0 {
                self.next_round();
            }
        }
        if self.round_ticks != 0 {
            self.round_ticks -= 1;
            if self.round_ticks == 0 {
                // `DM_EndRound`.
                self.scoreboard();
                self.round_freeze();
            }
            // `DM_MsgMinutesLeft` would announce "%d MINUTES LEFT IN THIS ROUND", but its test
            // (minutes % 3000 == 0, minutes >= 5) never passes, so the stock host is silent.
        }
    }

    /// `DM_NextRound`. The original changes map after `spacesav[0x59]` rounds (1 by default);
    /// this server has no map rotation yet, so every round restarts the same map.
    fn next_round(&mut self) {
        self.reset_round();
        self.queue(ev::EV_ROUND_RESUME, vec![0, 0], Delivery { budget: BUDGET_RELIABLE, ..Default::default() });
        self.start_round_timer();
        println!("new round");
    }

    /// `MpRules_ResetRound`: bump the session (0x1033), rebuild the statics from the SPX, and
    /// per player `DM_OnPlayerReset` (alive, kills and deaths cleared, new spawn point moved in
    /// with 0x1007) then `Ship_InitPower`.
    pub(super) fn reset_round(&mut self) {
        self.session = self.session.wrapping_add(1);
        self.queue(ev::EV_SESSION_BUMP, ev::session_bump(self.session), Delivery::all());
        self.respawn_statics();
        let slots: Vec<u16> = self.conns.iter().flatten().map(|c| c.slot).collect();
        let bdf = self.map.spx.backdrop.clone();
        let quadrant = self.map.spx.sector_id as u16;
        for slot in slots {
            let (pos, angles) = self.dm_spawn_pos();
            let ship = {
                let c = self.conn_mut(slot).unwrap();
                c.dead = false;
                c.kills = 0;
                c.deaths = 0;
                c.camera = pos;
                c.in_quadrant = false;
                c.far_cursor = None;
                c.ship
            };
            self.place_ship(ship, pos, angles);
            if let Some(e) = self.world.get_mut(ship) {
                e.target = 0;
            }
            // `Srv_MovePlayerToQuadrant`: static list, then 0x1007 (which also makes the client
            // re-confirm with 0x2008 kind 2 before records resume).
            self.build_static_list(slot);
            self.queue(ev::EV_ENTER_QUADRANT, ev::enter_quadrant(pos, angles, quadrant, 1, 1, &bdf), Delivery::only(slot));
            self.init_ship_power(slot);
        }
        self.scoreboard();
    }
}
