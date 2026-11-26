mod branches;
mod commands;
mod error;
mod git;
mod github;
mod importer;
mod jumplist;
mod launch;
mod meta;
mod naming;
mod settings;
mod system;
mod updates;
mod workspace;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    updates::finish_update();
    jumplist::init_process();
    tauri::Builder::default()
        // First, so a second launch hands over its arguments before anything starts.
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| launch::second_launch(app, argv)))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            use tauri::Manager;
            // Before any git work: let the built-in git handle long paths.
            if let Ok(dir) = app.path().app_config_dir() {
                git::enable_long_paths(&dir);
            }
            // Jump-list launches: `--workspace <path>` or `--new-window`.
            let args: Vec<String> = std::env::args().skip(1).collect();
            app.state::<launch::Windows>().set_intent("main", launch::Intent::from_launch(launch::parse(&args)));
            jumplist::repair_shortcuts();
            jumplist::refresh(settings::load(app.handle()).workspaces);
            updates::watch(app.handle());
            // The window starts hidden so it never flashes white; the UI shows
            // it after its first frame. Safety net in case that never happens.
            if let Some(w) = app.get_webview_window("main") {
                jumplist::tag_window(&w);
                launch::show_fallback(w);
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            use tauri::Manager;
            match event {
                tauri::WindowEvent::Focused(true) => window.state::<launch::Windows>().focused(window.label()),
                tauri::WindowEvent::Destroyed => {
                    let st = window.state::<launch::Windows>();
                    let showed = st.showing_in(window.label());
                    st.closed(window.label());
                    // The last window closing decides what a plain start
                    // reopens: its workspace, or the start screen.
                    let app = window.app_handle();
                    if !app.webview_windows().keys().any(|l| l != window.label()) {
                        let _ = settings::set_last_workspace(app, showed.as_deref());
                    }
                }
                _ => {}
            }
        })
        .manage(commands::AppState::new())
        .manage(launch::Windows::default())
        .manage(updates::Updates::default())
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::set_github_client_id,
            commands::forget_workspace,
            commands::list_workspaces,
            launch::launch_intent,
            launch::set_window_workspace,
            commands::environment,
            updates::update_status,
            updates::check_for_update,
            updates::install_update,
            commands::inspect_folder,
            commands::init_workspace,
            commands::open_workspace,
            commands::scan_projects,
            commands::git_status,
            commands::git_submodules,
            commands::attach_submodules,
            commands::git_fetch,
            commands::git_pull,
            commands::git_full_history,
            commands::git_branches,
            commands::git_switch_branch,
            commands::git_checkout_remote,
            commands::git_create_branch,
            commands::git_update_branch,
            commands::git_delete_branch,
            commands::github_branches,
            commands::github_search,
            commands::app_icons,
            commands::add_category,
            commands::update_category,
            commands::delete_category,
            commands::set_project_categories,
            commands::set_project_name,
            commands::move_project,
            commands::unarchive_project,
            commands::adopt_project,
            commands::suggest_folder_name,
            commands::create_project,
            commands::parse_repo,
            commands::clone_repo,
            commands::cancel_job,
            commands::inspect_import,
            commands::import_project,
            commands::github_account,
            commands::github_start_device,
            commands::github_poll_device,
            commands::github_sign_in_token,
            commands::github_sign_out,
            commands::github_list_repos,
            commands::open_in_explorer,
            commands::open_in_vscode,
            commands::open_in_terminal,
            commands::open_url,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Hangar");
}
