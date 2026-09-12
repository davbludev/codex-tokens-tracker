pub(crate) mod monitoring;
pub(crate) mod pricing;
pub(crate) mod runtime;
pub(crate) mod settings;

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
pub async fn usage_call_activity(
    app: tauri::AppHandle,
    runtime: tauri::State<'_, Arc<crate::activity::Runtime>>,
    query: crate::activity::Query,
) -> Result<crate::activity::Page, crate::weekly::ReadError> {
    let path = app
        .path()
        .app_data_dir()
        .map_err(|_| crate::weekly::ReadError::Storage)?
        .join("usage.sqlite");
    let runtime = runtime.inner().clone();
    tauri::async_runtime::spawn_blocking(move || runtime.activity(&path, query))
        .await
        .map_err(|_| crate::weekly::ReadError::Storage)?
}

#[tauri::command]
pub async fn usage_activity_text(
    runtime: tauri::State<'_, Arc<crate::activity::Runtime>>,
    query: crate::activity::TextQuery,
) -> Result<crate::activity::TextPage, String> {
    let runtime = runtime.inner().clone();
    tauri::async_runtime::spawn_blocking(move || runtime.text(query))
        .await
        .map_err(|_| "Activity worker unavailable".to_owned())?
}

#[tauri::command]
pub async fn usage_calls(
    app: tauri::AppHandle,
    query: crate::calls::Query,
) -> Result<crate::calls::Page, crate::weekly::ReadError> {
    query.validate()?;
    let path = app
        .path()
        .app_data_dir()
        .map_err(|_| crate::weekly::ReadError::Storage)?
        .join("usage.sqlite");
    tauri::async_runtime::spawn_blocking(move || Store::read_calls(&path, query))
        .await
        .map_err(|_| crate::weekly::ReadError::Storage)?
}

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

fn publish(app: &tauri::AppHandle, mut snapshot: Snapshot) {
    if snapshot.diagnostic.is_none() {
        snapshot.diagnostic = app
            .state::<crate::desktop::Runtime>()
            .diagnostic
            .lock()
            .ok()
            .and_then(|value| value.clone());
    }
    if let Ok(mut current) = app.state::<State>().0.lock() {
        *current = snapshot.clone();
    }
    let _ = app.emit("usage-updated", snapshot);
    if let Ok(path) = app.path().app_data_dir() {
        crate::desktop::refresh(app, path.join("usage.sqlite"));
    }
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
    #[cfg(test)]
    pub receive: mpsc::Receiver<pricing::Message>,
    pub overflow: Arc<AtomicBool>,
    home: PathBuf,
    watched: Vec<PathBuf>,
}

impl NativeWatch {
    #[cfg(test)]
    pub fn new(home: &Path) -> notify::Result<Self> {
        let (control, receive) = pricing::channel();
        let mut watch = Self::with_control(home, control)?;
        watch.receive = receive;
        Ok(watch)
    }

    fn with_control(home: &Path, control: pricing::Control) -> notify::Result<Self> {
        let overflow = Arc::new(AtomicBool::new(false));
        let callback_overflow = overflow.clone();
        let watcher = notify::recommended_watcher(move |event| {
            if control.0.try_send(pricing::Message::Source(event)).is_err() {
                callback_overflow.store(true, Ordering::Relaxed);
            }
        })?;
        let mut result = Self {
            watcher,
            #[cfg(test)]
            receive: mpsc::channel().1,
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
            .or_else(|| self.home.is_dir().then_some(self.home.as_path()))
            .ok_or_else(|| notify::Error::generic("Source parent is unavailable"))?;
        self.watcher.watch(parent, RecursiveMode::NonRecursive)?;
        self.watched.push(parent.to_path_buf());
        if self.home.is_dir() && self.home != parent {
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
        let runtime = app.state::<monitoring::Runtime>();
        let mut store = match Store::open(&database) {
            Ok(store) => store,
            Err(error) => {
                failure(&app, &error.to_string());
                runtime.mark_finished();
                if runtime.is_stopping() {
                    monitoring::request_exit(&app);
                }
                return;
            }
        };
        let control = app.state::<pricing::Control>().inner().clone();
        let (mut native, mut work) = settings::initialize(&app, &store, &control);
        let mut first = None;
        let mut dirty = true;
        let mut published = Instant::now() - Duration::from_millis(100);
        'writer: loop {
            if runtime.is_stopping() {
                break;
            }
            let events: Vec<_> = first
                .take()
                .into_iter()
                .chain(receive.try_iter().take(255))
                .collect();
            for event in events {
                if runtime.is_stopping() {
                    break 'writer;
                }
                match event {
                    pricing::Message::Wake => (),
                    pricing::Message::Source(event) => {
                        if let Some(native) = &mut native {
                            dirty |= native.accept(&mut work, event, Instant::now());
                        }
                    }
                    pricing::Message::Pricing(request) => {
                        pricing::handle(request, &mut store, &mut work)
                    }
                    pricing::Message::Settings(request) => {
                        let result = settings::apply(
                            request.settings,
                            &mut store,
                            &mut work,
                            &mut native,
                            &control,
                            &crate::desktop::NativePlatform::new(app.clone()),
                        );
                        if let Ok(view) = &result {
                            settings::publish(&app, view.clone());
                            dirty = true;
                        }
                        let _ = request.reply.send(result);
                    }
                }
            }
            if let Some(native) = &mut native {
                dirty |= native.recover_overflow(&mut work);
            }
            if runtime.is_stopping() {
                break;
            }
            match runtime.step(&mut work, &mut store, Instant::now()) {
                Ok(changed) => dirty |= changed,
                Err(error) => {
                    failure(&app, &error.to_string());
                    break;
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
                        break;
                    }
                }
                published = Instant::now();
                dirty = false;
            }
            if runtime.is_stopping() {
                break;
            }
            if work.busy() {
                continue;
            }
            let deadline = work
                .deadline()
                .into_iter()
                .chain(work.retention_deadline())
                .chain(dirty.then_some(published + Duration::from_millis(100)))
                .min();
            // With no queued debounce or publication, wait indefinitely for an event.
            first = match deadline {
                Some(deadline) => {
                    match receive.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                        Ok(event) => Some(event),
                        Err(mpsc::RecvTimeoutError::Timeout) => None,
                        Err(mpsc::RecvTimeoutError::Disconnected) => {
                            failure(&app, "Native source watcher stopped");
                            break;
                        }
                    }
                }
                None => match receive.recv() {
                    Ok(event) => Some(event),
                    Err(_) => {
                        failure(&app, "Native source watcher stopped");
                        break;
                    }
                },
            };
        }
        drop(native);
        if runtime.is_stopping() {
            app.state::<settings::ExportState>().wait_for_idle();
        }
        let closed = store.close();
        runtime.mark_finished();
        if closed.is_err() {
            failure(&app, "The local database could not be checkpointed cleanly. Committed usage is retained; close the app again to exit.");
        } else if runtime.is_stopping() {
            app.exit(0);
        }
    });
}
