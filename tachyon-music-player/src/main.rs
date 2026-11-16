//! Tachyon: The Fringe music player.
//!
//! Usage: tachyon-music-player [PFF_DIRECTORY | Tachyon.pff]
//!
//! Loops single `Ttfmusic.sbf` segments, or plays a theme sequenced by the game's
//! `TTFMUSIC.BIN` script with settable game variables (hostiles and friends).

mod app;
mod audio;

fn main() -> eframe::Result {
    let path = std::env::args_os().nth(1).map(std::path::PathBuf::from);
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Tachyon Music Player")
            .with_inner_size([960.0, 640.0]),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_native("Tachyon Music Player", options, Box::new(move |_cc| Ok(Box::new(app::MusicApp::new(path)))))
}
