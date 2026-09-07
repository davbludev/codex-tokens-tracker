use crate::{
    source,
    storage::{Snapshot, Store},
};
use notify::{RecursiveMode, Watcher};
use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tauri::{Emitter, Manager};

pub struct State(pub Mutex<Snapshot>);

#[tauri::command]
pub fn usage_snapshot(state: tauri::State<'_, State>) -> std::result::Result<Snapshot, String> {
    state
        .0
        .lock()
        .map(|s| s.clone())
        .map_err(|_| "Usage state is unavailable".into())
}
fn publish(app: &tauri::AppHandle, snapshot: Snapshot) {
    if let Ok(mut current) = app.state::<State>().0.lock() {
        *current = snapshot.clone();
    }
    let _ = app.emit("usage-updated", snapshot);
}
fn failure(app: &tauri::AppHandle, message: &str) {
    let mut snapshot = app
        .state::<State>()
        .0
        .lock()
        .map(|s| s.clone())
        .unwrap_or_default();
    snapshot.diagnostic = Some(message.into());
    snapshot.source_available = false;
    if snapshot.direct_tokens.is_none() {
        snapshot.coverage = "Local usage is unavailable".into();
    }
    publish(app, snapshot);
}

pub fn start(app: tauri::AppHandle, database: PathBuf) {
    std::thread::spawn(move || {
        let Some(directory) = source::sessions_directory() else {
            failure(&app, "Codex home could not be discovered");
            return;
        };
        if !directory.is_dir() {
            failure(
                &app,
                "Codex sessions directory is missing. Start Codex, then restart this monitor.",
            );
            return;
        }
        let mut store = match Store::open(&database) {
            Ok(s) => s,
            Err(e) => {
                failure(&app, &e.to_string());
                return;
            }
        };
        let (send, receive) = mpsc::sync_channel(256);
        let overflow = Arc::new(AtomicBool::new(false));
        let callback_overflow = overflow.clone();
        let mut watcher = match notify::recommended_watcher(move |event| {
            if send.try_send(event).is_err() {
                callback_overflow.store(true, Ordering::Relaxed);
            }
        }) {
            Ok(w) => w,
            Err(_) => {
                failure(&app, "Native source watcher could not start");
                return;
            }
        };
        // Register before reading to close the discovery/read append race.
        if watcher.watch(&directory, RecursiveMode::Recursive).is_err() {
            failure(&app, "Codex sessions could not be watched");
            return;
        }
        let mut error = None;
        match source::latest_rollout(&directory) {
            Ok(Some(path)) => {
                if let Err(e) = source::ingest(&mut store, &path) {
                    error = Some(e.to_string());
                }
            }
            Ok(None) => (),
            Err(_) => error = Some("Codex sources could not be discovered".into()),
        }
        publish_result(&app, &store, error);
        while let Ok(first) = receive.recv() {
            let started = Instant::now();
            let mut paths = HashSet::new();
            let mut error = None;
            for event in std::iter::once(first).chain(receive.try_iter().take(255)) {
                match event {
                    Ok(event) => {
                        if matches!(
                            event.kind,
                            notify::EventKind::Create(_) | notify::EventKind::Modify(_)
                        ) {
                            for path in event.paths {
                                if path.starts_with(&directory) && source::is_rollout(&path) {
                                    paths.insert(path);
                                }
                            }
                        }
                    }
                    Err(_) => error = Some("Native source watcher reported an error".into()),
                }
            }
            for path in paths {
                if let Err(e) = source::ingest(&mut store, &path) {
                    error = Some(e.to_string());
                }
            }
            if overflow.swap(false, Ordering::Relaxed) {
                error = Some("Source event queue overflowed; coverage may be incomplete. Restart to resume the latest source.".into());
            }
            // Bound IPC frequency without polling while idle.
            if let Some(remaining) = Duration::from_millis(100).checked_sub(started.elapsed()) {
                std::thread::sleep(remaining);
            }
            publish_result(&app, &store, error);
        }
        failure(&app, "Native source watcher stopped");
    });
}
fn publish_result(app: &tauri::AppHandle, store: &Store, error: Option<String>) {
    match store.snapshot() {
        Ok(mut snapshot) => {
            if error.is_some() {
                snapshot.diagnostic = error;
            }
            publish(app, snapshot);
        }
        Err(e) => failure(app, &e.to_string()),
    }
}
