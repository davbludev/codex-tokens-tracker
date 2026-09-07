mod accounting;
mod adapter;
mod aggregates;
mod commands;
mod hierarchy;
mod identity;
mod identity_filesystem;
mod pricing;
mod source;
mod storage;

use tauri::Manager;

pub fn run() {
    let (pricing_control, pricing_inbox) = commands::pricing::channel();
    tauri::Builder::default()
        .manage(pricing_control)
        .manage(commands::State(std::sync::Mutex::new(storage::Snapshot {
            coverage: "Discovering local Codex usage…".into(),
            ..Default::default()
        })))
        .invoke_handler(tauri::generate_handler![
            commands::usage_snapshot,
            commands::usage_aggregates,
            commands::pricing::pricing_models,
            commands::pricing::save_model_price
        ])
        .setup(move |app| {
            match app.path().app_data_dir().and_then(|directory| {
                std::fs::create_dir_all(&directory).map_err(tauri::Error::Io)?;
                Ok(directory.join("usage.sqlite"))
            }) {
                Ok(database) => commands::start(app.handle().clone(), database, pricing_inbox),
                Err(_) => {
                    if let Ok(mut state) = app.state::<commands::State>().0.lock() {
                        state.diagnostic = Some("Local application storage is unavailable".into());
                    }
                }
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("Desktop application runtime could not start");
}

#[cfg(test)]
mod tests;
