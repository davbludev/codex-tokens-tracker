use super::{presentation::Summary, Platform};
use crate::{
    commands::{self, monitoring},
    settings::Error,
    storage::Store,
    weekly,
};
use std::{path::PathBuf, sync::Mutex, time::Duration};
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager,
};
use tauri_plugin_autostart::ManagerExt;

const TRAY_ID: &str = "usage-monitor";

#[derive(Default)]
pub(crate) struct Runtime {
    menu: Mutex<Option<TrayMenu>>,
    refresh: Mutex<Refresh>,
    pub(crate) diagnostic: Mutex<Option<String>>,
}

#[derive(Default)]
struct Refresh {
    running: bool,
    dirty: bool,
}

#[derive(Clone)]
struct TrayMenu {
    metrics: Vec<MenuItem<tauri::Wry>>,
    pause: MenuItem<tauri::Wry>,
    status: MenuItem<tauri::Wry>,
}

pub(crate) struct NativePlatform {
    app: tauri::AppHandle,
}

impl NativePlatform {
    pub(crate) fn new(app: tauri::AppHandle) -> Self {
        Self { app }
    }
}

fn platform_error(action: &str) -> Error {
    Error::new(
        "desktop",
        format!("Could not {action}. Check desktop permissions and save Settings to retry."),
    )
}

fn open(app: &tauri::AppHandle) -> Result<(), Error> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| platform_error("open the main window"))?;
    window
        .unminimize()
        .and_then(|_| window.show())
        .and_then(|_| window.set_focus())
        .map_err(|_| platform_error("open the main window"))
}

fn report(app: &tauri::AppHandle, error: Error) {
    if let Ok(mut diagnostic) = app.state::<Runtime>().diagnostic.lock() {
        *diagnostic = Some(error.message.clone());
    }
    if let Ok(mut snapshot) = app.state::<commands::State>().0.lock() {
        snapshot.diagnostic = Some(error.message.clone());
    }
    let menu = app
        .state::<Runtime>()
        .menu
        .lock()
        .ok()
        .and_then(|menu| menu.clone());
    if let Some(menu) = menu {
        let _ = menu.status.set_text(error.message);
    }
}

impl Platform for NativePlatform {
    fn autostart_enabled(&self) -> Result<bool, Error> {
        self.app
            .autolaunch()
            .is_enabled()
            .map_err(|_| platform_error("read startup registration"))
    }
    fn set_autostart(&self, enabled: bool) -> Result<(), Error> {
        let result = if enabled {
            self.app.autolaunch().enable()
        } else {
            self.app.autolaunch().disable()
        };
        result.map_err(|_| platform_error("update startup registration"))
    }
    fn tray_enabled(&self) -> bool {
        self.app.tray_by_id(TRAY_ID).is_some()
    }
    fn set_tray(&self, enabled: bool) -> Result<(), Error> {
        if enabled == self.tray_enabled() {
            return Ok(());
        }
        if !enabled {
            // Removing the only way back to a hidden window must first reveal it.
            open(&self.app)?;
            self.app.remove_tray_by_id(TRAY_ID);
            *self
                .app
                .state::<Runtime>()
                .menu
                .lock()
                .map_err(|_| Error::unavailable())? = None;
            return Ok(());
        }
        let summary = Summary::unavailable();
        let metrics: Vec<_> = summary
            .rows
            .iter()
            .enumerate()
            .map(|(index, text)| {
                MenuItem::with_id(
                    &self.app,
                    format!("metric-{index}"),
                    text,
                    false,
                    None::<&str>,
                )
            })
            .collect::<tauri::Result<_>>()
            .map_err(|_| platform_error("create tray metrics"))?;
        let menu = Menu::new(&self.app).map_err(|_| platform_error("create the tray menu"))?;
        for item in &metrics {
            menu.append(item)
                .map_err(|_| platform_error("create the tray menu"))?;
        }
        let status = MenuItem::with_id(
            &self.app,
            "monitor-status",
            "Monitoring local usage",
            false,
            None::<&str>,
        )
        .map_err(|_| platform_error("create tray status"))?;
        let open_item = MenuItem::with_id(&self.app, "open", "Open", true, None::<&str>)
            .map_err(|_| platform_error("create tray actions"))?;
        let paused = self.app.state::<monitoring::Runtime>().is_paused();
        let pause = MenuItem::with_id(
            &self.app,
            "pause",
            if paused {
                "Resume monitoring"
            } else {
                "Pause monitoring"
            },
            true,
            None::<&str>,
        )
        .map_err(|_| platform_error("create tray actions"))?;
        let exit = MenuItem::with_id(&self.app, "exit", "Exit", true, None::<&str>)
            .map_err(|_| platform_error("create tray actions"))?;
        let separator = PredefinedMenuItem::separator(&self.app)
            .map_err(|_| platform_error("create the tray menu"))?;
        menu.append_items(&[&status, &separator, &open_item, &pause, &exit])
            .map_err(|_| platform_error("create the tray menu"))?;
        let icon = self
            .app
            .default_window_icon()
            .cloned()
            .ok_or_else(|| platform_error("load the tray icon"))?;
        TrayIconBuilder::with_id(TRAY_ID)
            .icon(icon)
            .tooltip(summary.tooltip)
            .menu(&menu)
            .show_menu_on_left_click(false)
            .on_menu_event(|app, event| match event.id.as_ref() {
                "open" => {
                    if let Err(error) = open(app) {
                        report(app, error);
                    }
                }
                "pause" => {
                    let paused = !app.state::<monitoring::Runtime>().is_paused();
                    monitoring::set_paused(app, paused);
                    update_status(app);
                }
                "exit" => {
                    monitoring::request_exit(app);
                    update_status(app);
                }
                _ => (),
            })
            .on_tray_icon_event(|tray, event| {
                if matches!(
                    event,
                    TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    }
                ) {
                    if let Err(error) = open(tray.app_handle()) {
                        report(tray.app_handle(), error);
                    }
                }
            })
            .build(&self.app)
            .map_err(|_| platform_error("create the system tray icon"))?;
        *self
            .app
            .state::<Runtime>()
            .menu
            .lock()
            .map_err(|_| Error::unavailable())? = Some(TrayMenu {
            metrics,
            pause,
            status,
        });
        update_status(&self.app);
        if let Ok(path) = self.app.path().app_data_dir() {
            refresh(&self.app, path.join("usage.sqlite"));
        }
        Ok(())
    }
}

