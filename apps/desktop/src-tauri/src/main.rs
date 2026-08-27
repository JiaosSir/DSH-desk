// Prevents an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // 冒烟模式（DSH_DESK_SMOKE）以 0/1 退出进程，不会执行到正常启动。
    if std::env::var_os("DSH_DESK_SMOKE").is_some() {
        dsh_desk_lib::smoke::run()
    }
    dsh_desk_lib::run()
}
