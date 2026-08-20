// Prevents an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if std::env::var_os("DSH_DESK_SMOKE").is_some() {
        dsh_desk_lib::smoke::run()
    }
    dsh_desk_lib::run()
}
