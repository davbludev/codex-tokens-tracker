//! Bounded analytics queries over the existing direct accounting projection.
use super::*;
use crate::{
    aggregates::{analytics as a, session_detail::cost_share},
    weekly::{self, Time},
};

const MODEL_ID: &str = "CASE WHEN model IS NULL THEN 'unknown:' ELSE 'model:' || model END";

fn project_selection() -> Selection {
    Selection {
        filter: "project_id=?1 AND is_placeholder=0",
        ..Selection::all()
    }
}
fn model_selection() -> Selection {
    Selection { filter: "is_placeholder=0 AND thread_id IN (SELECT thread_id FROM observations WHERE accepted=1 AND CASE WHEN model IS NULL THEN 'unknown:' ELSE 'model:' || model END=?1)", model: true, members: "" }
}

fn ids(
    tx: &Transaction<'_>,
    cte: &str,
    key: Option<&str>,
    page: dto::PageRequest,
) -> Result<(Vec<String>, u64, Option<String>), ReadError> {
    page.validate()?;
    let total = tx
        .query_row(
            &format!("{cte} SELECT COUNT(*) FROM groups WHERE (?1 IS NULL OR 1)"),
            [key],
            |r| count(r, 0),
        )
        .map_err(|_| ReadError::Storage)?;
    let mut statement = tx.prepare(&format!("{cte} SELECT id FROM groups WHERE (?1 IS NULL OR 1) AND (?2 IS NULL OR id>?2) ORDER BY id LIMIT ?3")).map_err(|_| ReadError::Storage)?;
    let mut ids = statement
        .query_map(params![key, page.after, page.limit + 1], |r| r.get(0))
        .map_err(|_| ReadError::Storage)?
        .collect::<rusqlite::Result<Vec<String>>>()
        .map_err(|_| ReadError::Storage)?;
    let next = trim_page(&mut ids, page.limit);
    Ok((ids, total, next))
}

pub(super) fn project_models(
    tx: &Transaction<'_>,
    project: &str,
    page: dto::PageRequest,
) -> Result<a::ProjectModels, ReadError> {
    let cte = format!(
        "{}, groups AS (SELECT DISTINCT {MODEL_ID} AS id FROM usage WHERE accepted=1)",
        project_selection().cte()
    );
    let (ids, total_items, next_cursor) = ids(tx, &cte, Some(project), page)?;
    Ok(a::ProjectModels {
        items: ids.into_iter().map(attribution).collect(),
        total_items,
        next_cursor,
    })
}

pub(super) fn projects(
    tx: &Transaction<'_>,
    page: dto::PageRequest,
    pending: bool,
    now: Time,
) -> Result<a::Projects, ReadError> {
    let cte = format!("WITH {PROJECTS}, groups AS (SELECT DISTINCT project_id AS id FROM project_sessions WHERE is_placeholder=0)");
    let (ids, total_items, next_cursor) = ids(tx, &cte, None, page)?;
    let weekly = super::super::weekly::project(
        tx,
        weekly::Query {
            before: None,
            limit: 1,
        },
        now,
    )
    .map_err(|_| ReadError::Storage)?;
    let mut items = Vec::with_capacity(ids.len());
    for id in ids {
        let direct = summary(tx, &project_selection(), Some(&id))?;
        let average_session_cost = a::average(&direct)?;
        let classification = if pending {
            None
        } else {
            let bucket = |filter| {
                summary(
                    tx,
                    &Selection {
                        filter,
                        ..Selection::all()
                    },
                    Some(&id),
                )
            };
            Some(a::Classification {
                proven_subagent: bucket(
                    "project_id=?1 AND is_placeholder=0 AND parent_state='available'",
                )?,
                parent_classification_unavailable: bucket(
                    "project_id=?1 AND is_placeholder=0 AND parent_state<>'available'",
                )?,
            })
        };
        let interval = match (weekly.overall.start, weekly.overall.end) {
            (Some(start), Some(end)) => {
                let cte = format!("WITH {PROJECTS}, scope AS (SELECT * FROM project_sessions WHERE project_id=?1 AND is_placeholder=0), usage AS (SELECT o.* FROM observations o JOIN scope s ON s.thread_id=o.thread_id WHERE o.time_seconds IS NOT NULL AND o.time_nanos IS NOT NULL AND (o.time_seconds,o.time_nanos)>(?2,?3) AND (o.time_seconds,o.time_nanos)<=(?4,?5))");
                Some(summary_from_cte(
                    tx,
                    &cte,
                    params![id, start.seconds, start.nanos, end.seconds, end.nanos],
                )?)
            }
            _ => None,
        };
        let current_cycle = a::CycleUsage {
            label: "Observed current-cycle usage",
            cycle_key: weekly.current_cycle.as_ref().map(|c| c.key.clone()),
            start: weekly.overall.start,
            end: weekly.overall.end,
            observation_age_seconds: weekly.observation_age_seconds,
            partial: true,
            has_ambiguous_observations: weekly
                .current_cycle
                .as_ref()
                .is_some_and(|c| c.has_ambiguous_observations),
            unavailable_reason: if interval.is_some() {
                None
            } else {
                weekly.overall.unavailable_reason.clone()
            },
            direct: interval,
        };
        let models = project_models(
            tx,
            &id,
            dto::PageRequest {
                after: None,
                limit: 5,
            },
        )?;
        items.push(a::Project {
            attribution: attribution(id),
            direct,
            average_session_cost,
            models,
            classification,
            current_cycle,
        });
    }
    Ok(a::Projects {
        evaluated_at: now,
        items,
        total_items,
        next_cursor,
        direct: global_summary(tx)?,
    })
}