pub(crate) fn window_event(window: &tauri::Window, event: &tauri::WindowEvent) {
    if window.label() != "main" {
        return;
    }
    let app = window.app_handle();
    let close_to_tray = app
        .state::<commands::settings::Runtime>()
        .0
        .lock()
        .ok()
        .and_then(|view| view.as_ref().map(|view| view.config.close_to_tray))
        .unwrap_or(false);
    let hide_to_tray = close_to_tray && app.tray_by_id(TRAY_ID).is_some();
    match event {
        tauri::WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            if hide_to_tray {
                if window.hide().is_err() {
                    report(app, platform_error("hide the main window"));
                }
            } else {
                monitoring::request_exit(app);
            }
        }
        // Windows sends a resize on minimization; ordinary resizing stays visible.
        tauri::WindowEvent::Resized(_)
            if hide_to_tray && window.is_minimized().unwrap_or(false) =>
        {
            if window.hide().is_err() {
                report(app, platform_error("minimize the main window to the tray"));
            }
        }
        _ => (),
    }
}

fn update_status(app: &tauri::AppHandle) {
    let menu = app
        .state::<Runtime>()
        .menu
        .lock()
        .ok()
        .and_then(|menu| menu.clone());
    if let Some(menu) = menu {
        let runtime = app.state::<monitoring::Runtime>();
        let paused = runtime.is_paused();
        let diagnostic = app
            .state::<commands::State>()
            .0
            .lock()
            .ok()
            .and_then(|snapshot| snapshot.diagnostic.clone());
        let _ = menu.pause.set_text(if paused {
            "Resume monitoring"
        } else {
            "Pause monitoring"
        });
        let status = if runtime.is_finished() {
            diagnostic.as_deref().unwrap_or("Monitoring stopped")
        } else if runtime.is_stopping() {
            "Finishing writes before exit…"
        } else if paused {
            "Monitoring paused · showing last observations"
        } else {
            diagnostic.as_deref().unwrap_or("Monitoring local usage")
        };
        let _ = menu.status.set_text(status);
    }
}

/// Coalesce snapshot publications into one read, with at most one trailing read.
/// No timer or database work remains when ingestion is idle or the tray is disabled.
pub(crate) fn refresh(app: &tauri::AppHandle, database: PathBuf) {
    if app.tray_by_id(TRAY_ID).is_none() {
        return;
    }
    let runtime = app.state::<Runtime>();
    let Ok(mut pending) = runtime.refresh.lock() else {
        return;
    };
    pending.dirty = true;
    if pending.running {
        return;
    }
    pending.running = true;
    let app = app.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(1));
        if let Ok(mut pending) = app.state::<Runtime>().refresh.lock() {
            pending.dirty = false;
        }
        let summary = Store::read_weekly(
            &database,
            weekly::Query {
                before: None,
                limit: 1,
            },
        )
        .map(|weekly| Summary::from_weekly(&weekly))
        .unwrap_or_else(|_| Summary::unavailable());
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || {
            if let Some(tray) = handle.tray_by_id(TRAY_ID) {
                let _ = tray.set_tooltip(Some(summary.tooltip));
                let menu = handle
                    .state::<Runtime>()
                    .menu
                    .lock()
                    .ok()
                    .and_then(|menu| menu.clone());
                if let Some(menu) = menu {
                    for (item, text) in menu.metrics.iter().zip(summary.rows) {
                        let _ = item.set_text(text);
                    }
                }
                update_status(&handle);
            }
        });
        let runtime = app.state::<Runtime>();
        let Ok(mut pending) = runtime.refresh.lock() else {
            return;
        };
        if !pending.dirty || app.tray_by_id(TRAY_ID).is_none() {
            pending.running = false;
            return;
        }
    });
}
