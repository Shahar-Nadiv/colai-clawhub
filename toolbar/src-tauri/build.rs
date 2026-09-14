fn main() {
    // Every command the page may call. The same list the permission file grants, because
    // a command declared here and not granted there is unreachable, and one granted and
    // not declared does not exist.
    const COMMANDS: &[&str] = &[
        "colai_agents",
        "colai_allowed",
        "colai_at_work",
        "colai_automate",
        "colai_capture_mark",
        "colai_cut_recording",
        "colai_describe_files",
        "colai_forget_marks",
        "colai_frontmost",
        "colai_in_front",
        "colai_libraries",
        "colai_library_search",
        "colai_models",
        "colai_open_settings",
        "colai_pick_files",
        "colai_pick_folder",
        "colai_points",
        "colai_release",
        "colai_rewind",
        "colai_said",
        "colai_screens",
        "colai_search_files",
        "colai_send",
        "colai_sessions",
        "colai_shape",
        "colai_showing",
        "colai_start_here",
        "colai_stop",
        "colai_take_keyboard",
        "colai_threads",
        "colai_unwatch",
        "colai_watch",
    ];
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("Tauri build configuration should be valid");
}
