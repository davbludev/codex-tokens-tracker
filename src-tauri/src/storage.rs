use crate::{
    accounting,
    adapter::{self, Record, Usage},
    identity, identity_filesystem,
};
pub(crate) mod aggregates;
mod dashboard;
mod hierarchy;
pub(crate) mod pricing;
pub(crate) mod retention;
mod settings;
pub(crate) mod weekly;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Local database operation failed")]
    Database(#[from] rusqlite::Error),
    #[error("Normalized metadata could not be encoded")]
    Encoding(#[from] serde_json::Error),
    #[error("Source could not be read")]
    Io(#[from] std::io::Error),
    #[error("Source checkpoint exceeds supported range")]
    Offset,
    #[error("Source batch no longer matches its checkpoint")]
    StaleBatch,
    #[error("Invalid source recovery metadata")]
    RecoveryMetadata,
    #[error("Database schema is newer than this application")]
    Schema,
    #[error(transparent)]
    Pricing(#[from] crate::pricing::Error),
    #[error(transparent)]
    Settings(#[from] crate::settings::Error),
}
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Default, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub thread_id: Option<String>,
    pub direct_tokens: Option<String>,
    pub observed_at: Option<String>,
    pub coverage: String,
    pub diagnostic: Option<String>,
    pub source_available: bool,
}
pub struct Store {
    connection: Connection,
}

/// Only boundaries and a fixed-size digest are durable; never JSONL tail bytes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SourceProgress {
    pub offset: u64,
    pub ordinal: i64,
    pub known_size: u64,
    pub tail_length: u64,
    pub tail_discarding: bool,
    pub verification_start: u64,
    pub verification_length: u32,
    pub verification_hash: Option<[u8; 32]>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceState {
    pub generation: i64,
    pub identity: Option<String>,
    pub progress: SourceProgress,
}

pub struct InputLine {
    pub start: u64,
    pub end: u64,
    pub ordinal: i64,
    pub record: std::result::Result<Record, &'static str>,
}

pub const MAX_BATCH_LINES: usize = 64;
const PROMOTION_LIMIT: usize = 32;

impl Store {
    /// Flush committed WAL pages and release the sole writer without draining durable jobs.
    pub fn close(self) -> Result<()> {
        let checkpoint = self
            .connection
            .execute_batch("PRAGMA wal_checkpoint(PASSIVE)");
        let close = self.connection.close().map_err(|(_, error)| error);
        checkpoint?;
        close?;
        Ok(())
    }

    #[cfg(test)]
    pub fn connection(&self) -> &Connection {
        &self.connection
    }
    pub fn open(path: &Path) -> Result<Self> {
        let mut connection = Connection::open(path)?;
        connection.busy_timeout(std::time::Duration::from_secs(3))?;
        let version: i64 = connection.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version > 10 {
            return Err(Error::Schema);
        }
        if version == 0 {
            connection.execute_batch(include_str!("../migrations/001_initial.sql"))?;
        }
        if version < 2 {
            let tx = connection.transaction()?;
            tx.execute_batch(include_str!("../migrations/002_resumable_sources.sql"))?;
            // Stream the old normalized rows; migration never reads original logs.
            {
                let mut query = tx.prepare("SELECT id,timestamp,normalized FROM observations")?;
                let mut rows = query.query([])?;
                while let Some(row) = rows.next()? {
                    let id: i64 = row.get(0)?;
                    let timestamp: String = row.get(1)?;
                    let usage: Usage = serde_json::from_str(&row.get::<_, String>(2)?)?;
                    let parsed = adapter::observation_time(&timestamp).ok();
                    tx.execute("UPDATE observations SET time_seconds=?,time_nanos=?,endpoint_order=?,start_order=? WHERE id=?", params![parsed.map(|t| t.0), parsed.map(|t| t.1),accounting::endpoint_key(&usage.thread_token_usage),accounting::start_key(&usage.usage,&usage.thread_token_usage), id])?;
                    // #3 recorded subsequent normalized usage as suppressed after
                    // a gap. Recover only independently valid, matching-thread rows
                    // with evidence of that specific earlier halt in this source.
                    if parsed.is_some()
                        && accounting::reconcile(&usage.usage, &usage.thread_token_usage, None)
                            .is_ok()
                    {
                        tx.execute("UPDATE observations AS candidate SET state='pending',diagnostic=? WHERE id=? AND accepted=0 AND diagnostic='Source accounting stopped after an unsupported record' AND EXISTS(SELECT 1 FROM sources s WHERE s.path=candidate.source_path AND s.thread_id=candidate.thread_id AND s.diagnostic='Unexplained thread gap; remaining source usage unavailable') AND EXISTS(SELECT 1 FROM observations gap WHERE gap.source_path=candidate.source_path AND gap.thread_id=candidate.thread_id AND gap.source_ordinal<candidate.source_ordinal AND gap.diagnostic='Unexplained thread gap; remaining source usage unavailable')",params![GAP,id])?;
                    }
                }
            }
            tx.execute("UPDATE observations SET state='pending',diagnostic=? WHERE accepted=0 AND diagnostic='Unexplained thread gap; remaining source usage unavailable'", [GAP])?;
            tx.execute("UPDATE sources SET halted=0,diagnostic=? WHERE diagnostic='Unexplained thread gap; remaining source usage unavailable'", [GAP])?;
            tx.execute("INSERT INTO reconciliation_work(observation_id) SELECT id FROM observations WHERE state='pending'", [])?;
            tx.commit()?;
        }
        if version < 3 {
            let tx = connection.transaction()?;
            tx.execute_batch(include_str!("../migrations/003_snapshot_chronology.sql"))?;
            tx.commit()?;
        }
        if version < 4 {
            let tx = connection.transaction()?;
            tx.execute_batch(include_str!("../migrations/004_metadata_evidence.sql"))?;
            tx.commit()?;
        }
        if version < 5 {
            let tx = connection.transaction()?;
            tx.execute_batch(include_str!("../migrations/005_identity_resolution.sql"))?;
            tx.commit()?;
        }
        if version < 6 {
            let tx = connection.transaction()?;
            tx.execute_batch(include_str!("../migrations/006_aggregate_queries.sql"))?;
            tx.commit()?;
        }
        if version < 7 {
            let tx = connection.transaction()?;
            tx.execute_batch(include_str!("../migrations/007_model_pricing.sql"))?;
            tx.commit()?;
        }
        if version < 8 {
            let tx = connection.transaction()?;
            tx.execute_batch(include_str!("../migrations/008_tracker_settings.sql"))?;
            tx.commit()?;
        }
        if version < 9 {
            let tx = connection.transaction()?;
            tx.execute_batch(include_str!("../migrations/009_model_price_backfills.sql"))?;
            tx.commit()?;
        }
        if version < 10 {
            let tx = connection.transaction()?;
            tx.execute_batch(include_str!("../migrations/010_retention.sql"))?;
            tx.commit()?;
        }
        Ok(Self { connection })
    }

    pub fn source_state(&self, path: &str) -> Result<SourceState> {
        self.checkpoint(path)?;
        Ok(self.connection.query_row("SELECT generation,identity,offset,ordinal,known_size,tail_length,tail_discarding,verification_start,verification_length,verification_hash FROM sources WHERE path=?", [path], |r| {
            let digest: Option<Vec<u8>> = r.get(9)?;
            let digest = digest.map(|v| v.try_into().map_err(|_| rusqlite::Error::InvalidQuery)).transpose()?;
            Ok(SourceState { generation: r.get(0)?, identity: r.get(1)?, progress: SourceProgress {
                offset: unsigned(r,2)?, ordinal: r.get(3)?, known_size: unsigned(r,4)?, tail_length: unsigned(r,5)?, tail_discarding: r.get(6)?, verification_start: unsigned(r,7)?, verification_length: r.get(8)?, verification_hash: digest,
            }})
        })?)
    }

    /// Capture an upper bound once so live inserts cannot prolong a recovery pass.
    pub fn source_watermark(&self) -> Result<Option<String>> {
        Ok(self
            .connection
            .query_row("SELECT MAX(path) FROM sources", [], |row| row.get(0))?)
    }

    /// Keyset recovery metadata only; no offsets into a growing table or raw data.
    pub fn source_page(&self, after: Option<&str>, through: &str) -> Result<Vec<(String, i64)>> {
        let mut query = self.connection.prepare(if after.is_some() {
            "SELECT path,generation FROM sources WHERE path>?1 AND path<=?2 ORDER BY path LIMIT 64"
        } else {
            "SELECT path,generation FROM sources WHERE path<=?2 ORDER BY path LIMIT 64"
        })?;
        let rows = query.query_map(params![after, through], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Bind a new source without changing its generation or any accounting state.
    pub fn initialize_source_identity(
        &self,
        path: &str,
        generation: i64,
        identity: &str,
        known_size: u64,
    ) -> Result<()> {
        if identity.is_empty() || identity.len() > 256 {
            return Err(Error::RecoveryMetadata);
        }
        let size = i64::try_from(known_size).map_err(|_| Error::Offset)?;
        let changed = self.connection.execute("UPDATE sources SET identity=?,known_size=? WHERE path=? AND generation=? AND identity IS NULL AND offset=0 AND ordinal=0 AND tail_length=0 AND tail_discarding=0 AND verification_start=0 AND verification_length=0 AND verification_hash IS NULL AND NOT EXISTS(SELECT 1 FROM observations WHERE source_path=? AND source_generation=?)", params![identity,size,path,generation,path,generation])?;
        if changed != 1 {
            return Err(Error::StaleBatch);
        }
        Ok(())
    }

    /// Call only after replacement, truncation, or verification failure is established.
    /// Observations retain their original path/generation provenance and accepted totals.
    pub fn restart_source(
        &mut self,
        path: &str,
        expected_generation: i64,
        identity: Option<&str>,
        known_size: u64,
    ) -> Result<()> {
        if identity.is_some_and(|value| value.is_empty() || value.len() > 256) {
            return Err(Error::RecoveryMetadata);
        }
        let size = i64::try_from(known_size).map_err(|_| Error::Offset)?;
        self.checkpoint(path)?;
        let changed = self.connection.execute("UPDATE sources SET generation=generation+1,identity=?,offset=0,ordinal=0,known_size=?,tail_length=0,tail_discarding=0,verification_start=0,verification_length=0,verification_hash=NULL,partial=0,thread_id=NULL,halted=0,legacy=0,diagnostic='Source generation changed; retained confirmed usage and will replay available records' WHERE path=? AND generation=?", params![identity,size,path,expected_generation])?;
        if changed != 1 {
            return Err(Error::StaleBatch);
        }
        Ok(())
    }

    pub fn source_removed(&self, path: &str, generation: i64) -> Result<()> {
        let changed = self.connection.execute("UPDATE sources SET tail_length=0,tail_discarding=0,partial=0,verification_start=0,verification_length=0,verification_hash=NULL,diagnostic='Source removed; confirmed usage retained, unfinished records unavailable' WHERE path=? AND generation=?", params![path,generation])?;
        if changed != 1 {
            return Err(Error::StaleBatch);
        }
        Ok(())
    }
    pub fn checkpoint(&self, path: &str) -> Result<(u64, i64)> {
        self.connection
            .execute("INSERT OR IGNORE INTO sources(path) VALUES(?)", [path])?;
        Ok(self.connection.query_row(
            "SELECT offset, ordinal FROM sources WHERE path=?",
            [path],
            |r| {
                let offset: i64 = r.get(0)?;
                let offset = u64::try_from(offset)
                    .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(0, offset))?;
                Ok((offset, r.get(1)?))
            },
        )?)
    }
    pub fn source_status(&self, path: &str, partial: bool, diagnostic: Option<&str>) -> Result<()> {
        self.connection.execute(
            "UPDATE sources SET partial=?, diagnostic=COALESCE(?, diagnostic) WHERE path=?",
            params![partial, diagnostic, path],
        )?;
        Ok(())
    }
    pub fn line(
        &mut self,
        path: &str,
        start: u64,
        end: u64,
        ordinal: i64,
        record: std::result::Result<Record, &'static str>,
    ) -> Result<()> {
        let state = self.source_state(path)?;
        let progress = SourceProgress {
            offset: end,
            ordinal,
            known_size: end.max(state.progress.known_size),
            ..Default::default()
        };
        self.batch(
            path,
            state.generation,
            vec![InputLine {
                start,
                end,
                ordinal,
                record,
            }],
            progress,
        )
    }

    /// Bounded complete-line work and its recovery checkpoint share one transaction.
    pub fn batch(
        &mut self,
        path: &str,
        generation: i64,
        lines: Vec<InputLine>,
        progress: SourceProgress,
    ) -> Result<()> {
        validate_progress(&progress)?;
        if lines.len() > MAX_BATCH_LINES {
            return Err(Error::RecoveryMetadata);
        }
        let consumed_lines = !lines.is_empty();
        let tx = self.connection.transaction()?;
        let (stored_generation, mut offset, mut ordinal): (i64, u64, i64) = tx.query_row(
            "SELECT generation,offset,ordinal FROM sources WHERE path=?",
            [path],
            |r| Ok((r.get(0)?, unsigned(r, 1)?, r.get(2)?)),
        )?;
        if stored_generation != generation {
            return Err(Error::StaleBatch);
        }
        for line in lines {
            if line.start != offset
                || line.end <= line.start
                || ordinal.checked_add(1) != Some(line.ordinal)
            {
                return Err(Error::StaleBatch);
            }
            offset = line.end;
            ordinal = line.ordinal;
            apply_record(
                &tx,
                path,
                i64::try_from(line.start).map_err(|_| Error::Offset)?,
                line.ordinal,
                line.record,
            )?;
        }
        if progress.offset != offset || progress.ordinal != ordinal {
            return Err(Error::StaleBatch);
        }
        tx.execute("UPDATE sources SET offset=?,ordinal=?,partial=?,known_size=?,tail_length=?,tail_discarding=?,verification_start=?,verification_length=?,verification_hash=? WHERE path=?", params![progress.offset as i64,progress.ordinal,progress.tail_length>0,progress.known_size as i64,progress.tail_length as i64,progress.tail_discarding,progress.verification_start as i64,progress.verification_length,progress.verification_hash.as_ref().map(|h| h.as_slice()),path])?;
        promote(&tx, PROMOTION_LIMIT)?;
        if consumed_lines {
            settings::record_ingestion(&tx)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// The import/live coordinator resumes this bounded work until it returns false.
    pub fn reconcile_pending(&mut self) -> Result<bool> {
        let tx = self.connection.transaction()?;
        promote(&tx, PROMOTION_LIMIT)?;
        hierarchy::advance(&tx)?;
        let accounting_remains: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM reconciliation_work)",
            [],
            |r| r.get(0),
        )?;
        let remains = accounting_remains || hierarchy::pending(&tx)?;
        tx.commit()?;
        Ok(remains)
    }

    /// Future hierarchy consumers must gate readiness and consume edges in one read transaction.
    /// Direct usage deliberately does not depend on this graph's readiness.
    pub fn effective_parent(
        &mut self,
        thread: &str,
    ) -> std::result::Result<Option<String>, crate::hierarchy::ReadError> {
        use crate::hierarchy::ReadError;
        let tx = self
            .connection
            .transaction()
            .map_err(|_| ReadError::Storage)?;
        if hierarchy::pending(&tx).map_err(|_| ReadError::Storage)? {
            return Err(ReadError::HierarchyPending);
        }
        tx.query_row(
            "SELECT parent_thread_id FROM sessions WHERE thread_id=?",
            [thread],
            |r| r.get(0),
        )
        .optional()
        .map(|value| value.flatten())
        .map_err(|_| ReadError::Storage)
    }

    pub fn snapshot(&self) -> Result<Snapshot> {
        snapshot(&self.connection)
    }
}

fn validate_progress(progress: &SourceProgress) -> Result<()> {
    if progress.known_size > i64::MAX as u64
        || progress.offset > progress.known_size
        || progress.ordinal < 0
        || progress.tail_length > progress.known_size - progress.offset
        || progress.verification_start > progress.known_size
        || u64::from(progress.verification_length)
            > progress.known_size - progress.verification_start
        || progress.verification_length > 4096
        || (progress.verification_length == 0) != progress.verification_hash.is_none()
        || (progress.tail_discarding && progress.tail_length == 0)
    {
        return Err(Error::RecoveryMetadata);
    }
    Ok(())
}

fn unsigned(row: &rusqlite::Row<'_>, column: usize) -> rusqlite::Result<u64> {
    let value: i64 = row.get(column)?;
    u64::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(column, value))
}

fn apply_record(
    tx: &Transaction<'_>,
    path: &str,
    start: i64,
    ordinal: i64,
    record: std::result::Result<Record, &'static str>,
) -> Result<()> {
    match record {
        Ok(Record::Metadata(meta)) => {
            let existing: Option<String> =
                tx.query_row("SELECT thread_id FROM sources WHERE path=?", [path], |r| {
                    r.get(0)
                })?;
            let encoded = serde_json::to_string(&meta)?;
            if existing.as_ref().is_some_and(|id| id != &meta.id) {
                diagnostic(&tx, path, "Conflicting direct session identity", true)?;
            } else {
                tx.execute("INSERT INTO sessions(thread_id, metadata) VALUES(?,?) ON CONFLICT(thread_id) DO UPDATE SET metadata=COALESCE(sessions.metadata,excluded.metadata),is_placeholder=0", params![meta.id, encoded])?;
                tx.execute(
                    "UPDATE sources SET thread_id=? WHERE path=?",
                    params![meta.id, path],
                )?;
                for (origin, parent) in [
                    (
                        "session_meta.parent_thread_id",
                        meta.parent_thread_id.as_deref(),
                    ),
                    (
                        "session_meta.source.subagent.thread_spawn.parent_thread_id",
                        meta.nested_parent_thread_id.as_deref(),
                    ),
                ] {
                    if let Some(parent) = parent {
                        evidence(tx, path, start, &meta.id, "", "parent", parent, origin)?;
                    }
                }
                location_evidence(
                    tx,
                    path,
                    start,
                    &meta.id,
                    "",
                    "session_meta",
                    meta.cwd.as_deref(),
                    meta.workspace_roots.as_deref(),
                )?;
                if let Some(version) = meta.cli_version.as_deref() {
                    evidence(
                        tx,
                        path,
                        start,
                        &meta.id,
                        "",
                        "cli_version",
                        version,
                        "session_meta",
                    )?;
                }
                refresh_attribution(tx, &meta.id)?;
            }
        }
        Ok(Record::Context(context)) => {
            // Preserve each detected name before conflict resolution can erase
            // its current attribution. Unknown remains nonconfigurable.
            pricing::detect_model(tx, context.model.as_deref())?;
            let thread: Option<String> =
                tx.query_row("SELECT thread_id FROM sources WHERE path=?", [path], |r| {
                    r.get(0)
                })?;
            if let Some(thread) = thread {
                location_evidence(
                    tx,
                    path,
                    start,
                    &thread,
                    &context.turn_id,
                    "turn_context",
                    context.cwd.as_deref(),
                    context.workspace_roots.as_deref(),
                )?;
                refresh_attribution(tx, &thread)?;
                let old: Option<Option<String>> = tx
                    .query_row(
                        "SELECT model FROM turn_contexts WHERE thread_id=? AND turn_id=?",
                        params![thread, context.turn_id],
                        |r| r.get(0),
                    )
                    .optional()?;
                if old.as_ref().is_some_and(|m| m != &context.model) {
                    diagnostic(
                        &tx,
                        path,
                        "Conflicting model context; model unavailable",
                        false,
                    )?;
                    tx.execute(
                        "UPDATE turn_contexts SET model=NULL WHERE thread_id=? AND turn_id=?",
                        params![thread, context.turn_id],
                    )?;
                    tx.execute(
                            "UPDATE observations SET model=NULL, diagnostic=COALESCE(diagnostic, 'Conflicting model context; model unavailable') WHERE thread_id=? AND json_extract(normalized, '$.turn_id')=?",
                            params![thread, context.turn_id],
                        )?;
                } else {
                    tx.execute(
                        "INSERT OR IGNORE INTO turn_contexts VALUES(?,?,?)",
                        params![thread, context.turn_id, context.model],
                    )?;
                }
            }
        }
        Ok(Record::Usage { timestamp, usage }) => {
            ingest_usage(&tx, path, start, ordinal, &timestamp, &usage)?
        }
        Ok(Record::Legacy { timestamp, windows }) => {
            tx.execute("UPDATE sources SET legacy=1 WHERE path=?", [path])?;
            let retired = match (timestamp.as_deref(), retention::floor(&tx)?) {
                (Some(value), Some(floor)) => {
                    adapter::observation_time(value).is_ok_and(|time| time < floor)
                }
                _ => false,
            };
            for (bucket, position, window) in windows.into_iter().filter(|_| !retired) {
                let normalized = serde_json::to_string(&(
                    &bucket,
                    window.window_minutes,
                    window.window_minutes.is_none().then_some(&position),
                    &timestamp,
                    &window.used_percent,
                    window.resets_at,
                ))?;
                tx.execute("INSERT OR IGNORE INTO limit_samples(bucket,position,window_minutes,timestamp,used_percent,resets_at,normalized,source_path,source_offset,adapter) VALUES(?,?,?,?,?,?,?,?,?,?)", params![bucket, position, window.window_minutes, timestamp, window.used_percent.map(|p| p.to_string()), window.resets_at, normalized, path, start, adapter::VERSION])?;
            }
        }
        Ok(Record::Ignore) => (),
        Ok(Record::UnsupportedEnvelope) => diagnostic(
            &tx,
            path,
            "Unrecognized envelope skipped; only supported modern usage is shown",
            false,
        )?,
        Err(message) => diagnostic(&tx, path, message, true)?,
    }
    Ok(())
}

fn evidence(
    tx: &Transaction<'_>,
    path: &str,
    offset: i64,
    thread: &str,
    turn: &str,
    kind: &str,
    value: &str,
    origin: &str,
) -> Result<()> {
    if value.is_empty() {
        return Ok(());
    }
    tx.execute("INSERT OR IGNORE INTO metadata_evidence(thread_id,kind,value,origin,turn_id,source_path,source_generation,source_offset) SELECT ?,?,?,?,?,path,generation,? FROM sources WHERE path=?", params![thread,kind,value,origin,turn,offset,path])?;
    if kind == "parent" {
        hierarchy::placeholder(tx, value)?;
    }
    Ok(())
}

fn location_evidence(
    tx: &Transaction<'_>,
    path: &str,
    offset: i64,
    thread: &str,
    turn: &str,
    origin: &str,
    cwd: Option<&str>,
    roots: Option<&[String]>,
) -> Result<()> {
    if let Some(cwd) = cwd {
        evidence(tx, path, offset, thread, turn, "cwd", cwd, origin)?;
    }
    if let Some(roots) = roots {
        for root in roots {
            evidence(
                tx,
                path,
                offset,
                thread,
                turn,
                "workspace_root",
                root,
                origin,
            )?;
        }
    }
    Ok(())
}

fn refresh_attribution(tx: &Transaction<'_>, thread: &str) -> Result<()> {
    hierarchy::refresh_candidate(tx, thread)?;
    refresh_location(tx, thread)?;
    Ok(())
}

fn refresh_location(tx: &Transaction<'_>, thread: &str) -> Result<()> {
    let evidence: Vec<(String,String)> = tx.prepare("SELECT DISTINCT kind,value FROM metadata_evidence WHERE thread_id=? AND kind IN ('cwd','workspace_root') ORDER BY kind,value LIMIT ?")?
        .query_map(params![thread,identity::MAX_LOCATION_CANDIDATES as i64 + 1], |r| Ok((r.get(0)?,r.get(1)?)))?.collect::<std::result::Result<_,_>>()?;
    let mut cwds = Vec::new();
    let mut roots = Vec::new();
    for (kind, value) in evidence {
        if kind == "cwd" {
            cwds.push(value);
        } else {
            roots.push(value);
        }
    }
    let location = identity::resolve_location(&cwds, &roots);
    let proof = location
        .path
        .as_deref()
        .map(|path| identity_filesystem::resolve_repository(Path::new(path)));
    tx.execute("UPDATE sessions SET location_path=?,location_state=?,repository_common_directory=?,repository_state=? WHERE thread_id=?", params![location.path,location.state,proof.as_ref().and_then(|proof|proof.common_directory.as_deref()),proof.as_ref().map_or("unresolved",|proof|proof.state),thread])?;
    Ok(())
}

fn snapshot(connection: &Connection) -> Result<Snapshot> {
    let selected: Option<(String, Option<String>)> = connection
        .query_row(
            "SELECT thread_id, timestamp FROM observations WHERE time_seconds IS NOT NULL AND time_nanos IS NOT NULL ORDER BY time_seconds DESC,time_nanos DESC,thread_id ASC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let diagnostic: Option<String> = connection.query_row("SELECT diagnostic FROM sources WHERE diagnostic IS NOT NULL ORDER BY rowid DESC LIMIT 1", [], |r| r.get(0)).optional()?;
    let partial: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sources WHERE partial=1)",
        [],
        |r| r.get(0),
    )?;
    let mut snapshot = Snapshot {
        source_available: true,
        diagnostic,
        coverage: "Unavailable — waiting for supported modern usage".into(),
        ..Default::default()
    };
    if let Some((thread, timestamp)) = selected {
        let thread_diagnostic: Option<String> = connection.query_row("SELECT diagnostic FROM observations WHERE thread_id=? AND diagnostic IS NOT NULL ORDER BY time_seconds DESC,time_nanos DESC LIMIT 1", [&thread], |r| r.get(0)).optional()?;
        let unresolved: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM observations WHERE thread_id=? AND accepted=0)",
            [&thread],
            |r| r.get(0),
        )?;
        let total: Option<i64> = connection.query_row(
            "SELECT SUM(total) FROM observations WHERE thread_id=? AND accepted=1",
            [&thread],
            |r| r.get(0),
        )?;
        snapshot.diagnostic = thread_diagnostic.map(|message| {
            if message == "Source accounting stopped after an unsupported record" {
                "Some historical usage remains unavailable until its source is replayed and validated".into()
            } else {
                message
            }
        }).or(snapshot.diagnostic);
        snapshot.thread_id = Some(thread);
        snapshot.observed_at = timestamp;
        snapshot.direct_tokens = total.map(|n| n.to_string());
        snapshot.coverage = if unresolved {
            if snapshot.direct_tokens.is_some() {
                "Incomplete observed direct usage; some observations are pending or unavailable"
            } else {
                "Direct usage unavailable; observations are pending or unsupported"
            }
        } else {
            "Observed direct usage only; history may be incomplete"
        }
        .into();
    } else {
        let legacy: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM sources WHERE legacy=1)",
            [],
            |r| r.get(0),
        )?;
        if legacy && snapshot.diagnostic.is_none() {
            snapshot.diagnostic =
                Some("Legacy-only counters are not supported; usage unavailable".into());
        }
    }
    if partial && snapshot.diagnostic.is_none() {
        snapshot.diagnostic = Some("Waiting for an incomplete trailing line to finish".into());
    }
    Ok(snapshot)
}

