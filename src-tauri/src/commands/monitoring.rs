//! Session-only monitoring requests applied between atomic writer batches.
use super::{pricing, runtime::Work};
use crate::storage::{Result, Store};
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};
use tauri::Manager;

#[derive(Default)]
pub struct Runtime {
    paused: AtomicBool,
    resume_requested: AtomicBool,
    stopping: AtomicBool,
    finished: AtomicBool,
    exit_waiting: AtomicBool,
}

impl Runtime {
    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Acquire)
    }

    pub fn set_paused(&self, paused: bool, control: &pricing::Control) {
        if self.is_stopping() || self.is_finished() {
            return;
        }
        if self.paused.swap(paused, Ordering::AcqRel) && !paused {
            self.resume_requested.store(true, Ordering::Release);
        }
        // A full inbox already wakes the writer, which reads these flags first.
        let _ = control.0.try_send(pricing::Message::Wake);
    }

    pub fn is_stopping(&self) -> bool {
        self.stopping.load(Ordering::Acquire)
    }

    pub fn is_finished(&self) -> bool {
        self.finished.load(Ordering::Acquire)
    }

    pub fn request_exit(&self, control: &pricing::Control) {
        self.stopping.store(true, Ordering::Release);
        let _ = control.0.try_send(pricing::Message::Wake);
    }

    /// Called only after the worker has released its writer connection, or failed to open it.
    pub fn mark_finished(&self) {
        self.finished.store(true, Ordering::Release);
    }

    pub fn step(&self, work: &mut Work, store: &mut Store, now: Instant) -> Result<bool> {
        if self.is_stopping() {
            work.set_paused(true);
            return Ok(false);
        }
        let mut changed = work.set_paused(self.is_paused());
        if self.resume_requested.swap(false, Ordering::AcqRel) {
            work.recover("Monitoring resumed; recovering available files");
            changed = true;
        }
        Ok(work.step(store, now)? || changed)
    }
}

pub fn set_paused(app: &tauri::AppHandle, paused: bool) {
    app.state::<Runtime>()
        .set_paused(paused, &app.state::<pricing::Control>());
}

pub fn request_exit(app: &tauri::AppHandle) {
    let runtime = app.state::<Runtime>();
    let exports = app.state::<super::settings::ExportState>().inner().clone();
    exports.begin_shutdown();
    runtime.request_exit(&app.state::<pricing::Control>());
    if runtime.is_finished() {
        if exports.is_idle() {
            app.exit(0);
        } else if !runtime.exit_waiting.swap(true, Ordering::AcqRel) {
            let app = app.clone();
            std::thread::spawn(move || {
                exports.wait_for_idle();
                app.exit(0);
            });
        }
    }
}

#[cfg(test)]
mod tests;
