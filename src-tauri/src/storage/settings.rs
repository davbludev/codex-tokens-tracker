use super::{Result, Store};
use crate::settings::{Config, Diagnostics, Error};
use rusqlite::{params, Connection, OpenFlags, Transaction};
use std::{
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

impl Store {
    pub fn tracker_settings(&self) -> Result<Config> {
        Ok(self.connection.query_row(
            "SELECT codex_directory_override,autostart,tray_enabled,close_to_tray FROM tracker_settings WHERE id=1",
            [],
            |row| Ok(Config {
                codex_directory_override: row.get(0)?,
                autostart: row.get(1)?,
                tray_enabled: row.get(2)?,
                close_to_tray: row.get(3)?,
            }),
        )?)
    }

    pub fn save_tracker_settings(&mut self, config: &Config) -> Result<()> {
        config.validate()?;
        self.connection.execute(
            "UPDATE tracker_settings SET codex_directory_override=?1,autostart=?2,tray_enabled=?3,close_to_tray=?4 WHERE id=1",
            params![config.codex_directory_override, config.autostart, config.tray_enabled, config.close_to_tray],
        )?;
        Ok(())
    }

    /// Counts and durable progress are observed from one read snapshot, off the writer.
    pub fn read_diagnostics(path: &Path) -> std::result::Result<Diagnostics, Error> {
        let mut connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|_| Error::storage())?;
        connection
            .busy_timeout(std::time::Duration::from_secs(3))
            .map_err(|_| Error::storage())?;
        let tx = connection.transaction().map_err(|_| Error::storage())?;
        let mut diagnostics = tx.query_row(
            "SELECT (SELECT COUNT(*) FROM sessions WHERE is_placeholder=0),(SELECT COUNT(*) FROM observations),last_successful_ingestion_at_ms FROM ingestion_status WHERE id=1",
            [],
            |row| Ok(Diagnostics {
                tracked_session_count: super::unsigned(row, 0)?,
                usage_record_count: super::unsigned(row, 1)?,
                last_successful_ingestion_at_ms: row.get(2)?,
                ..Diagnostics::default()
            }),
        ).map_err(|_| Error::storage())?;
        diagnostics.database_path =
            crate::source::normalized_path(&fs::canonicalize(path).map_err(|_| Error::storage())?)
                .to_string_lossy()
                .into_owned();
        diagnostics.database_size_bytes = fs::metadata(path).map_err(|_| Error::storage())?.len();
        let mut wal_path = path.as_os_str().to_os_string();
        wal_path.push("-wal");
        match fs::metadata(wal_path) {
            Ok(metadata) => {
                diagnostics.database_size_bytes = diagnostics
                    .database_size_bytes
                    .checked_add(metadata.len())
                    .ok_or_else(Error::storage)?
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(_) => return Err(Error::storage()),
        }
        Ok(diagnostics)
    }
}

pub(super) fn record_ingestion(tx: &Transaction<'_>) -> Result<()> {
    let at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| super::Error::RecoveryMetadata)?;
    let milliseconds = i64::try_from(at.as_millis()).map_err(|_| super::Error::Offset)?;
    tx.execute(
        "UPDATE ingestion_status SET last_successful_ingestion_at_ms=MAX(COALESCE(last_successful_ingestion_at_ms,?1),?1) WHERE id=1",
        [milliseconds],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source;
    use std::io::Write;

    #[test]
    fn settings_defaults_and_atomic_preferences_survive_restart() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("settings.sqlite");
        let mut store = Store::open(&path).unwrap();
        assert_eq!(store.tracker_settings().unwrap(), Config::default());
        fs::create_dir(temp.path().join("sessions")).unwrap();
        let config = Config {
            codex_directory_override: Some(
                crate::settings::validate_override(temp.path().to_str().unwrap()).unwrap(),
            ),
            autostart: true,
            tray_enabled: true,
            close_to_tray: true,
        };
        store.save_tracker_settings(&config).unwrap();
        let invalid = Config {
            tray_enabled: false,
            ..config.clone()
        };
        assert!(store.save_tracker_settings(&invalid).is_err());
        assert_eq!(store.tracker_settings().unwrap(), config);
        drop(store);
        let mut store = Store::open(&path).unwrap();
        assert_eq!(store.tracker_settings().unwrap(), config);
        store.save_tracker_settings(&Config::default()).unwrap();
        drop(store);
        assert_eq!(
            Store::open(&path).unwrap().tracker_settings().unwrap(),
            Config::default()
        );
    }

    #[test]
    fn diagnostics_follow_committed_ingestion_and_resume_without_double_counting() {
        let temp = tempfile::tempdir().unwrap();
        let db = temp.path().join("diagnostics.sqlite");
        let log = temp.path().join("rollout-diagnostics.jsonl");
        let mut store = Store::open(&db).unwrap();
        let empty = Store::read_diagnostics(&db).unwrap();
        assert_eq!(
            (empty.tracked_session_count, empty.usage_record_count),
            (0, 0)
        );
        assert_eq!(empty.last_successful_ingestion_at_ms, None);
        let mut fixture = include_str!("../../../fixtures/codex/active-root.jsonl").lines();
        let mut metadata: serde_json::Value =
            serde_json::from_str(fixture.next().unwrap()).unwrap();
        metadata["payload"]["parent_thread_id"] = "missing-parent".into();
        let metadata = format!("{metadata}\n");
        let usage = format!("{}\n", fixture.collect::<Vec<_>>().join("\n"));
        // Consume only metadata first; incomplete bytes must not mark ingestion.
        fs::write(&log, &metadata[..metadata.len() - 1]).unwrap();
        source::ingest(&mut store, &log).unwrap();
        assert_eq!(
            Store::read_diagnostics(&db)
                .unwrap()
                .last_successful_ingestion_at_ms,
            None
        );
        fs::OpenOptions::new()
            .append(true)
            .open(&log)
            .unwrap()
            .write_all(b"\n")
            .unwrap();
        source::ingest(&mut store, &log).unwrap();
        let observed = Store::read_diagnostics(&db).unwrap();
        assert_eq!(observed.tracked_session_count, 1);
        assert_eq!(observed.usage_record_count, 0);
        assert!(observed.last_successful_ingestion_at_ms.is_some());
        assert!(observed.database_size_bytes >= fs::metadata(&db).unwrap().len());
        // A fixed durable timestamp makes accidental updates during idle scans visible.
        store
            .connection
            .execute(
                "UPDATE ingestion_status SET last_successful_ingestion_at_ms=123",
                [],
            )
            .unwrap();
        drop(store);
        let mut store = Store::open(&db).unwrap();
        source::ingest(&mut store, &log).unwrap();
        let resumed = Store::read_diagnostics(&db).unwrap();
        assert_eq!(resumed.last_successful_ingestion_at_ms, Some(123));
        assert_eq!(resumed.tracked_session_count, 1);
        assert_eq!(resumed.usage_record_count, 0);
        fs::OpenOptions::new()
            .append(true)
            .open(&log)
            .unwrap()
            .write_all(usage.as_bytes())
            .unwrap();
        source::ingest(&mut store, &log).unwrap();
        let imported = Store::read_diagnostics(&db).unwrap();
        assert_eq!(imported.tracked_session_count, 1);
        assert_eq!(imported.usage_record_count, 1);
        assert!(imported.last_successful_ingestion_at_ms.unwrap() > 123);
        drop(store);
        let mut store = Store::open(&db).unwrap();
        source::ingest(&mut store, &log).unwrap();
        let resumed = Store::read_diagnostics(&db).unwrap();
        assert_eq!(
            resumed.last_successful_ingestion_at_ms,
            imported.last_successful_ingestion_at_ms
        );
        assert_eq!(resumed.usage_record_count, 1);
        assert_eq!(
            store.snapshot().unwrap().direct_tokens.as_deref(),
            Some("26587")
        );
    }
}