fn diagnostic(tx: &Transaction<'_>, path: &str, message: &str, halt: bool) -> Result<()> {
    tx.execute(
        "UPDATE sources SET diagnostic=CASE WHEN halted=1 THEN diagnostic ELSE ? END,halted=MAX(halted,?) WHERE path=?",
        params![message, halt, path],
    )?;
    Ok(())
}
fn ingest_usage(
    tx: &Transaction<'_>,
    path: &str,
    offset: i64,
    ordinal: i64,
    timestamp: &str,
    usage: &Usage,
) -> Result<()> {
    let encoded = serde_json::to_string(usage)?;
    let endpoint = serde_json::to_string(&usage.thread_token_usage)?;
    let time = adapter::observation_time(timestamp);
    // Replay validation precedes duplicate handling. Only a new generation's
    // clean prefix can reconsider previously suppressed (not invalid) usage.
    let (thread, halted, generation): (Option<String>, bool, i64) = tx.query_row(
        "SELECT thread_id,halted,generation FROM sources WHERE path=?",
        [path],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let validation = if halted {
        Err("Source accounting stopped after an unsupported record")
    } else if thread.as_ref().is_some_and(|id| id != &usage.thread_id) {
        Err("Metadata and direct thread identity conflict")
    } else {
        time.and_then(|_| accounting::reconcile(&usage.usage, &usage.thread_token_usage, None))
    };
    let prior: Option<(i64, String, String, String)> = tx.query_row(
        "SELECT id,normalized,timestamp,state FROM observations WHERE thread_id=? AND (endpoint=? OR (response_id IS NOT NULL AND response_id=?)) ORDER BY id LIMIT 1",
        params![usage.thread_id, endpoint, usage.response_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).optional()?;
    if let Some((id, prior, prior_time, state)) = prior {
        if prior != encoded || adapter::observation_time(&prior_time) != time {
            diagnostic(
                tx,
                path,
                "Conflicting usage identity; duplicate endpoint was not counted",
                true,
            )?;
            tx.execute("UPDATE observations SET state='rejected',diagnostic='Conflicting usage identity; pending usage unavailable' WHERE id=? AND state='pending'",[id])?;
        } else if let Err(message) = validation {
            // An immutable duplicate does not make this replay prefix valid.
            // Restore its source halt without rewriting the stored decision.
            diagnostic(tx, path, message, true)?;
        } else if state == "pending" {
            reconcile_candidate(tx, id)?;
        } else if state == "rejected"
            && validation.is_ok()
            && thread.as_ref() == Some(&usage.thread_id)
        {
            let recovered = tx.execute("UPDATE observations SET state='pending',diagnostic=? WHERE id=? AND accepted=0 AND state='rejected' AND diagnostic='Source accounting stopped after an unsupported record' AND source_path=? AND source_generation<?",params![GAP,id,path,generation])?;
            if recovered != 0 {
                reconcile_candidate(tx, id)?;
            }
        }
        return Ok(());
    }
    // Retired history is never re-imported by a replay; the source still
    // learns its thread identity so identity conflicts remain detectable.
    if let (Ok(parsed), Some(floor)) = (time, retention::floor(tx)?) {
        if parsed < floor {
            tx.execute(
                "UPDATE sources SET thread_id=COALESCE(thread_id,?) WHERE path=?",
                params![usage.thread_id, path],
            )?;
            return Ok(());
        }
    }
    tx.execute(
        "INSERT INTO sessions(thread_id) VALUES(?) ON CONFLICT(thread_id) DO UPDATE SET is_placeholder=0",
        [&usage.thread_id],
    )?;
    tx.execute(
        "UPDATE sources SET thread_id=COALESCE(thread_id,?) WHERE path=?",
        params![usage.thread_id, path],
    )?;
    let message = validation.as_ref().err().copied();
    if let Some(message) = message {
        diagnostic(tx, path, message, true)?;
    }
    let model: Option<String> = tx
        .query_row(
            "SELECT model FROM turn_contexts WHERE thread_id=? AND turn_id=?",
            params![usage.thread_id, usage.turn_id],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    let parsed = time.ok();
    tx.execute("INSERT INTO observations(thread_id,endpoint,response_id,timestamp,normalized,adapter,source_path,source_offset,source_ordinal,model,accepted,total,diagnostic,source_generation,state,time_seconds,time_nanos,endpoint_order,start_order) VALUES(?,?,?,?,?,?,?,?,?,?,0,NULL,?,?,?,?,?,?,?)",
        params![usage.thread_id, endpoint, usage.response_id, timestamp, encoded, adapter::VERSION, path, offset, ordinal, model, message,generation,if validation.is_ok() { "pending" } else { "rejected" },parsed.map(|t| t.0),parsed.map(|t| t.1),accounting::endpoint_key(&usage.thread_token_usage),accounting::start_key(&usage.usage,&usage.thread_token_usage)])?;
    if validation.is_ok() {
        reconcile_candidate(tx, tx.last_insert_rowid())?;
    }
    Ok(())
}

struct Observation {
    id: i64,
    usage: Usage,
    time: (i64, u32),
    path: String,
    generation: i64,
    offset: i64,
}

fn observation(row: &rusqlite::Row<'_>) -> rusqlite::Result<Observation> {
    let encoded: String = row.get(1)?;
    let usage = serde_json::from_str(&encoded).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Text, Box::new(e))
    })?;
    Ok(Observation {
        id: row.get(0)?,
        usage,
        time: (row.get(2)?, row.get(3)?),
        path: row.get(4)?,
        generation: row.get(5)?,
        offset: row.get(6)?,
    })
}

