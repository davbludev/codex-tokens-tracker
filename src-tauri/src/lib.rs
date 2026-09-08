mod accounting;
mod adapter;
mod aggregates;
mod commands;
mod dashboard;
mod desktop;
mod export;
mod hierarchy;
mod identity;
mod identity_filesystem;
mod pricing;
mod settings;
mod source;
mod storage;
mod weekly;

use tauri::Manager;

pub fn run() {
    let (pricing_control, pricing_inbox) = commands::pricing::channel();
    tauri::Builder::default()
        .manage(pricing_control)
        .plugin(tauri_plugin_autostart::Builder::new().build())
        .manage(commands::monitoring::Runtime::default())
        .manage(desktop::Runtime::default())
        .manage(commands::settings::Runtime::default())
        .manage(commands::settings::ExportState::default())
        .manage(commands::State(std::sync::Mutex::new(storage::Snapshot {
            coverage: "Discovering local Codex usage…".into(),
            ..Default::default()
        })))
        .invoke_handler(tauri::generate_handler![
            commands::usage_snapshot,
            commands::usage_aggregates,
            commands::usage_weekly,
            commands::usage_weekly_models,
            commands::usage_dashboard,
            commands::pricing::pricing_models,
            commands::pricing::save_model_price,
            commands::settings::tracker_settings,
            commands::settings::save_tracker_settings,
            commands::settings::tracker_diagnostics,
            commands::settings::export_usage_csv
        ])
        .setup(move |app| {
            match app.path().app_data_dir().and_then(|directory| {
                std::fs::create_dir_all(&directory).map_err(tauri::Error::Io)?;
                Ok(directory.join("usage.sqlite"))
            }) {
                Ok(database) => commands::start(app.handle().clone(), database, pricing_inbox),
                Err(_) => {
                    app.state::<commands::monitoring::Runtime>().mark_finished();
                    if let Ok(mut state) = app.state::<commands::State>().0.lock() {
                        state.diagnostic = Some("Local application storage is unavailable".into());
                    }
                }
            }
            Ok(())
        })
        .on_window_event(desktop::window_event)
        .build(tauri::generate_context!())
        .expect("Desktop application runtime could not start")
        .run(|app, event| {
            if let tauri::RunEvent::ExitRequested { api, .. } = event {
                let exports = app.state::<commands::settings::ExportState>();
                exports.begin_shutdown();
                if !app.state::<commands::monitoring::Runtime>().is_finished() || !exports.is_idle()
                {
                    api.prevent_exit();
                    commands::monitoring::request_exit(app);
                }
            }
        });
}

#[cfg(test)]
mod tests;
