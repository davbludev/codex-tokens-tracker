pub(crate) mod pricing;
pub(crate) mod runtime;

use crate::{
    source,
    storage::{Snapshot, Store},
};
use notify::{RecursiveMode, Watcher};
use runtime::Work;
use std::{
    path::{Path, PathBuf},
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

#[tauri::command]
pub async fn usage_aggregates(
    app: tauri::AppHandle,
    query: crate::aggregates::Query,
) -> std::result::Result<crate::aggregates::Response, crate::aggregates::ReadError> {
    let path = app
        .path()
        .app_data_dir()
        .map_err(|_| crate::aggregates::ReadError::Storage)?
        .join("usage.sqlite");
    tauri::async_runtime::spawn_blocking(move || Store::read_aggregates(&path, query))
        .await
        .map_err(|_| crate::aggregates::ReadError::Storage)?
}

#[tauri::command]
pub async fn usage_weekly(
    app: tauri::AppHandle,
    query: crate::weekly::Query,
) -> std::result::Result<crate::weekly::Response, crate::weekly::ReadError> {
    query.validate()?;
    let path = app
        .path()
        .app_data_dir()
        .map_err(|_| crate::weekly::ReadError::Storage)?
        .join("usage.sqlite");
    tauri::async_runtime::spawn_blocking(move || Store::read_weekly(&path, query))
        .await
        .map_err(|_| crate::weekly::ReadError::Storage)?
}

#[tauri::command]
pub async fn usage_weekly_models(
    app: tauri::AppHandle,
    query: crate::weekly::ModelsQuery,
) -> std::result::Result<Option<crate::weekly::Models>, crate::weekly::ReadError> {
    query.validate()?;
    let path = app
        .path()
        .app_data_dir()
        .map_err(|_| crate::weekly::ReadError::Storage)?
        .join("usage.sqlite");
    tauri::async_runtime::spawn_blocking(move || Store::read_weekly_models(&path, query))
        .await
        .map_err(|_| crate::weekly::ReadError::Storage)?
}

#[tauri::command]
pub async fn usage_dashboard(
    app: tauri::AppHandle,
    query: crate::dashboard::Query,
) -> std::result::Result<crate::dashboard::Response, crate::weekly::ReadError> {
    query.validate()?;
    let path = app
        .path()
        .app_data_dir()
        .map_err(|_| crate::weekly::ReadError::Storage)?
        .join("usage.sqlite");
    tauri::async_runtime::spawn_blocking(move || Store::read_dashboard(&path, query))
        .await
        .map_err(|_| crate::weekly::ReadError::Storage)?
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

/// Watch the home non-recursively for missing/replaced source roots. If the home
/// is absent, watch one existing parent and advance toward the home on events.
pub(crate) struct NativeWatch {
    watcher: notify::RecommendedWatcher,
    pub receive: mpsc::Receiver<pricing::Message>,
    pub overflow: Arc<AtomicBool>,
    home: PathBuf,
    watched: Vec<PathBuf>,
}

impl NativeWatch {
    #[cfg(test)]
    pub fn new(home: &Path) -> notify::Result<Self> {
        let (control, receive) = pricing::channel();
        Self::with_inbox(home, control, receive)
    }

    fn with_inbox(
        home: &Path,
        control: pricing::Control,
        receive: mpsc::Receiver<pricing::Message>,
    ) -> notify::Result<Self> {
        let overflow = Arc::new(AtomicBool::new(false));
        let callback_overflow = overflow.clone();
        let watcher = notify::recommended_watcher(move |event| {
            if control.0.try_send(pricing::Message::Source(event)).is_err() {
                callback_overflow.store(true, Ordering::Relaxed);
            }
        })?;
        let mut result = Self {
            watcher,
            receive,
            overflow,
            home: source::normalized_path(home),
            watched: Vec::new(),
        };
        result.refresh()?;
        Ok(result)
    }

    pub fn structural(&self, event: &notify::Event) -> bool {
        matches!(
            event.kind,
            notify::EventKind::Create(_)
                | notify::EventKind::Remove(_)
                | notify::EventKind::Modify(notify::event::ModifyKind::Name(_))
        ) && event.paths.iter().any(|path| {
            let path = source::normalized_path(path);
            self.home.starts_with(&path)
                || path == self.home.join("sessions")
                || path == self.home.join("archived_sessions")
        })
    }

    pub fn accept(
        &mut self,
        work: &mut Work,
        event: notify::Result<notify::Event>,
        now: Instant,
    ) -> bool {
        match event {
            Ok(event) if matches!(event.kind, notify::EventKind::Access(_)) => false,
            Ok(event) => {
                if !event.need_rescan()
                    && !event.paths.iter().any(|path| {
                        let path = source::normalized_path(path);
                        self.home.starts_with(&path)
                            || work.roots.iter().any(|root| path.starts_with(root))
                    })
                {
                    return false;
                }
                if self.structural(&event) {
                    work.recover("Source directories changed; recovering available files");
                    if self.refresh().is_err() {
                        work.failed("Some source directories could not be watched".into());
                    }
                }
                work.event(event, now);
                true
            }
            Err(_) => {
                work.recover("Native watcher reported an error; recovering available files");
                if self.refresh().is_err() {
                    work.failed("Some source directories could not be watched".into());
                }
                true
            }
        }
    }

    pub fn recover_overflow(&mut self, work: &mut Work) -> bool {
        if !self.overflow.swap(false, Ordering::Relaxed) {
            return false;
        }
        work.recover("Source events overflowed; recovering available files");
        if self.refresh().is_err() {
            work.failed("Some source directories could not be watched".into());
        }
        true
    }

    pub fn refresh(&mut self) -> notify::Result<()> {
        for path in self.watched.drain(..) {
            let _ = self.watcher.unwatch(&path);
        }
        let parent = self
            .home
            .ancestors()
            .skip(1)
            .find(|path| path.is_dir())
            .ok_or_else(|| notify::Error::generic("Source parent is unavailable"))?;
        self.watcher.watch(parent, RecursiveMode::NonRecursive)?;
        self.watched.push(parent.to_path_buf());
        if self.home.is_dir() {
            self.watcher
                .watch(&self.home, RecursiveMode::NonRecursive)?;
            self.watched.push(self.home.clone());
        }
        for root in [
            self.home.join("sessions"),
            self.home.join("archived_sessions"),
        ] {
            if root.is_dir() {
                self.watcher.watch(&root, RecursiveMode::Recursive)?;
                self.watched.push(root);
            }
        }
        Ok(())
    }
}

pub fn start(app: tauri::AppHandle, database: PathBuf, receive: mpsc::Receiver<pricing::Message>) {
    std::thread::spawn(move || {
        let Some(directory) = source::sessions_directory() else {
            failure(&app, "Codex home could not be discovered");
            return;
        };
        let home = match std::path::absolute(directory.parent().unwrap()) {
            Ok(home) => home,
            Err(_) => {
                failure(&app, "Codex home could not be resolved");
                return;
            }
        };
        let mut store = match Store::open(&database) {
            Ok(store) => store,
            Err(error) => {
                failure(&app, &error.to_string());
                return;
            }
        };
        // Subscribe before opening the first discovery iterator.
        let control = app.state::<pricing::Control>().inner().clone();
        let mut native = match NativeWatch::with_inbox(&home, control, receive) {
            Ok(watcher) => watcher,
            Err(_) => {
                failure(&app, "Native source watcher could not start");
                return;
            }
        };
        let mut work = Work::new(&home);
        let mut first = None;
        let mut dirty = true;
        let mut published = Instant::now() - Duration::from_millis(100);
        loop {
            let events: Vec<_> = first
                .take()
                .into_iter()
                .chain(native.receive.try_iter().take(255))
                .collect();
            for event in events {
                match event {
                    pricing::Message::Source(event) => {
                        dirty |= native.accept(&mut work, event, Instant::now())
                    }
                    pricing::Message::Pricing(request) => {
                        pricing::handle(request, &mut store, &mut work)
                    }
                }
            }
            dirty |= native.recover_overflow(&mut work);
            match work.step(&mut store, Instant::now()) {
                Ok(changed) => dirty |= changed,
                Err(error) => {
                    failure(&app, &error.to_string());
                    return;
                }
            }
            if dirty && published.elapsed() >= Duration::from_millis(100) {
                match store.snapshot() {
                    Ok(mut snapshot) => {
                        snapshot.coverage = format!("{}. {}", work.progress(), snapshot.coverage);
                        snapshot.source_available = work.roots.iter().any(|path| path.is_dir());
                        if work.diagnostic.is_some() {
                            snapshot.diagnostic = work.diagnostic.clone();
                        }
                        publish(&app, snapshot);
                    }
                    Err(error) => {
                        failure(&app, &error.to_string());
                        return;
                    }
                }
                published = Instant::now();
                dirty = false;
            }
            if work.busy() {
                continue;
            }
            let deadline = work
                .deadline()
                .into_iter()
                .chain(dirty.then_some(published + Duration::from_millis(100)))
                .min();
            // With no queued debounce or publication, wait indefinitely for an event.
            first = match deadline {
                Some(deadline) => match native
                    .receive
                    .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                {
                    Ok(event) => Some(event),
                    Err(mpsc::RecvTimeoutError::Timeout) => None,
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        failure(&app, "Native source watcher stopped");
                        return;
                    }
                },
                None => match native.receive.recv() {
                    Ok(event) => Some(event),
                    Err(_) => {
                        failure(&app, "Native source watcher stopped");
                        return;
                    }
                },
            };
        }
    });
}
