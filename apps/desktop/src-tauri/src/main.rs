// Hide the extra console window on Windows in release builds; keep it in
// debug so `tracing`/panics stay visible during the spike.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    dbboard_desktop_lib::run();
}
