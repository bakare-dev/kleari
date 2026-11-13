mod activity;
mod apps;
mod apps_commands;
mod cleanup;
mod cleanup_commands;
mod commands;
mod developer;
mod explorer_commands;
mod files;
mod files_commands;
mod finder;
mod history;
mod ids;
mod login_item;
mod menubar;
mod mouse;
mod notifications;
mod persistence;
mod preferences;
mod quarantine;
mod schedule;
mod settings_commands;
mod storage;
mod system_monitor;
mod system_settings;
mod uninstall;
mod updates;

use std::sync::Mutex;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(updates::UpdateState::default())
        .manage(Mutex::new(system_monitor::SystemSampler::new()))
        .manage(Mutex::new(system_monitor::ActivitySampler::new()))
        .manage(Mutex::new(system_monitor::ProcessListSampler::new()))
        .manage(storage::StorageState::default())
        .manage(storage::analysis::AnalysisState::default())
        .manage(developer::DeveloperState::default())
        .manage(apps::AppsState::default())
        .manage(storage::explorer::ExplorerState::default())
        .manage(files::FilesState::default())
        .manage(cleanup::CleanupState::default())
        .manage(quarantine::QuarantineState::default())
        .manage(menubar::PopoverState::default())
        .manage(mouse::MouseState::default())
        .setup(|app| {
            // The history and quarantine folder live in the app's data folder. If it can't be
            // found, the history is unavailable and nothing can be moved.
            let folder = app.path().app_data_dir().ok();
            let history = history::History::open(folder);
            let menu_bar = preferences::load(&history.db).preferences.menu_bar;
            app.manage(history);
            // Without the item Kleari still opens, and closing the window quits as usual;
            // Settings › Show in menu bar tries again and reports a failure.
            let _ = menubar::apply(app.handle(), menu_bar);
            // Clicking a scan's notification opens the review.
            notifications::open_review_on_click(app.handle().clone());
            schedule::start(app.handle().clone());
            // The Mouse settings apply from launch if they are on and macOS allows them.
            mouse::start(app.handle());
            Ok(())
        })
        .on_window_event(|window, event| match event {
            // With the menu-bar item on screen, closing the main window keeps Kleari running
            // there.
            tauri::WindowEvent::CloseRequested { api, .. } if window.label() == "main" => {
                if menubar::active(window.app_handle()) {
                    api.prevent_close();
                    // Hiding is best effort; if it fails the window simply stays open.
                    let _ = window.hide();
                } else {
                    // No menu-bar item: closing the window quits, even if another window
                    // (a closing popover) is still around.
                    window.app_handle().exit(0);
                }
            }
            // The popover closes when you click elsewhere.
            tauri::WindowEvent::Focused(false) if window.label() == menubar::POPOVER => {
                menubar::close_on_blur(window);
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_system_metrics,
            commands::get_system_activity,
            commands::get_system_details,
            commands::list_startup_items,
            commands::open_system_settings,
            commands::choose_scan_folder,
            commands::start_scan,
            commands::cancel_scan,
            commands::query_scan,
            commands::get_storage_overview,
            commands::start_storage_analysis,
            commands::cancel_storage_analysis,
            commands::list_processes,
            commands::quit_app,
            commands::start_developer_scan,
            commands::cancel_developer_scan,
            commands::add_developer_folder,
            apps_commands::start_apps_scan,
            apps_commands::cancel_apps_scan,
            apps_commands::send_app_items_to_review,
            apps_commands::remove_app,
            explorer_commands::list_storage_folder,
            explorer_commands::cancel_storage_listing,
            explorer_commands::send_storage_items_to_review,
            files_commands::start_files_scan,
            files_commands::cancel_files_scan,
            files_commands::send_files_to_review,
            cleanup_commands::get_cleanup_scope,
            cleanup_commands::request_folder_access,
            cleanup_commands::add_cleanup_folder,
            cleanup_commands::start_cleanup_scan,
            cleanup_commands::cancel_cleanup_scan,
            cleanup_commands::get_cleanup_review,
            cleanup_commands::send_developer_items_to_review,
            cleanup_commands::add_storage_file_to_review,
            cleanup_commands::reveal_review_item,
            cleanup_commands::create_cleanup_preview,
            cleanup_commands::approve_cleanup_preview,
            cleanup_commands::list_quarantine,
            cleanup_commands::restore_quarantine_items,
            cleanup_commands::delete_quarantine_items,
            cleanup_commands::stop_quarantine_delete,
            cleanup_commands::reveal_quarantine_item,
            cleanup_commands::choose_restore_folder,
            settings_commands::get_settings,
            settings_commands::set_preferences,
            settings_commands::set_open_at_login,
            settings_commands::request_notifications,
            settings_commands::add_excluded_folder,
            settings_commands::remove_excluded_folder,
            settings_commands::remove_folder_access,
            settings_commands::reveal_quarantine_folder,
            settings_commands::list_activity,
            settings_commands::clear_activity,
            settings_commands::uninstall_kleari,
            settings_commands::quit_kleari,
            updates::check_for_update,
            updates::install_update,
            mouse::get_mouse,
            mouse::set_mouse,
            mouse::request_mouse_access,
            mouse::listen_mouse_probe,
            mouse::capture_mouse_probe,
            menubar::open_main,
            menubar::hide_popover
        ])
        .build(tauri::generate_context!())
        .expect("failed to build Kleari desktop shell")
        .run(|_app, _event| {
            // Clicking the Dock icon brings back a window closed into the menu bar (macOS only).
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = _event {
                menubar::show_main(_app);
            }
        });
}
