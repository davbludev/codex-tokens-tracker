//! The ingestion Store remains the only writer. Runtime scheduling and IPC are
//! intentionally separate consumers of these bounded storage operations.
use super::{Result, Store};
use crate::{
    adapter::Usage,
    pricing::{self, PriceInput},
};
use rusqlite::{params, OptionalExtension, Transaction};
use serde::Serialize;

pub const PRICING_BATCH_SIZE: usize = 64;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PriceVersion {
    pub id: i64,
    pub model: String,
    pub effective_seconds: i64,
    pub effective_nanos: u32,
    pub backfill_before: bool,
    pub configuration: PriceInput,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedModel {
    pub model: String,
    pub latest_price: Option<PriceVersion>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Valuation {
    pub observation_id: i64,
    pub version_id: i64,
    pub model: String,
    /// Canonical integer string in 10^-12 USD, never a floating-point USD value.
    pub amount: String,
    pub attribution_conflict: bool,
}

pub(super) fn detect_model(tx: &Transaction<'_>, model: Option<&str>) -> Result<()> {
    if let Some(model) = model.filter(|name| !name.trim().is_empty()) {
        tx.execute(
            "INSERT OR IGNORE INTO detected_models(model) VALUES(?)",
            [model],
        )?;
    }
    Ok(())
}

impl Store {
    /// Keyset catalog page, including detected names without attributable usage.
    /// Continue with the last returned model until a page is empty.
    pub fn pricing_models(&self, after: Option<&str>) -> Result<Vec<DetectedModel>> {
        let mut query = self.connection.prepare(
            "SELECT model FROM detected_models WHERE model>COALESCE(?,'') ORDER BY model LIMIT 64",
        )?;
        let names = query
            .query_map([after], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        names.into_iter().map(|model| {
            let latest_price = self.connection.query_row(
                "SELECT id,model,effective_seconds,effective_nanos,backfill_before,configuration FROM model_price_versions WHERE model=? ORDER BY effective_seconds DESC,effective_nanos DESC LIMIT 1",
                [&model], version).optional()?;
            Ok(DetectedModel { model, latest_price })
        }).collect()
    }

    pub fn save_model_price(
        &mut self,
        model: &str,
        configuration: PriceInput,
        backfill_before: bool,
    ) -> Result<PriceVersion> {
        let now = time::OffsetDateTime::now_utc();
        self.save_price_at(
            model,
            configuration,
            backfill_before,
            (now.unix_timestamp(), now.nanosecond()),
        )
    }

    #[cfg(test)]
    pub(crate) fn save_model_price_at(
        &mut self,
        model: &str,
        configuration: PriceInput,
        backfill_before: bool,
        now: (i64, u32),
    ) -> Result<PriceVersion> {
        self.save_price_at(model, configuration, backfill_before, now)
    }

    fn save_price_at(
        &mut self,
        model: &str,
        configuration: PriceInput,
        backfill_before: bool,
        now: (i64, u32),
    ) -> Result<PriceVersion> {
        configuration.validate()?;
        let tx = self.connection.transaction()?;
        let detected: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM detected_models WHERE model=?)",
            [model],
            |r| r.get(0),
        )?;
        if !detected {
            return Err(pricing::Error::UnknownModel.into());
        }
        let last: Option<(i64, u32)> = tx.query_row("SELECT effective_seconds,effective_nanos FROM model_price_versions WHERE model=? ORDER BY effective_seconds DESC,effective_nanos DESC LIMIT 1", [model], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
        if now.1 >= 1_000_000_000 || last.is_some_and(|previous| previous >= now) {
            return Err(pricing::Error::Clock.into());
        }
        if last.is_some() && backfill_before {
            return Err(pricing::Error::BackfillOnlyFirst.into());
        }
        tx.execute("INSERT INTO model_price_versions(model,effective_seconds,effective_nanos,backfill_before,configuration) VALUES(?,?,?,?,?)", params![model,now.0,now.1,backfill_before,serde_json::to_string(&configuration)?])?;
        let id = tx.last_insert_rowid();
        tx.execute("INSERT INTO pricing_work(version_id,through_id) SELECT ?,COALESCE(MAX(id),0) FROM observations", [id])?;
        tx.commit()?;
        Ok(PriceVersion {
            id,
            model: model.into(),
            effective_seconds: now.0,
            effective_nanos: now.1,
            backfill_before,
            configuration,
        })
    }

    pub fn pricing_work_pending(&self) -> Result<bool> {
        Ok(self
            .connection
            .query_row("SELECT EXISTS(SELECT 1 FROM pricing_work)", [], |r| {
                r.get(0)
            })?)
    }

    /// At most 64 observations/job completions per transaction. The keyset
    /// cursor and valuations commit together, so interruption safely resumes.
    /// Returns the number of observations examined (not the number priced).
    pub fn process_pricing_work(&mut self) -> Result<usize> {
        let tx = self.connection.transaction()?;
        let mut budget = PRICING_BATCH_SIZE;
        let mut examined = 0;
        while budget > 0 {
            let job: Option<(i64, String, i64, i64)> = tx.query_row("SELECT w.version_id,v.model,w.after_id,w.through_id FROM pricing_work w JOIN model_price_versions v ON v.id=w.version_id ORDER BY w.version_id LIMIT 1", [], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
            let Some((job_id, model, after, through)) = job else {
                break;
            };
            let ids = {
                let mut query = tx.prepare("SELECT id FROM observations WHERE model=? AND accepted=1 AND id>? AND id<=? ORDER BY id LIMIT ?")?;
                let rows = query.query_map(params![model, after, through, budget as i64], |r| {
                    r.get::<_, i64>(0)
                })?;
                rows.collect::<std::result::Result<Vec<_>, _>>()?
            };
            if ids.is_empty() {
                tx.execute("DELETE FROM pricing_work WHERE version_id=?", [job_id])?;
                budget -= 1;
            } else {
                for id in &ids {
                    value_observation(&tx, *id)?;
                }
                tx.execute(
                    "UPDATE pricing_work SET after_id=? WHERE version_id=?",
                    params![ids.last(), job_id],
                )?;
                budget -= ids.len();
                examined += ids.len();
            }
        }
        tx.commit()?;
        Ok(examined)
    }

    pub fn observation_valuation(&self, observation_id: i64) -> Result<Option<Valuation>> {
        Ok(self.connection.query_row("SELECT v.observation_id,v.version_id,v.model,v.amount,o.model IS NOT v.model FROM observation_valuations v JOIN observations o ON o.id=v.observation_id WHERE v.observation_id=?", [observation_id], |r| Ok(Valuation { observation_id:r.get(0)?,version_id:r.get(1)?,model:r.get(2)?,amount:r.get(3)?,attribution_conflict:r.get(4)? })).optional()?)
    }
}

fn version(row: &rusqlite::Row<'_>) -> rusqlite::Result<PriceVersion> {
    let encoded: String = row.get(5)?;
    let configuration = serde_json::from_str(&encoded).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(error))
    })?;
    Ok(PriceVersion {
        id: row.get(0)?,
        model: row.get(1)?,
        effective_seconds: row.get(2)?,
        effective_nanos: row.get(3)?,
        backfill_before: row.get(4)?,
        configuration,
    })
}

