//! The UI: a clip / theme list on the left, transport, game variables and history on the right.

use crate::audio::Output;
use eframe::egui;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tachyon_core::music::{self, NowPlaying, Request, GAME_VARS, VAR_NAMES};
use tachyon_core::source::AssetSource;

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Clips,
    Themes,
}

/// What's loaded: the segment and theme lists, and handles into the playing `Player`.
struct Library {
    sbf_path: PathBuf,
    /// (name, seconds) per segment.
    segs: Vec<(String, f32)>,
    /// (state, label) for every script state with code.
    themes: Vec<(usize, String)>,
    now: Arc<Mutex<NowPlaying>>,
    gain: Arc<Mutex<f32>>,
}

pub struct MusicApp {
    path_input: String,
    /// Opened on first load; `None` inside once that failed.
    output: Option<Option<Output>>,
    lib: Option<Library>,
    error: Option<String>,
    tab: Tab,
    filter: String,
    /// The last thing picked, to highlight it and step from it with the arrow keys.
    picked: Option<Request>,
    scroll_to_picked: bool,
    vars: [i32; GAME_VARS],
    hostiles: bool,
    hostile_count: i32,
    free_run: bool,
    paused: bool,
    volume: f32,
}

impl MusicApp {
    pub fn new(path: Option<PathBuf>) -> Self {
        let mut app = Self {
            path_input: path.as_ref().map(|p| p.display().to_string()).unwrap_or_default(),
            output: None,
            lib: None,
            error: None,
            tab: Tab::Clips,
            filter: String::new(),
            picked: None,
            scroll_to_picked: false,
            vars: [0; GAME_VARS],
            hostiles: false,
            hostile_count: 3,
            free_run: false,
            paused: true,
            volume: 0.6,
        };
        if path.is_some() {
            app.open();
        }
        app
    }

    fn open(&mut self) {
        let path = PathBuf::from(self.path_input.trim());
        match self.load(&path) {
            Ok(lib) => {
                self.lib = Some(lib);
                self.error = None;
                self.picked = None;
                self.paused = true;
                self.push_settings();
            }
            Err(e) => self.error = Some(format!("{e:#}")),
        }
    }

