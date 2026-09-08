use super::*;
use std::cell::Cell;

#[derive(Default)]
struct FakePlatform {
    autostart: Cell<bool>,
    tray: Cell<bool>,
    reject_autostart: Cell<bool>,
}

impl Platform for FakePlatform {
    fn autostart_enabled(&self) -> Result<bool, Error> {
        Ok(self.autostart.get())
    }
    fn set_autostart(&self, enabled: bool) -> Result<(), Error> {
        if self.reject_autostart.get() {
            return Err(Error::new("autostart", "Registration failed"));
        }
        self.autostart.set(enabled);
        Ok(())
    }
    fn tray_enabled(&self) -> bool {
        self.tray.get()
    }
    fn set_tray(&self, enabled: bool) -> Result<(), Error> {
        self.tray.set(enabled);
        Ok(())
    }
}

#[test]
fn preferences_activate_opt_in_and_disable_it_after_reopening_saved_settings() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("usage.sqlite");
    let platform = FakePlatform::default();
    let mut store = crate::storage::Store::open(&path).unwrap();
    let config = Config {
        autostart: true,
        tray_enabled: true,
        close_to_tray: true,
        ..Config::default()
    };
    save_preferences(&platform, &config, || {
        store
            .save_tracker_settings(&config)
            .map_err(|_| Error::storage())
    })
    .unwrap();
    assert!(platform.autostart.get());
    assert!(platform.tray.get());
    drop(store);
    let mut store = crate::storage::Store::open(&path).unwrap();
    assert_eq!(store.tracker_settings().unwrap(), config);
    save_preferences(&platform, &Config::default(), || {
        store
            .save_tracker_settings(&Config::default())
            .map_err(|_| Error::storage())
    })
    .unwrap();
    assert!(!platform.autostart.get());
    assert!(!platform.tray.get());
    assert_eq!(store.tracker_settings().unwrap(), Config::default());
}

#[test]
fn failed_registration_or_save_restores_platform_and_preserves_preferences() {
    let platform = FakePlatform::default();
    let config = Config {
        autostart: true,
        tray_enabled: true,
        ..Config::default()
    };
    let saved = Cell::new(false);
    platform.reject_autostart.set(true);
    assert_eq!(
        save_preferences(&platform, &config, || {
            saved.set(true);
            Ok(())
        })
        .unwrap_err()
        .code,
        "autostart"
    );
    assert!(!saved.get());
    assert!(!platform.tray.get());
    assert!(!platform.autostart.get());
    platform.reject_autostart.set(false);
    assert_eq!(
        save_preferences(&platform, &config, || Err(Error::storage()))
            .unwrap_err()
            .code,
        "storage"
    );
    assert!(!platform.tray.get());
    assert!(!platform.autostart.get());
}
