//! Desktop preferences and tray presentation; ingestion remains platform independent.
use crate::settings::{Config, Error};

mod native;
mod presentation;
pub(crate) use native::{refresh, window_event, NativePlatform, Runtime};

pub(crate) trait Platform {
    fn autostart_enabled(&self) -> Result<bool, Error>;
    fn set_autostart(&self, enabled: bool) -> Result<(), Error>;
    fn tray_enabled(&self) -> bool;
    fn set_tray(&self, enabled: bool) -> Result<(), Error>;
}

pub(crate) fn save_preferences(
    platform: &impl Platform,
    configuration: &Config,
    save: impl FnOnce() -> Result<(), Error>,
) -> Result<(), Error> {
    configuration.validate()?;
    let old_autostart = platform.autostart_enabled()?;
    let old_tray = platform.tray_enabled();
    let result = (|| {
        if old_tray != configuration.tray_enabled {
            platform.set_tray(configuration.tray_enabled)?;
        }
        if old_autostart != configuration.autostart {
            platform.set_autostart(configuration.autostart)?;
        }
        save()
    })();
    if let Err(error) = result {
        let restored_startup = platform.autostart_enabled().and_then(|enabled| {
            if enabled != old_autostart {
                platform.set_autostart(old_autostart)
            } else {
                Ok(())
            }
        });
        let restored_tray = if platform.tray_enabled() != old_tray {
            platform.set_tray(old_tray)
        } else {
            Ok(())
        };
        if restored_startup.is_err() || restored_tray.is_err() {
            return Err(Error::new("desktop_restore", "Settings were not saved and desktop preferences could not be restored. Open Settings and save again to reconcile them."));
        }
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