    fn load(&mut self, path: &Path) -> anyhow::Result<Library> {
        let src = AssetSource::open(path)?;
        let sbf_path = music::Sbf::find(path)
            .ok_or_else(|| anyhow::anyhow!("Ttfmusic.sbf not found (it's in the game folder, beside Tachyon.pff)"))?;
        let sbf = Arc::new(music::Sbf::open(&sbf_path)?);
        let script = Arc::new(music::Script::parse(src.read("TTFMUSIC.BIN")?)?);
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u32);
        let player = music::Player::new(sbf.clone(), script, seed, music::THEMES[0]);
        let lib = Library {
            sbf_path,
            segs: (0..sbf.segs.len()).map(|i| (sbf.segs[i].0.clone(), sbf.seconds(i))).collect(),
            themes: player.state_labels(),
            now: player.now.clone(),
            gain: player.gain.clone(),
        };
        let out = self.output.get_or_insert_with(Output::new).as_ref().ok_or_else(|| anyhow::anyhow!("no audio output device"))?;
        // Paused until something is picked.
        player.now.lock().unwrap().paused = true;
        *out.slot.lock().unwrap() = Some(player);
        Ok(lib)
    }

    /// Copy the UI's settings into the player.
    fn push_settings(&mut self) {
        self.vars[0] = if self.hostiles { self.hostile_count.max(1) } else { 0 };
        let Some(lib) = &self.lib else { return };
        if let Ok(mut n) = lib.now.lock() {
            n.vars = self.vars;
            n.paused = self.paused;
            n.free_run = self.free_run;
        }
        if let Ok(mut g) = lib.gain.lock() {
            *g = self.volume;
        }
    }

    fn pick(&mut self, r: Request) {
        if let Some(lib) = &self.lib {
            if let Ok(mut n) = lib.now.lock() {
                n.select = Some(r);
            }
        }
        self.picked = Some(r);
        self.paused = false;
        self.push_settings();
    }

    fn now(&self) -> Option<NowPlaying> {
        self.lib.as_ref().and_then(|l| l.now.lock().ok().map(|n| n.clone()))
    }

    /// The current tab's entries after the filter, as (request, label).
    fn entries(&self) -> Vec<(Request, String)> {
        let Some(lib) = &self.lib else { return vec![] };
        let f = self.filter.to_lowercase();
        let keep = |s: &str| f.is_empty() || s.to_lowercase().contains(&f);
        match self.tab {
            Tab::Clips => lib
                .segs
                .iter()
                .enumerate()
                .map(|(i, (n, s))| (Request::Clip(i), format!("{i:>3}  {n:<16} {s:>5.1} s")))
                .filter(|e| keep(&e.1))
                .collect(),
            Tab::Themes => lib.themes.iter().map(|(s, l)| (Request::Theme(*s), l.clone())).filter(|e| keep(&e.1)).collect(),
        }
    }

    fn side_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.tab, Tab::Clips, "Clips").on_hover_text("Loop one Ttfmusic.sbf segment");
            ui.selectable_value(&mut self.tab, Tab::Themes, "Themes")
                .on_hover_text("Play a script state: the game's TTFMUSIC.BIN picks each next segment");
        });
        ui.horizontal(|ui| {
            ui.label("Filter");
            ui.add(egui::TextEdit::singleline(&mut self.filter).desired_width(f32::INFINITY));
        });
        ui.separator();
        let entries = self.entries();
        // Up/down step through the list (unless a text box has focus).
        let typing = ui.ctx().wants_keyboard_input();
        let (up, down) = ui.input(|i| (i.key_pressed(egui::Key::ArrowUp), i.key_pressed(egui::Key::ArrowDown)));
        if !typing && (up || down) && !entries.is_empty() {
            let cur = entries.iter().position(|e| Some(e.0) == self.picked);
            let next = match (cur, down) {
                (None, _) => 0,
                (Some(i), true) => (i + 1).min(entries.len() - 1),
                (Some(i), false) => i.saturating_sub(1),
            };
            self.pick(entries[next].0);
            self.scroll_to_picked = true;
        }
        let mut clicked = None;
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            for (r, label) in &entries {
                let sel = Some(*r) == self.picked;
                let resp = ui.selectable_label(sel, egui::RichText::new(label).monospace());
                if resp.clicked() {
                    clicked = Some(*r);
                }
                if sel && self.scroll_to_picked {
                    resp.scroll_to_me(Some(egui::Align::Center));
                }
            }
        });
        self.scroll_to_picked = false;
        if let Some(r) = clicked {
            self.pick(r);
        }
    }

    fn main_panel(&mut self, ui: &mut egui::Ui) {
        let Some(now) = self.now() else {
            ui.label("Open an extracted pff directory or Tachyon.pff; Ttfmusic.sbf is looked for beside it.");
            return;
        };
        let mut changed = false;

        // Transport.
        ui.horizontal(|ui| {
            let label = if self.paused { "▶ Play" } else { "⏸ Pause" };
            if ui.button(label).on_hover_text("Space").clicked() {
                self.paused = !self.paused;
                changed = true;
            }
            if ui.button("⟲ Restart").on_hover_text("Start the clip or theme again from its top").clicked() {
                let r = self.picked.unwrap_or(match now.clip {
                    Some(c) => Request::Clip(c),
                    None => Request::Theme(now.home),
                });
                self.pick(r);
            }
            ui.label("Volume");
            changed |= ui.add(egui::Slider::new(&mut self.volume, 0.0..=1.5).show_value(false)).changed();
        });
        ui.add_space(6.0);

        // Now playing.
        let mode = match now.clip {
            Some(c) => format!("Looping clip {c}"),
            None => {
                let label = self.lib.as_ref().and_then(|l| l.themes.iter().find(|t| t.0 == now.home)).map_or("", |t| t.1.as_str());
                format!("Theme {label}")
            }
        };
        ui.heading(mode);
        let seg = if now.segment.is_empty() { "(silence)".to_string() } else { now.segment.clone() };
        let mut line = match now.clip {
            Some(_) => seg,
            None => format!("state {} · {seg}", now.state),
        };
        if now.clip.is_none() && now.terminal_secs > 0.0 && !self.free_run {
            line += &format!(" · theme ended, restarting in {:.0} s", (music::TERMINAL_SECONDS - now.terminal_secs).max(0.0));
        }
        ui.label(line);
        let frac = if now.seg_len > 0.0 { (now.seg_pos / now.seg_len).clamp(0.0, 1.0) } else { 0.0 };
        ui.add(egui::ProgressBar::new(frac).text(format!("{:.1} / {:.1} s", now.seg_pos, now.seg_len)));
        if self.paused {
            ui.small("Paused");
        }
        ui.separator();

        // Game variables.
        ui.add_enabled_ui(now.clip.is_none(), |ui| {
            ui.horizontal(|ui| {
                changed |= ui
                    .checkbox(&mut self.hostiles, "Hostiles present")
                    .on_hover_text(
                        "Game variable 0, the number of hostiles. Themes check it between bars: hostiles \
                         appearing switch to the combat states (state 2 → 11, the f1i1e battle bars); \
                         when they leave, the script plays a combat ending (state 9 FM1Cda after a big \
                         fight: peak over 6, then under 2 left; otherwise state 10 FM1EL).",
                    )
                    .changed();
                ui.add_enabled_ui(self.hostiles, |ui| {
                    changed |= ui.add(egui::DragValue::new(&mut self.hostile_count).range(1..=50).prefix("count ")).changed();
                });
            });
            changed |= ui
                .checkbox(&mut self.free_run, "Free run")
                .on_hover_text(format!(
                    "Let the script run on by itself after the theme ends, instead of restarting the \
                     theme after {:.0} s in the terminal states",
                    music::TERMINAL_SECONDS
                ))
                .changed();
            egui::CollapsingHeader::new("Other game variables").show(ui, |ui| {
                ui.small(
                    "Script memory slots set by the game (Music_UpdateFlightVars). sector = region × 1000 + \
                     sector (EARTH 0, NEUTRAL 1, GALSPAN 2, BORA 3, FRONTIER 4, MYSTERY 5, DISPUTED 6, \
                     GDISPUTE 7, BDISPUTE 8); job = CURRENT_JOB id. The dispatcher (state 1) reads them; \
                     pick a theme again to restart with them.",
                );
                egui::Grid::new("vars").num_columns(2).show(ui, |ui| {
                    for k in 1..GAME_VARS {
                        ui.label(VAR_NAMES[k]);
                        changed |= ui.add(egui::DragValue::new(&mut self.vars[k])).changed();
                        ui.end_row();
                    }
                });
                if ui.button("Reset").clicked() {
                    self.vars[1..].fill(0);
                    changed = true;
                }
            });
        });
        if now.clip.is_some() {
            ui.small("Game variables apply to themes; clips bypass the script.");
        }
        ui.separator();

        // History.
        ui.label("History");
        egui::ScrollArea::vertical().auto_shrink(false).stick_to_bottom(true).show(ui, |ui| {
            for (state, seg) in &now.history {
                let t = match state {
                    Some(s) => format!("{s:>3}  {seg}"),
                    None => format!("  -  {seg}"),
                };
                ui.label(egui::RichText::new(t).monospace().small());
            }
        });

        if changed {
            self.push_settings();
        }
        if !self.paused {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
        }
    }
}

impl eframe::App for MusicApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.lib.is_some() && !ctx.wants_keyboard_input() && ctx.input(|i| i.key_pressed(egui::Key::Space)) {
            self.paused = !self.paused;
            self.push_settings();
        }
        egui::TopBottomPanel::top("path").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let open = ui.button("Open").clicked();
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut self.path_input)
                            .hint_text("pff directory or Tachyon.pff")
                            .desired_width(f32::INFINITY),
                    );
                    if open || (resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                        self.open();
                    }
                });
            });
            if let Some(e) = &self.error {
                ui.colored_label(ui.visuals().error_fg_color, e);
            } else if let Some(lib) = &self.lib {
                ui.small(format!("{} · {} segments · {} script states", lib.sbf_path.display(), lib.segs.len(), lib.themes.len()));
            }
            ui.add_space(4.0);
        });
        if self.lib.is_some() {
            egui::SidePanel::left("list").resizable(true).default_width(320.0).show(ctx, |ui| self.side_panel(ui));
        }
        egui::CentralPanel::default().show(ctx, |ui| self.main_panel(ui));
    }
}
