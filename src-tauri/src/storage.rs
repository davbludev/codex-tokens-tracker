use crate::{
    accounting,
    adapter::{self, Record, Usage},
};
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
impl Store {
    #[cfg(test)]
    pub fn connection(&self) -> &Connection {
        &self.connection
    }
    pub fn open(path: &Path) -> Result<Self> {
        let connection = Connection::open(path)?;
        connection.busy_timeout(std::time::Duration::from_secs(3))?;
        connection.execute_batch(include_str!("../migrations/001_initial.sql"))?;
        Ok(Self { connection })
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
        let start = i64::try_from(start).map_err(|_| Error::Offset)?;
        let end = i64::try_from(end).map_err(|_| Error::Offset)?;
        let tx = self.connection.transaction()?;
        match record {
            Ok(Record::Metadata(meta)) => {
                let existing: Option<String> =
                    tx.query_row("SELECT thread_id FROM sources WHERE path=?", [path], |r| {
                        r.get(0)
                    })?;
                let encoded = serde_json::to_string(&meta)?;
                let old: Option<String> = tx
                    .query_row(
                        "SELECT metadata FROM sessions WHERE thread_id=?",
                        [&meta.id],
                        |r| r.get(0),
                    )
                    .optional()?
                    .flatten();
                if existing.as_ref().is_some_and(|id| id != &meta.id)
                    || old.as_ref().is_some_and(|value| value != &encoded)
                {
                    diagnostic(&tx, path, "Conflicting session identity or metadata", true)?;
                } else {
                    tx.execute("INSERT INTO sessions(thread_id, metadata) VALUES(?,?) ON CONFLICT(thread_id) DO UPDATE SET metadata=COALESCE(sessions.metadata,excluded.metadata)", params![meta.id, encoded])?;
                    tx.execute(
                        "UPDATE sources SET thread_id=? WHERE path=?",
                        params![meta.id, path],
                    )?;
                }
            }
            Ok(Record::Context(context)) => {
                let thread: Option<String> =
                    tx.query_row("SELECT thread_id FROM sources WHERE path=?", [path], |r| {
                        r.get(0)
                    })?;
                if let Some(thread) = thread {
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
                for (bucket, position, window) in windows {
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
        tx.execute(
            "UPDATE sources SET offset=?,ordinal=?,partial=0 WHERE path=?",
            params![end, ordinal, path],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn snapshot(&self) -> Result<Snapshot> {
        let selected: Option<(String, Option<String>)> = self
            .connection
            .query_row(
                "SELECT thread_id, timestamp FROM observations ORDER BY id DESC LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let diagnostic: Option<String> = self.connection.query_row("SELECT diagnostic FROM sources WHERE diagnostic IS NOT NULL ORDER BY rowid DESC LIMIT 1", [], |r| r.get(0)).optional()?;
        let partial: bool = self.connection.query_row(
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
            let total: Option<i64> = self.connection.query_row(
                "SELECT SUM(total) FROM observations WHERE thread_id=? AND accepted=1",
                [&thread],
                |r| r.get(0),
            )?;
            snapshot.thread_id = Some(thread);
            snapshot.observed_at = timestamp;
            snapshot.direct_tokens = total.map(|n| n.to_string());
            snapshot.coverage = "Observed direct usage only; history may be incomplete".into();
        } else {
            let legacy: bool = self.connection.query_row(
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
    let prior: Option<(String, String)> = tx.query_row(
        "SELECT normalized,timestamp FROM observations WHERE thread_id=? AND (endpoint=? OR (response_id IS NOT NULL AND response_id=?)) ORDER BY id LIMIT 1",
        params![usage.thread_id, endpoint, usage.response_id], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
    if let Some((prior, prior_time)) = prior {
        if prior != encoded || prior_time != timestamp {
            diagnostic(
                tx,
                path,
                "Conflicting usage identity; duplicate endpoint was not counted",
                true,
            )?;
        }
        return Ok(());
    }
    let (thread, halted): (Option<String>, bool) = tx.query_row(
        "SELECT thread_id,halted FROM sources WHERE path=?",
        [path],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let previous: Option<String> = tx.query_row("SELECT normalized FROM observations WHERE thread_id=? AND accepted=1 ORDER BY id DESC LIMIT 1", [&usage.thread_id], |r| r.get(0)).optional()?;
    let previous: Option<Usage> = previous.map(|s| serde_json::from_str(&s)).transpose()?;
    let validation = if halted {
        Err("Source accounting stopped after an unsupported record")
    } else if thread.as_ref().is_some_and(|id| id != &usage.thread_id) {
        Err("Metadata and direct thread identity conflict")
    } else {
        accounting::reconcile(
            &usage.usage,
            &usage.thread_token_usage,
            previous.as_ref().map(|p| &p.thread_token_usage),
        )
    };
    tx.execute(
        "INSERT OR IGNORE INTO sessions(thread_id) VALUES(?)",
        [&usage.thread_id],
    )?;
    tx.execute(
        "UPDATE sources SET thread_id=COALESCE(thread_id,?) WHERE path=?",
        params![usage.thread_id, path],
    )?;
    let accepted = validation.is_ok();
    let message = validation.as_ref().err().copied();
    if let Some(message) = message {
        diagnostic(tx, path, message, true)?;
    }
    if validation == Ok(true) {
        tx.execute(
            "UPDATE sessions SET incomplete=1 WHERE thread_id=?",
            [&usage.thread_id],
        )?;
        diagnostic(
            tx,
            path,
            "Opening endpoint includes unobserved usage; only explicit usage counted",
            false,
        )?;
    }
    let model: Option<String> = tx
        .query_row(
            "SELECT model FROM turn_contexts WHERE thread_id=? AND turn_id=?",
            params![usage.thread_id, usage.turn_id],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    tx.execute("INSERT INTO observations(thread_id,endpoint,response_id,timestamp,normalized,adapter,source_path,source_offset,source_ordinal,model,accepted,total,diagnostic) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?)",
        params![usage.thread_id, endpoint, usage.response_id, timestamp, encoded, adapter::VERSION, path, offset, ordinal, model, accepted, if accepted { usage.usage.total_tokens.value() } else { None }, message])?;
    Ok(())
}
