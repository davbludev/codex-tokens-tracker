use super::{attribution, count, summary, Selection, PROJECTS};
use crate::aggregates::{
    session_list::{Page, Query, Row, Sort},
    ReadError,
};
use rusqlite::{params, Transaction};

pub(super) fn read(tx: &Transaction<'_>, query: Query, pending: bool) -> Result<Page, ReadError> {
    query.validate()?;
    // Only accepted observations contribute sort metrics. Exact cost is ordered by
    // canonical decimal length then text, never coerced to SQLite REAL or INTEGER.
    let cte = format!("WITH {PROJECTS}, metrics AS (
        SELECT s.thread_id, s.project_id,
          (SELECT timestamp FROM observations WHERE thread_id=s.thread_id AND accepted=1 AND time_seconds IS NOT NULL ORDER BY time_seconds DESC,time_nanos DESC,id DESC LIMIT 1) AS observed_at,
          (SELECT time_seconds FROM observations WHERE thread_id=s.thread_id AND accepted=1 AND time_seconds IS NOT NULL ORDER BY time_seconds DESC,time_nanos DESC,id DESC LIMIT 1) AS seconds,
          (SELECT time_nanos FROM observations WHERE thread_id=s.thread_id AND accepted=1 AND time_seconds IS NOT NULL ORDER BY time_seconds DESC,time_nanos DESC,id DESC LIMIT 1) AS nanos,
          SUM(o.total) AS tokens,
          CASE WHEN COUNT(o.id)>0 AND COUNT(o.id)=COUNT(v.observation_id) THEN estimated_cost_sum(v.amount) END AS cost
        FROM project_sessions s LEFT JOIN observations o ON o.thread_id=s.thread_id AND o.accepted=1
        LEFT JOIN observation_valuations v ON v.observation_id=o.id
        WHERE s.is_placeholder=0 GROUP BY s.thread_id
      ), filtered AS (SELECT * FROM metrics m WHERE
        (?1 IS NULL OR instr(lower(m.thread_id),lower(?1))>0)
        AND (?2 IS NULL OR instr(lower(CASE WHEN project_id='unknown:' THEN 'Unavailable' ELSE substr(project_id,instr(project_id,':')+1) END),lower(?2))>0)
        AND (?3 IS NULL OR EXISTS(SELECT 1 FROM observations o WHERE o.thread_id=m.thread_id AND o.accepted=1 AND instr(lower(COALESCE(o.model,'Unavailable')),lower(?3))>0)
          OR (instr('unavailable',lower(?3))>0 AND NOT EXISTS(SELECT 1 FROM observations o WHERE o.thread_id=m.thread_id AND o.accepted=1)))
        AND (?4 IS NULL OR seconds>=?4) AND (?5 IS NULL OR seconds<?5))");
    let filter = params![
        query.search,
        query.project,
        query.model,
        query.from_seconds,
        query.before_seconds
    ];
    let total_items: u64 = tx
        .query_row(
            &format!("{cte} SELECT COUNT(*) FROM filtered"),
            filter,
            |r| count(r, 0),
        )
        .map_err(|_| ReadError::Storage)?;
    let order = match query.sort {
        Sort::Newest => "seconds DESC NULLS LAST,nanos DESC NULLS LAST",
        Sort::Usd => "length(cost) DESC NULLS LAST,cost DESC NULLS LAST",
        Sort::Tokens => "tokens DESC NULLS LAST",
    };
    let mut statement = tx.prepare(&format!("{cte} SELECT thread_id,project_id,observed_at FROM filtered ORDER BY project_id,{order},thread_id LIMIT ?6 OFFSET ?7")).map_err(|_| ReadError::Storage)?;
    let selected = statement
        .query_map(
            params![
                query.search,
                query.project,
                query.model,
                query.from_seconds,
                query.before_seconds,
                query.limit,
                query.offset
            ],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .map_err(|_| ReadError::Storage)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|_| ReadError::Storage)?;
    let mut items = Vec::with_capacity(selected.len());
    for (thread, project, last_observed_at) in selected {
        let mut models_query = tx.prepare("SELECT DISTINCT model FROM observations WHERE thread_id=?1 AND accepted=1 AND model IS NOT NULL ORDER BY model LIMIT 8").map_err(|_| ReadError::Storage)?;
        let models = models_query
            .query_map([&thread], |r| r.get(0))
            .map_err(|_| ReadError::Storage)?
            .collect::<rusqlite::Result<Vec<String>>>()
            .map_err(|_| ReadError::Storage)?;
        let (model_count, unknown_model): (u64,bool) = tx.query_row("SELECT COUNT(DISTINCT model),COUNT(*)=0 OR COUNT(*)>COUNT(model) FROM observations WHERE thread_id=?1 AND accepted=1", [&thread], |r| Ok((count(r, 0)?,r.get(1)?))).map_err(|_| ReadError::Storage)?;
        let direct_subagent_count = if pending {
            None
        } else {
            Some(tx.query_row("SELECT COUNT(*) FROM sessions WHERE parent_thread_id=?1 AND parent_state='available' AND is_placeholder=0", [&thread], |r| count(r, 0)).map_err(|_| ReadError::Storage)?)
        };
        items.push(Row {
            direct: summary(tx, &Selection::thread(), Some(&thread))?,
            thread_id: thread,
            title: None,
            last_observed_at,
            duration_seconds: None,
            project: attribution(project),
            models,
            model_count,
            unknown_model,
            direct_subagent_count,
            weekly_percentage_impact: None,
        });
    }
    let end = u64::from(query.offset) + items.len() as u64;
    Ok(Page {
        items,
        total_items,
        offset: query.offset,
        next_offset: if end < total_items {
            u32::try_from(end).ok()
        } else {
            None
        },
    })
}
