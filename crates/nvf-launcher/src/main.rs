//! NVForever's launcher (#2708, docs/design/launcher.md): the project's news, Play/Continue
//! and the settings of the home's `NVForever.ini` in one window, beside `nvforever.exe`.
//! It writes only NVForever's own files (the INI, `plugins.txt`), never the game folder or
//! the original game's (`fnv_install::guard_write`).

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use nvf_launcher::App;

const ICON: &[u8] = include_bytes!("../assets/icon-64.png");

fn main() -> eframe::Result {
    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_title("NVForever Launcher")
        .with_inner_size([1180.0, 760.0])
        .with_min_inner_size([900.0, 560.0]);

    if let Ok(icon) = eframe::icon_data::from_png_bytes(ICON) {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native("NVForever Launcher", options, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}
