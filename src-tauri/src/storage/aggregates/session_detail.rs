//! Session-scoped reads reuse accounting summaries and stream timestamp groups.
use super::{attribution, count, summary, summary_with_params, trim_page, Selection};
use crate::{
    aggregates::{
        self as dto,
        session_detail::{self as detail, Downsample, Projection, UsageGroup},
        ReadError,
    },
    weekly::Time,
};
use rusqlite::{params, OptionalExtension, Transaction};

fn exists(tx: &Transaction<'_>, thread: &str) -> Result<bool, ReadError> {
    tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM sessions WHERE thread_id=?1)",
        [thread],
        |row| row.get(0),
    )
    .map_err(|_| ReadError::Storage)
}

pub(super) fn observed_bounds(
    tx: &Transaction<'_>,
    thread: &str,
) -> Result<(Option<String>, Option<String>), ReadError> {
    let bound = |order: &str| {
        tx.query_row(
        &format!("SELECT timestamp FROM observations WHERE thread_id=?1 AND accepted=1 AND time_seconds IS NOT NULL AND time_nanos IS NOT NULL ORDER BY time_seconds {order},time_nanos {order},id ASC LIMIT 1"),
        [thread], |row| row.get::<_, Option<String>>(0),
    ).optional().map(Option::flatten).map_err(|_| ReadError::Storage)
    };
    Ok((bound("ASC")?, bound("DESC")?))
}

pub(super) fn models(
    tx: &Transaction<'_>,
    thread: &str,
    page: dto::PageRequest,
) -> Result<Option<detail::Models>, ReadError> {
    page.validate()?;
    if !exists(tx, thread)? {
        return Ok(None);
    }
    let direct = summary(tx, &Selection::thread(), Some(thread))?;
    let groups = "WITH groups AS (
        SELECT DISTINCT CASE WHEN model IS NULL THEN 'unknown:' ELSE 'model:' || model END AS id
        FROM observations WHERE thread_id=?1
        UNION SELECT 'unknown:' WHERE EXISTS(SELECT 1 FROM sessions WHERE thread_id=?1 AND is_placeholder=0)
        AND NOT EXISTS(SELECT 1 FROM observations WHERE thread_id=?1))";
    let total_items = tx
        .query_row(
            &format!("{groups} SELECT COUNT(*) FROM groups"),
            [thread],
            |row| count(row, 0),
        )
        .map_err(|_| ReadError::Storage)?;
    let mut statement = tx
        .prepare(&format!(
            "{groups} SELECT id FROM groups WHERE (?2 IS NULL OR id>?2) ORDER BY id LIMIT ?3"
        ))
        .map_err(|_| ReadError::Storage)?;
    let mut ids = statement
        .query_map(params![thread, page.after, page.limit + 1], |row| {
            row.get::<_, String>(0)
        })
        .map_err(|_| ReadError::Storage)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|_| ReadError::Storage)?;
    let next_cursor = trim_page(&mut ids, page.limit);
    let selection = Selection {
        members: "",
        filter: "thread_id=?2",
        model: true,
    };
    let items = ids
        .into_iter()
        .map(|id| {
            let row = summary_with_params(tx, &selection, [Some(id.as_str()), Some(thread)])?;
            let (cost_share, cost_share_unavailable_reason) =
                detail::cost_share(&row.estimated_cost, &direct.estimated_cost)?;
            Ok(detail::ModelUsage {
                attribution: attribution(id),
                direct: row,
                cost_share,
                cost_share_unavailable_reason,
            })
        })
        .collect::<Result<_, ReadError>>()?;
    Ok(Some(detail::Models {
        scope: "direct",
        items,
        next_cursor,
        total_items,
        direct,
    }))
}

pub(super) fn timeline(
    tx: &Transaction<'_>,
    thread: &str,
    budget: Option<u32>,
) -> Result<Option<detail::Timeline>, ReadError> {
    let point_budget = detail::point_budget(budget)?;
    if !exists(tx, thread)? {
        return Ok(None);
    }
    let direct = summary(tx, &Selection::thread(), Some(thread))?;
    let untimed_observation_count = tx.query_row(
        "SELECT COUNT(*) FROM observations WHERE thread_id=?1 AND accepted=1 AND (time_seconds IS NULL OR time_nanos IS NULL)",
        [thread], |row| count(row, 0),
    ).map_err(|_| ReadError::Storage)?;
    let bound = |order: &str| {
        tx.query_row(
        &format!("SELECT time_seconds,time_nanos FROM observations WHERE thread_id=?1 AND accepted=1 AND time_seconds IS NOT NULL AND time_nanos IS NOT NULL ORDER BY time_seconds {order},time_nanos {order} LIMIT 1"),
        [thread], |row| Ok(Time { seconds: row.get(0)?, nanos: row.get(1)? }),
    ).optional().map_err(|_| ReadError::Storage)
    };
    let first_observed_at = bound("ASC")?;
    let last_observed_at = bound("DESC")?;
    let mut result = detail::Timeline {
        scope: "direct", time_source: "acceptedObservationTimestamp",
        first_observed_at, last_observed_at, point_budget, bin_count: 0,
        source_observation_count: 0, source_point_count: 0, returned_point_count: 0,
        untimed_observation_count, direct, points: Vec::new(), boundaries: Vec::new(),
        coverage_note: "Direct accepted usage since observation began, not session lifecycle. Equal timestamps are combined. Untimed usage is included in direct totals but excluded from the timeline. Values are exact cumulative known subtotals; incomplete values remain incomplete. Missing or unpriced spans break their series; overloaded bins conservatively disconnect. No synthetic zero or resets. Separate requests are live snapshots.",
    };
    let (Some(start), Some(end)) = (first_observed_at, last_observed_at) else {
        return Ok(Some(result));
    };
    let mut projection = Projection::default();
    let mut downsample = Downsample::new(start, end, point_budget);
    // Group before accumulating: peers at one timestamp must produce one exact
    // cumulative point regardless of insertion order. Never collect source rows.
    let mut statement = tx.prepare("SELECT o.time_seconds,o.time_nanos,COUNT(*),SUM(o.total),COUNT(o.total),estimated_cost_sum(v.amount),COUNT(v.observation_id)
        FROM observations o LEFT JOIN observation_valuations v ON v.observation_id=o.id
        WHERE o.thread_id=?1 AND o.accepted=1 AND o.time_seconds IS NOT NULL AND o.time_nanos IS NOT NULL
        GROUP BY o.time_seconds,o.time_nanos ORDER BY o.time_seconds,o.time_nanos").map_err(|_| ReadError::Storage)?;
    let mut rows = statement.query([thread]).map_err(|_| ReadError::Storage)?;
    while let Some(row) = rows.next().map_err(|_| ReadError::Storage)? {
        let read = || -> rusqlite::Result<UsageGroup> {
            Ok(UsageGroup {
                time: Time {
                    seconds: row.get(0)?,
                    nanos: row.get(1)?,
                },
                accepted: count(row, 2)?,
                tokens: row.get(3)?,
                tokens_known: count(row, 4)?,
                cost: row.get(5)?,
                priced: count(row, 6)?,
            })
        };
        let group = read().map_err(|_| ReadError::Storage)?;
        result.source_observation_count += group.accepted;
        result.source_point_count += 1;
        let (point, boundaries) = projection.push(group)?;
        downsample.push(point, boundaries);
    }
    (result.points, result.boundaries) = downsample.finish();
    result.bin_count = point_budget / 2;
    result.returned_point_count = result.points.len();
    Ok(Some(result))
}