pub(super) fn value_observation(tx: &Transaction<'_>, id: i64) -> Result<()> {
    // A durable valuation always wins, even if model attribution or the clock
    // subsequently changes. Never update accepted token or provenance fields.
    let observation: Option<(String,i64,u32,String)> = tx.query_row("SELECT model,time_seconds,time_nanos,normalized FROM observations WHERE id=? AND accepted=1 AND model IS NOT NULL AND time_seconds IS NOT NULL AND time_nanos IS NOT NULL AND NOT EXISTS(SELECT 1 FROM observation_valuations WHERE observation_id=observations.id)", [id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
    let Some((model, seconds, nanos, encoded)) = observation else {
        return Ok(());
    };
    let selected = tx.query_row("SELECT id,model,effective_seconds,effective_nanos,backfill_before,configuration FROM model_price_versions WHERE model=? AND (effective_seconds,effective_nanos)<=(?,?) ORDER BY effective_seconds DESC,effective_nanos DESC LIMIT 1", params![model,seconds,nanos], version).optional()?;
    let selected = match selected {
        Some(value) => Some(value),
        None => tx.query_row("SELECT id,model,effective_seconds,effective_nanos,backfill_before,configuration FROM model_price_versions WHERE model=? ORDER BY effective_seconds,effective_nanos LIMIT 1", [&model], version).optional()?.filter(|v| v.backfill_before),
    };
    let Some(selected) = selected else {
        return Ok(());
    };
    let usage: Usage = serde_json::from_str(&encoded)?;
    // Invalid stored configuration is an error; unsupported token semantics or
    // arithmetic overflow leave usage explicitly without a valuation.
    let rates = selected.configuration.validate()?;
    let Ok(amount) = rates.value(&usage.usage) else {
        return Ok(());
    };
    tx.execute("INSERT OR IGNORE INTO observation_valuations(observation_id,version_id,model,amount) VALUES(?,?,?,?)", params![id,selected.id,model,amount.to_string()])?;
    Ok(())
}