pub(super) fn models(
    tx: &Transaction<'_>,
    page: dto::PageRequest,
    now: Time,
) -> Result<a::Models, ReadError> {
    let cte = format!("WITH groups AS (SELECT 'model:' || model AS id FROM detected_models UNION SELECT {MODEL_ID} FROM observations WHERE accepted=1)");
    let (ids, total_items, next_cursor) = ids(tx, &cte, None, page)?;
    let direct = global_summary(tx)?;
    let mut items = Vec::with_capacity(ids.len());
    for id in ids {
        let selection = model_selection();
        let model_direct = summary(tx, &selection, Some(&id))?;
        let (accepted_usage_events, sessions_used) = tx
            .query_row(
                &format!(
                    "{} SELECT COUNT(*),COUNT(DISTINCT thread_id) FROM usage WHERE accepted=1",
                    selection.cte()
                ),
                [&id],
                |r| Ok((count(r, 0)?, count(r, 1)?)),
            )
            .map_err(|_| ReadError::Storage)?;
        let (cost_share, cost_share_unavailable_reason) =
            cost_share(&model_direct.estimated_cost, &direct.estimated_cost)?;
        let active_pricing_version = if let Some(model) = id.strip_prefix("model:") {
            tx.query_row("SELECT id,effective_seconds,effective_nanos FROM model_price_versions WHERE model=?1 AND (effective_seconds,effective_nanos)<=(?2,?3) ORDER BY effective_seconds DESC,effective_nanos DESC LIMIT 1", params![model, now.seconds, now.nanos], |r| Ok(a::ActivePrice { version_id: r.get(0)?, effective_at: Time { seconds: r.get(1)?, nanos: r.get(2)? } })).optional().map_err(|_| ReadError::Storage)?
        } else {
            None
        };
        items.push(a::Model {
            attribution: attribution(id),
            direct: model_direct,
            accepted_usage_events,
            sessions_used,
            cost_share,
            cost_share_unavailable_reason,
            active_pricing_version,
        });
    }
    Ok(a::Models {
        evaluated_at: now,
        items,
        total_items,
        next_cursor,
        direct,
    })
}

fn nanos(time: Time) -> i128 {
    i128::from(time.seconds) * 1_000_000_000 + i128::from(time.nanos)
}
fn time(value: i128) -> Time {
    Time {
        seconds: value.div_euclid(1_000_000_000) as i64,
        nanos: value.rem_euclid(1_000_000_000) as u32,
    }
}

