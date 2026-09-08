//! Tracker preferences and the selected Codex source directory.
use crate::source;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub codex_directory_override: Option<String>,
    pub autostart: bool,
    pub tray_enabled: bool,
    pub close_to_tray: bool,
}

impl Config {
    pub fn validate(&self) -> Result<(), Error> {
        if self.close_to_tray && !self.tray_enabled {
            return Err(Error::new(
                "invalid_settings",
                "Enable the tray before choosing close to tray.",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct View {
    #[serde(flatten)]
    pub config: Config,
    pub automatic_directory: Option<String>,
    pub monitored_directory: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Diagnostics {
    pub database_path: String,
    pub database_size_bytes: u64,
    pub tracked_session_count: u64,
    /// All normalized usage observations, including pending or rejected usage.
    pub usage_record_count: u64,
    pub monitored_directory: Option<String>,
    pub last_successful_ingestion_at_ms: Option<i64>,
    pub source_available: bool,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, thiserror::Error)]
#[error("{message}")]
pub struct Error {
    pub code: &'static str,
    pub message: String,
}

impl Error {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn storage() -> Self {
        Self::new(
            "storage",
            "Tracker settings or diagnostics could not be read or saved.",
        )
    }

    pub fn unavailable() -> Self {
        Self::new(
            "unavailable",
            "The tracker is not available yet. Try again shortly.",
        )
    }

    pub fn busy() -> Self {
        Self::new("busy", "The tracker is busy. Try again shortly.")
    }
}

/// Reuse source discovery, whose public path points at the active sessions tree.
pub fn automatic_directory() -> Option<PathBuf> {
    source::sessions_directory().and_then(|path| path.parent().map(normalize_available))
}

/// Persisted paths remain usable when their directory is temporarily unavailable.
/// New overrides must pass `validate_override` before this value is saved.
pub fn resolve(config: Config) -> Result<(View, Option<PathBuf>), Error> {
    config.validate()?;
    let automatic = automatic_directory();
    let monitored = match config.codex_directory_override.as_deref() {
        Some(value) => {
            let path = Path::new(value);
            if !path.is_absolute() {
                return Err(invalid_directory(
                    "Choose an absolute Codex directory path.",
                ));
            }
            Some(normalize_available(path))
        }
        None => automatic.clone(),
    };
    let view = View {
        config,
        automatic_directory: automatic.as_deref().map(path_string),
        monitored_directory: monitored.as_deref().map(path_string),
    };
    Ok((view, monitored))
}

pub fn validate_override(value: &str) -> Result<String, Error> {
    let path = Path::new(value);
    if !path.is_absolute() {
        return Err(invalid_directory(
            "Choose an absolute Codex directory path.",
        ));
    }
    let path = fs::canonicalize(path).map_err(|_| {
        invalid_directory("The Codex directory does not exist or cannot be accessed.")
    })?;
    if !directory_available(&path) {
        return Err(invalid_directory(
            "Choose a readable Codex directory containing sessions or archived_sessions.",
        ));
    }
    Ok(path_string(&source::normalized_path(&path)))
}

pub fn directory_available(path: &Path) -> bool {
    fs::read_dir(path).is_ok()
        && ["sessions", "archived_sessions"].iter().any(|name| {
            let child = path.join(name);
            fs::symlink_metadata(&child)
                .is_ok_and(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
                && fs::read_dir(child).is_ok()
        })
}

fn normalize_available(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().map_or_else(|_| path.to_path_buf(), |cwd| cwd.join(path))
    };
    source::normalized_path(&fs::canonicalize(&absolute).unwrap_or(absolute))
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn invalid_directory(message: &str) -> Error {
    Error::new("invalid_directory", message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn override_requires_readable_source_tree_and_normalizes_the_codex_home() {
        let temp = tempfile::tempdir().unwrap();
        assert_eq!(
            validate_override("relative/path").unwrap_err().code,
            "invalid_directory"
        );
        assert!(validate_override(temp.path().to_str().unwrap()).is_err());
        let file = temp.path().join("file");
        fs::write(&file, "").unwrap();
        assert!(validate_override(file.to_str().unwrap()).is_err());
        fs::create_dir(temp.path().join("archived_sessions")).unwrap();
        fs::create_dir(temp.path().join("child")).unwrap();
        let normalized =
            validate_override(temp.path().join("child").join("..").to_str().unwrap()).unwrap();
        assert_eq!(
            normalized,
            path_string(&source::normalized_path(
                &fs::canonicalize(temp.path()).unwrap()
            ))
        );
        let config = Config {
            codex_directory_override: Some(normalized.clone()),
            ..Config::default()
        };
        let (view, monitored) = resolve(config).unwrap();
        assert_eq!(
            view.monitored_directory.as_deref(),
            Some(normalized.as_str())
        );
        assert_eq!(monitored.unwrap(), PathBuf::from(&normalized));
    }

    #[test]
    fn saved_override_survives_temporary_source_disappearance() {
        let temp = tempfile::tempdir().unwrap();
        let missing = temp.path().join("offline-codex");
        let config = Config {
            codex_directory_override: Some(path_string(&missing)),
            ..Config::default()
        };
        let (_, monitored) = resolve(config).unwrap();
        assert_eq!(monitored, Some(missing));
    }
}
