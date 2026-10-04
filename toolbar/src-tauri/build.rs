fn main() {
    // Every command the page may call. The same list the permission file grants, because
    // a command declared here and not granted there is unreachable, and one granted and
    // not declared does not exist.
    const COMMANDS: &[&str] = &[
        "colai_allow_now",
        "colai_answer",
        "colai_at_work",
        "colai_came_from",
        "colai_capture_mark",
        "colai_claude_settings",
        "colai_claude_toggle",
        "colai_context_clear_staged",
        "colai_context_resolve",
        "colai_context_save_session",
        "colai_contexts_list",
        "colai_cut_recording",
        "colai_describe_files",
        "colai_frontmost",
        "colai_in_front",
        "colai_pick_files",
        "colai_pick_folder",
        "colai_release",
        "colai_render_shot",
        "colai_revert_edits",
        "colai_said",
        "colai_schedule_add",
        "colai_schedule_list",
        "colai_schedule_open_web",
        "colai_schedule_remove",
        "colai_schedule_run_now",
        "colai_schedule_toggle",
        "colai_screens",
        "colai_search_files",
        "colai_send",
        "colai_sessions",
        "colai_shape",
        "colai_showing",
        "colai_stop",
        "colai_summon",
        "colai_take_keyboard",
        "colai_undo",
    ];
    // The one capability lives inline in `tauri.conf.json`, yet `tauri-build` still tells
    // cargo to rerun this script whenever `capabilities/` changes. A path that does not
    // exist counts as changed on every build, so the whole crate recompiled each time
    // nothing had. The directory therefore exists, holding only a `.gitkeep`: the build reads
    // `.json`/`.toml` files from it and skips anything else, so it adds no capability.
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("Tauri build configuration should be valid");
}
