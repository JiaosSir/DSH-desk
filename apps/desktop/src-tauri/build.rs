fn main() {
    // 把壳的全部自定义命令注册进应用 ACL（生成 allow-<cmd> 权限），
    // 否则远程页面（sidecar 的 http://127.0.0.1）上的 invoke 会被
    // 「远程来源」安全网拒绝——见 capabilities/default.json 的权限列表。
    tauri_build::try_build(
        tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(&[
            "desktop_state",
            "desktop_retry",
            "desktop_open_logs",
            "desktop_quit",
            "desktop_get_onboarding",
            "desktop_open_releases",
            "desktop_set_autostart",
            "desktop_get_autostart",
            "desktop_get_hotkey",
            "desktop_notify",
            "desktop_sync_list",
            "desktop_sync_add",
        ])),
    )
    .expect("error running tauri-build");
}
