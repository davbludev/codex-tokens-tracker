//! Settings changes share the ingestion writer and replace watches only after validation.
use super::{pricing, runtime::Work, NativeWatch, State};
use crate::{
    settings::{self, Config, Diagnostics, Error, View},
    storage::Store,
};
use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
};
use tauri::Manager;

#[derive(Default)]
pub struct Runtime(pub Mutex<Option<View>>);

#[derive(Default)]
pub struct ExportState(Arc<AtomicBool>);

struct ExportPermit(Arc<AtomicBool>);

impl Drop for ExportPermit {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

pub(crate) struct Request {
    pub settings: Config,
    pub reply: mpsc::Sender<Result<View, Error>>,
}

pub(crate) fn publish(app: &tauri::AppHandle, view: View) {
    if let Ok(mut current) = app.state::<Runtime>().0.lock() {
        *current = Some(view);
    }
}

fn subscribe(home: &Path, control: &pricing::Control) -> Result<NativeWatch, Error> {
    // The coordinator owns the shared inbox. This watch contributes events to it.
    NativeWatch::with_control(home, control.clone()).map_err(|_| {
        Error::new(
            "invalid_directory",
            "The Codex directory could not be watched. Check access and try again.",
        )
    })
}

pub(crate) fn initialize(
    app: &tauri::AppHandle,
    store: &Store,
    control: &pricing::Control,
) -> (Option<NativeWatch>, Work) {
    let resolved = store
        .tracker_settings()
        .map_err(|_| Error::storage())
        .and_then(settings::resolve);
    match resolved {
        Ok((view, home)) => {
            publish(app, view);
            match home {
                Some(home) => {
                    // Subscribe before creating the initial discovery iterator.
                    match subscribe(&home, control) {
                        Ok(watch) => (Some(watch), Work::new(&home)),
                        Err(_) => {
                            let mut work = Work::new(&home);
                            work.failed(
                                "Native source watcher could not start. Save Settings to retry."
                                    .into(),
                            );
                            (None, work)
                        }
                    }
                }
                None => {
                    let mut work = Work::without_sources();
                    work.failed(
                        "Codex home could not be discovered. Set a directory in Settings.".into(),
                    );
                    (None, work)
                }
            }
        }
        Err(_) => {
            let mut work = Work::without_sources();
            work.failed("Saved settings could not be loaded. Save Settings to retry.".into());
            if let Ok((view, _)) = settings::resolve(Config::default()) {
                publish(app, view);
            }
            (None, work)
        }
    }
}

pub(crate) fn apply(
    mut configuration: Config,
    store: &mut Store,
    work: &mut Work,
    native: &mut Option<NativeWatch>,
    control: &pricing::Control,
) -> Result<View, Error> {
    if let Some(path) = configuration.codex_directory_override.as_deref() {
        configuration.codex_directory_override = Some(settings::validate_override(path)?);
    }
    configuration.validate()?;
    let (view, home) = settings::resolve(configuration.clone())?;
    let roots = home
        .as_ref()
        .map(|home| vec![home.join("sessions"), home.join("archived_sessions")])
        .unwrap_or_default();
    let switching = roots != work.roots || (home.is_some() && native.is_none());
    // Prepare a complete replacement before committing. A failed validation,
    // watcher registration, or database write leaves the old monitor intact.
    let replacement = if switching {
        home.as_deref()
            .map(|home| subscribe(home, control))
            .transpose()?
    } else {
        None
    };
    store
        .save_tracker_settings(&configuration)
        .map_err(|_| Error::storage())?;
    if switching {
        *native = replacement;
        *work = home
            .as_deref()
            .map(Work::new)
            .unwrap_or_else(Work::without_sources);
    }
    Ok(view)
}

#[tauri::command]
pub fn tracker_settings(runtime: tauri::State<'_, Runtime>) -> Result<View, Error> {
    runtime
        .0
        .lock()
        .map_err(|_| Error::unavailable())?
        .clone()
        .ok_or_else(Error::unavailable)
}

#[tauri::command]
pub async fn save_tracker_settings(
    control: tauri::State<'_, pricing::Control>,
    settings: Config,
) -> Result<View, Error> {
    let control = control.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (reply, receive) = mpsc::channel();
        control
            .0
            .try_send(pricing::Message::Settings(Request { settings, reply }))
            .map_err(|error| match error {
                mpsc::TrySendError::Full(_) => Error::busy(),
                mpsc::TrySendError::Disconnected(_) => Error::unavailable(),
            })?;
        receive.recv().map_err(|_| Error::unavailable())?
    })
    .await
    .map_err(|_| Error::unavailable())?
}

#[tauri::command]
pub async fn tracker_diagnostics(app: tauri::AppHandle) -> Result<Diagnostics, Error> {
    let path = app
        .path()
        .app_data_dir()
        .map_err(|_| Error::storage())?
        .join("usage.sqlite");
    let mut result = tauri::async_runtime::spawn_blocking(move || Store::read_diagnostics(&path))
        .await
        .map_err(|_| Error::storage())??;
    let runtime = app.state::<Runtime>();
    let current = runtime.0.lock().map_err(|_| Error::unavailable())?;
    result.monitored_directory = current
        .as_ref()
        .and_then(|view| view.monitored_directory.clone());
    let snapshot = app.state::<State>();
    let snapshot = snapshot.0.lock().map_err(|_| Error::unavailable())?;
    result.source_available = snapshot.source_available;
    result.error = snapshot.diagnostic.clone();
    Ok(result)
}

#[tauri::command]
pub async fn export_usage_csv(
    app: tauri::AppHandle,
    exports: tauri::State<'_, ExportState>,
    request: crate::export::Request,
) -> Result<crate::export::Outcome, crate::export::Error> {
    let path = app
        .path()
        .app_data_dir()
        .map_err(|_| crate::export::Error::Storage)?
        .join("usage.sqlite");
    exports
        .0
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| crate::export::Error::Busy)?;
    let permit = ExportPermit(exports.0.clone());
    tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        crate::export::export_csv(&path, request)
    })
    .await
    .map_err(|_| crate::export::Error::Storage)?
}

#[cfg(test)]
mod tests;
