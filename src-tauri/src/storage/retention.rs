//! Bounded retirement of usage older than the retention window. Only the sole
//! writer runs it; each pass commits a small batch with its valuations and
//! work rows, so interruption leaves nothing half retired.
use super::{Result, Store};
use crate::adapter;
use rusqlite::{params, Connection, OptionalExtension};

pub const RETENTION_SECONDS: i64 = 45 * 86_400;
pub const RETENTION_BATCH: usize = 256;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Retired {
    pub observations: usize,
    pub samples: usize,
}

/// Usage below this canonical time was retired and is not re-imported.
pub(super) fn floor(connection: &Connection) -> Result<Option<(i64, u32)>> {
    Ok(connection
        .query_row(
            "SELECT floor_seconds,floor_nanos FROM retention_control WHERE id=1",
            [],
            |row| {
                Ok(row
                    .get::<_, Option<i64>>(0)?
                    .zip(row.get::<_, Option<u32>>(1)?))
            },
        )
        .optional()?
        .flatten())
}

/// Calendar date `YYYY-MM-DD` for a lexical timestamp prefilter.
fn date(seconds: i64) -> Option<String> {
    let value = time::OffsetDateTime::from_unix_timestamp(seconds).ok()?;
    Some(format!(
        "{:04}-{:02}-{:02}",
        value.year(),
        u8::from(value.month()),
        value.day()
    ))
}

impl Store {
    /// Retire usage older than the window before the newest stored time. The
    /// anchor never exceeds the clock, so a future clock cannot empty history.
    /// Zero removals means the store is caught up.
    pub fn retire_expired(&mut self, now: (i64, u32), batch: usize) -> Result<Retired> {
        let tx = self.connection.transaction()?;
        // The observation must go before its valuation for the delete guard,
        // so the enforced foreign key is checked at commit instead.
        tx.execute_batch("PRAGMA defer_foreign_keys=ON")?;
        let latest_observation: Option<(i64, u32)> = tx.query_row("SELECT time_seconds,time_nanos FROM observations WHERE time_seconds IS NOT NULL AND time_nanos IS NOT NULL ORDER BY time_seconds DESC,time_nanos DESC LIMIT 1", [], |row| Ok((row.get(0)?, row.get(1)?))).optional()?;
        let latest_sample: Option<String> = tx.query_row("SELECT timestamp FROM limit_samples WHERE timestamp IS NOT NULL ORDER BY timestamp DESC LIMIT 1", [], |row| row.get(0)).optional()?;
        let latest_sample = latest_sample.and_then(|value| adapter::observation_time(&value).ok());
        let Some(newest) = latest_observation.max(latest_sample) else {
            return Ok(Retired::default());
        };
        let anchor = newest.min(now);
        let cutoff = (anchor.0 - RETENTION_SECONDS, anchor.1);

        // The newest id survives so rowids never restart and adopt a stale row.
        let expired: Vec<(i64, String, String, i64)> = {
            let mut query = tx.prepare("SELECT id,state,source_path,source_generation FROM observations WHERE time_seconds IS NOT NULL AND time_nanos IS NOT NULL AND (time_seconds,time_nanos)<(?1,?2) AND id<(SELECT MAX(id) FROM observations) ORDER BY time_seconds,time_nanos LIMIT ?3")?;
            let rows = query.query_map(params![cutoff.0, cutoff.1, batch as i64], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })?;
            rows.collect::<std::result::Result<_, _>>()?
        };
        let mut pending_sources: Vec<(String, i64)> = Vec::new();
        for (id, state, path, generation) in &expired {
            // The observation goes first so its valuation may follow.
            tx.execute("DELETE FROM observations WHERE id=?", [id])?;
            tx.execute(
                "DELETE FROM observation_valuations WHERE observation_id=?",
                [id],
            )?;
            tx.execute(
                "DELETE FROM reconciliation_work WHERE observation_id=?",
                [id],
            )?;
            if state == "pending"
                && !pending_sources
                    .iter()
                    .any(|(p, g)| p == path && g == generation)
            {
                pending_sources.push((path.clone(), *generation));
            }
        }
        for (path, generation) in pending_sources {
            tx.execute("UPDATE sources SET diagnostic=NULL WHERE path=?1 AND generation=?2 AND diagnostic=?3 AND NOT EXISTS(SELECT 1 FROM observations WHERE source_path=?1 AND source_generation=?2 AND state='pending')", params![path, generation, super::GAP])?;
        }

        // Timestamps keep their source offsets, so compare exactly in Rust after
        // a lexical date prefilter that is a superset for any offset.
        let mut samples = 0;
        if let Some(prefilter) = date(cutoff.0 + 2 * 86_400) {
            let mut after: (String, i64) = (String::new(), 0);
            loop {
                let page: Vec<(i64, String)> = {
                    let mut query = tx.prepare("SELECT id,timestamp FROM limit_samples WHERE timestamp IS NOT NULL AND (timestamp>?1 OR (timestamp=?1 AND id>?2)) AND timestamp<?3 ORDER BY timestamp,id LIMIT ?4")?;
                    let rows = query
                        .query_map(params![after.0, after.1, prefilter, batch as i64], |row| {
                            Ok((row.get(0)?, row.get(1)?))
                        })?;
                    rows.collect::<std::result::Result<_, _>>()?
                };
                let Some((last_id, last_timestamp)) = page.last() else {
                    break;
                };
                after = (last_timestamp.clone(), *last_id);
                for (id, timestamp) in &page {
                    if adapter::observation_time(timestamp).is_ok_and(|time| time < cutoff) {
                        tx.execute("DELETE FROM limit_samples WHERE id=?", [id])?;
                        samples += 1;
                    }
                }
                if page.len() < batch || samples >= batch {
                    break;
                }
            }
        }
        tx.execute("UPDATE retention_control SET floor_seconds=?1,floor_nanos=?2 WHERE id=1 AND (floor_seconds IS NULL OR floor_nanos IS NULL OR (floor_seconds,floor_nanos)<(?1,?2))", params![cutoff.0, cutoff.1])?;
        tx.commit()?;
        Ok(Retired {
            observations: expired.len(),
            samples,
        })
    }
}