pub(super) fn history(
    tx: &Transaction<'_>,
    model: &str,
    start: Time,
    end: Time,
    budget: Option<u32>,
) -> Result<a::History, ReadError> {
    let budget = budget.unwrap_or(512);
    if !(1..=4096).contains(&budget)
        || start >= end
        || start.nanos >= 1_000_000_000
        || end.nanos >= 1_000_000_000
        || !(model == "unknown:" || model.strip_prefix("model:").is_some_and(|m| !m.is_empty()))
    {
        return Err(ReadError::InvalidQuery);
    }
    let width = nanos(end) - nanos(start);
    // Exact i128 arithmetic avoids SQLite's floating-point promotion for wide ranges.
    tx.create_scalar_function(
        "analytics_bin",
        2,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        move |ctx| {
            let value = Time {
                seconds: ctx.get(0)?,
                nanos: ctx.get(1)?,
            };
            Ok(((nanos(value) - nanos(start)) * i128::from(budget) - 1).div_euclid(width) as i64)
        },
    )
    .map_err(|_| ReadError::Storage)?;
    let fields = [
        "o.total",
        "json_extract(o.normalized,'$.usage.input_tokens')",
        "json_extract(o.normalized,'$.usage.cached_input_tokens')",
        "json_extract(o.normalized,'$.usage.cache_write_input_tokens')",
        "json_extract(o.normalized,'$.usage.output_tokens')",
        "json_extract(o.normalized,'$.usage.reasoning_output_tokens')",
    ]
    .iter()
    .map(|field| format!("SUM({field}),COUNT({field})"))
    .collect::<Vec<_>>()
    .join(",");
    let cte = model_selection().cte();
    let untimed_accepted_usage_events = tx.query_row(&format!("{cte} SELECT COUNT(*) FROM usage WHERE accepted=1 AND (time_seconds IS NULL OR time_nanos IS NULL)"), [model], |r| count(r,0)).map_err(|_| ReadError::Storage)?;
    let sql = format!("{cte} SELECT analytics_bin(o.time_seconds,o.time_nanos) AS bin, COUNT(*), {fields}, estimated_cost_sum(v.amount),COUNT(v.observation_id) FROM usage o LEFT JOIN observation_valuations v ON v.observation_id=o.id WHERE o.accepted=1 AND o.time_seconds IS NOT NULL AND o.time_nanos IS NOT NULL AND (o.time_seconds,o.time_nanos)>(?2,?3) AND (o.time_seconds,o.time_nanos)<=(?4,?5) GROUP BY bin ORDER BY bin LIMIT ?6");
    let mut statement = tx.prepare(&sql).map_err(|_| ReadError::Storage)?;
    let bins = statement
        .query_map(
            params![
                model,
                start.seconds,
                start.nanos,
                end.seconds,
                end.nanos,
                budget
            ],
            |r| {
                let index: u32 = r.get(0)?;
                let accepted: i64 = r.get(1)?;
                let category = |column| -> rusqlite::Result<dto::Category> {
                    Ok(dto::Category::from_sum(
                        r.get(column)?,
                        r.get(column + 1)?,
                        accepted,
                    ))
                };
                let bound = |i: u32| {
                    time(nanos(start) + (width * i128::from(i)).div_euclid(i128::from(budget)))
                };
                Ok(a::HistoryBin {
                    index,
                    start: bound(index),
                    end: bound(index + 1),
                    accepted_usage_events: accepted as u64,
                    tokens: dto::Tokens {
                        total_tokens: category(2)?,
                        input_tokens: category(4)?,
                        cached_input_tokens: category(6)?,
                        cache_write_tokens: category(8)?,
                        output_tokens: category(10)?,
                        reasoning_tokens: category(12)?,
                    },
                    estimated_cost: dto::EstimatedCost {
                        known_subtotal: r.get(14)?,
                        complete: accepted == r.get::<_, i64>(15)?,
                    },
                })
            },
        )
        .map_err(|_| ReadError::Storage)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|_| ReadError::Storage)?;
    Ok(a::History { attribution: attribution(model.into()), start, end, point_budget: budget, bins, untimed_accepted_usage_events,
        coverage_note: "Accepted direct usage in start-exclusive/end-inclusive bins. Omitted bins have no accepted observations, not measured complete zero. Untimed accepted usage is outside the series. Cached input and reasoning overlap other categories; do not add categories. Local source coverage may be incomplete." })
}