const OBSERVATION_COLUMNS: &str =
    "id,normalized,time_seconds,time_nanos,source_path,source_generation,source_offset";
pub(super) const GAP: &str =
    "Usage pending: historical gap or ambiguous ordering; confirmed usage retained";

impl Observation {
    fn facts(&self) -> accounting::ObservationFacts<'_> {
        accounting::ObservationFacts {
            usage: &self.usage.usage,
            endpoint: &self.usage.thread_token_usage,
            time: self.time,
            source: &self.path,
            generation: self.generation,
            offset: self.offset,
        }
    }
}

/// Existing accepted rows are immutable anchors. A candidate must bridge both
/// neighbors where present, so restoring history never changes confirmed totals.
fn reconcile_candidate(tx: &Transaction<'_>, id: i64) -> Result<bool> {
    let candidate = tx
        .query_row(
            &format!(
                "SELECT {OBSERVATION_COLUMNS} FROM observations WHERE id=? AND state='pending'"
            ),
            [id],
            observation,
        )
        .optional()?;
    let Some(candidate) = candidate else {
        return Ok(false);
    };
    let thread = &candidate.usage.thread_id;
    let unknown_anchor: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM observations WHERE thread_id=? AND state='accepted' AND time_seconds IS NULL)",[thread],|r|r.get(0))?;
    // Indexed neighbors bound memory and work even for a large equal-time group.
    // The accounting module decides order and acceptance from these facts.
    let before = tx.query_row(&format!("SELECT {OBSERVATION_COLUMNS} FROM observations WHERE thread_id=? AND state='accepted' AND (time_seconds,time_nanos)<(?,?) ORDER BY time_seconds DESC,time_nanos DESC,endpoint_order DESC LIMIT 1"),params![thread,candidate.time.0,candidate.time.1],observation).optional()?;
    let after = tx.query_row(&format!("SELECT {OBSERVATION_COLUMNS} FROM observations WHERE thread_id=? AND state='accepted' AND (time_seconds,time_nanos)>(?,?) ORDER BY time_seconds,time_nanos,endpoint_order LIMIT 1"),params![thread,candidate.time.0,candidate.time.1],observation).optional()?;
    let key = accounting::endpoint_key(&candidate.usage.thread_token_usage);
    let mut equal = Vec::with_capacity(4);
    for (comparison, order) in [("<", "DESC"), (">", "ASC")] {
        if let Some(anchor)=tx.query_row(&format!("SELECT {OBSERVATION_COLUMNS} FROM observations WHERE thread_id=? AND state='accepted' AND time_seconds=? AND time_nanos=? AND endpoint_order {comparison} ? ORDER BY endpoint_order {order} LIMIT 1"),params![thread,candidate.time.0,candidate.time.1,key],observation).optional()? { equal.push(anchor); }
        if let Some(anchor)=tx.query_row(&format!("SELECT {OBSERVATION_COLUMNS} FROM observations WHERE thread_id=? AND state='accepted' AND time_seconds=? AND time_nanos=? AND source_path=? AND source_generation=? AND source_offset {comparison} ? ORDER BY source_offset {order} LIMIT 1"),params![thread,candidate.time.0,candidate.time.1,candidate.path,candidate.generation,candidate.offset],observation).optional()? {
            if !equal.iter().any(|other| other.id==anchor.id) { equal.push(anchor); }
        }
    }
    let anchors: Vec<_> = before.into_iter().chain(after).chain(equal).collect();
    let facts: Vec<_> = anchors.iter().map(Observation::facts).collect();
    if let accounting::Acceptance::Accept { incomplete_opening } =
        accounting::assess_candidate(candidate.facts(), &facts, unknown_anchor)
    {
        tx.execute("UPDATE observations SET state='accepted',accepted=1,total=?,diagnostic=CASE WHEN diagnostic=? THEN NULL ELSE diagnostic END WHERE id=? AND state='pending'",params![candidate.usage.usage.total_tokens.value(),GAP,candidate.id])?;
        pricing::value_observation(tx, candidate.id)?;
        if incomplete_opening {
            tx.execute(
                "UPDATE sessions SET incomplete=1 WHERE thread_id=?",
                [thread],
            )?;
            diagnostic(
                tx,
                &candidate.path,
                "Opening endpoint includes unobserved usage; only explicit usage counted",
                false,
            )?;
        }
        let pending: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM observations WHERE source_path=? AND source_generation=? AND state='pending')",params![candidate.path,candidate.generation],|r|r.get(0))?;
        if !pending {
            tx.execute(
                "UPDATE sources SET diagnostic=NULL WHERE path=? AND generation=? AND diagnostic=?",
                params![candidate.path, candidate.generation, GAP],
            )?;
        }
        // Wake only candidates connected by exact six-category endpoints. An
        // unrelated append must not rescan all unresolved history for a thread.
        let start =
            accounting::start_key(&candidate.usage.usage, &candidate.usage.thread_token_usage);
        tx.execute("INSERT OR IGNORE INTO reconciliation_work(observation_id) SELECT id FROM observations WHERE thread_id=? AND state='pending' AND endpoint_order=? LIMIT 1",params![thread,start])?;
        tx.execute("INSERT OR IGNORE INTO reconciliation_work(observation_id) SELECT id FROM observations WHERE thread_id=? AND state='pending' AND start_order=? ORDER BY time_seconds,time_nanos LIMIT 1",params![thread,key])?;
        Ok(true)
    } else {
        tx.execute(
            "UPDATE observations SET diagnostic=COALESCE(diagnostic,?) WHERE id=?",
            params![GAP, candidate.id],
        )?;
        diagnostic(tx, &candidate.path, GAP, false)?;
        Ok(false)
    }
}

fn promote(tx: &Transaction<'_>, limit: usize) -> Result<()> {
    for _ in 0..limit {
        let work: Option<i64> = tx
            .query_row(
                "SELECT observation_id FROM reconciliation_work ORDER BY observation_id LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()?;
        let Some(id) = work else {
            break;
        };
        tx.execute(
            "DELETE FROM reconciliation_work WHERE observation_id=?",
            [id],
        )?;
        reconcile_candidate(tx, id)?;
    }
    Ok(())
}
