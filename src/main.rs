#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod engine;
mod gui;
mod i18n;

fn main() -> eframe::Result {
    eframe::run_native(
        engine::APP_ID,
        gui::native_options(),
        Box::new(|cc| Ok(Box::new(gui::App::new(cc)))),
    )
}
