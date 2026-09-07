//! Read-side aggregation stays in SQLite; only bounded prepared rows leave storage.
use super::{hierarchy, Store};
use crate::aggregates::{self as dto, ReadError};
use rusqlite::{
    functions::{Aggregate, Context, FunctionFlags},
    params, Connection, OptionalExtension, Transaction,
};
use std::path::Path;

const PROJECTS: &str = "project_sessions AS (
 SELECT s.*,
 CASE WHEN is_placeholder=0 AND repository_state='confirmed' AND repository_common_directory IS NOT NULL
      THEN 'repository:' || repository_common_directory
      WHEN is_placeholder=0 AND location_state='available' AND location_path IS NOT NULL
      THEN 'location:' || location_path ELSE 'unknown:' END AS project_id
 FROM sessions s)";

struct Selection {
    members: &'static str,
    filter: &'static str,
    model: bool,
}
impl Selection {
    fn all() -> Self {
        Self {
            members: "",
            filter: "1",
            model: false,
        }
    }
    fn thread() -> Self {
        Self {
            filter: "thread_id=?1",
            ..Self::all()
        }
    }
    fn descendants() -> Self {
        Self {
            members: "members(thread_id) AS (SELECT ?1 UNION SELECT s.thread_id FROM sessions s JOIN members m ON s.parent_thread_id=m.thread_id WHERE s.parent_state='available'),",
            filter: "thread_id IN (SELECT thread_id FROM members)",
            model: false,
        }
    }
    fn cte(&self) -> String {
        format!("WITH RECURSIVE {} {PROJECTS}, scope AS (SELECT * FROM project_sessions WHERE {}), usage AS (SELECT o.* FROM observations o JOIN scope s ON s.thread_id=o.thread_id {})", self.members, self.filter, if self.model { "WHERE CASE WHEN o.model IS NULL THEN 'unknown:' ELSE 'model:' || o.model END=?1" } else { "" })
    }
}

impl Store {
    /// Delivery never migrates or creates storage, nor competes for the writer connection.
    pub fn read_aggregates(path: &Path, query: dto::Query) -> Result<dto::Response, ReadError> {
        let connection =
            Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .map_err(|_| ReadError::Storage)?;
        connection
            .busy_timeout(std::time::Duration::from_secs(3))
            .map_err(|_| ReadError::Storage)?;
        let mut store = Self { connection };
        store.aggregates(query)
    }

    pub fn aggregates(&mut self, query: dto::Query) -> Result<dto::Response, ReadError> {
        self.connection
            .create_aggregate_function(
                "estimated_cost_sum",
                1,
                FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
                CostSum,
            )
            .map_err(|_| ReadError::Storage)?;
        let tx = self
            .connection
            .transaction()
            .map_err(|_| ReadError::Storage)?;
        let pending = hierarchy::pending(&tx).map_err(|_| ReadError::Storage)?;
        let revision = tx
            .query_row(
                "SELECT revision FROM hierarchy_control WHERE id=1",
                [],
                |r| r.get(0),
            )
            .map_err(|_| ReadError::Storage)?;
        let data = match query {
            dto::Query::Global => dto::Data::Global(summary(&tx, &Selection::all(), None)?),
            dto::Query::Session { thread } => dto::Data::Session(session(&tx, &thread, pending)?),
            dto::Query::Sessions { page } => {
                dto::Data::Sessions(sessions(&tx, &Selection::all(), None, page, pending)?)
            }
            dto::Query::Children { thread, page } => {
                if pending {
                    return Err(ReadError::HierarchyPending);
                }
                let selection = Selection {
                    filter: "parent_thread_id=?1 AND parent_state='available'",
                    ..Selection::all()
                };
                dto::Data::Sessions(sessions(&tx, &selection, Some(&thread), page, false)?)
            }
            dto::Query::Ancestors { thread, page } => {
                if pending {
                    return Err(ReadError::HierarchyPending);
                }
                let selection = Selection {
                    members: "members(thread_id) AS (SELECT parent_thread_id FROM sessions WHERE thread_id=?1 AND parent_state='available' UNION SELECT s.parent_thread_id FROM sessions s JOIN members m ON s.thread_id=m.thread_id WHERE s.parent_state='available'),",
                    filter: "thread_id IN (SELECT thread_id FROM members)", model: false,
                };
                dto::Data::Sessions(sessions(&tx, &selection, Some(&thread), page, false)?)
            }
            dto::Query::Projects { page } => dto::Data::Projects(groups(&tx, page, false)?),
            dto::Query::Models { page } => dto::Data::Models(groups(&tx, page, true)?),
        };
        Ok(dto::Response {
            hierarchy_pending: pending, hierarchy_revision: revision, data,
            coverage_note: "Locally observed accepted usage only; legacy or unobserved history may be unavailable. Cached input and reasoning overlap other categories; do not add categories. Group totals use direct usage only.",
        })
    }
}

fn summary(
    tx: &Transaction<'_>,
    selection: &Selection,
    key: Option<&str>,
) -> Result<dto::Summary, ReadError> {
    let cte = selection.cte();
    // json_extract operates only on the durable allowlisted projection. No observation
    // list or normalized JSON is loaded into Rust or sent across IPC.
    let categories = [
        "total",
        "json_extract(normalized,'$.usage.input_tokens')",
        "json_extract(normalized,'$.usage.cached_input_tokens')",
        "json_extract(normalized,'$.usage.cache_write_input_tokens')",
        "json_extract(normalized,'$.usage.output_tokens')",
        "json_extract(normalized,'$.usage.reasoning_output_tokens')",
    ];
    let fields = categories
        .iter()
        .map(|field| format!("SUM({field}),COUNT({field})"))
        .collect::<Vec<_>>()
        .join(",");
    let tokens = tx
        .query_row(
            &format!(
                "{cte} SELECT COUNT(*),{fields} FROM usage WHERE accepted=1 AND (?1 IS NULL OR 1)"
            ),
            [key],
            |row| {
                let accepted: i64 = row.get(0)?;
                let category = |column| -> rusqlite::Result<dto::Category> {
                    Ok(dto::Category::from_sum(
                        row.get(column)?,
                        row.get(column + 1)?,
                        accepted,
                    ))
                };
                Ok(dto::Tokens {
                    total_tokens: category(1)?,
                    input_tokens: category(3)?,
                    cached_input_tokens: category(5)?,
                    cache_write_tokens: category(7)?,
                    output_tokens: category(9)?,
                    reasoning_tokens: category(11)?,
                })
            },
        )
        .map_err(|_| ReadError::Storage)?;
    let estimated_cost = tx.query_row(
        &format!("{cte} SELECT estimated_cost_sum(v.amount),COUNT(*),COUNT(v.observation_id) FROM usage o LEFT JOIN observation_valuations v ON v.observation_id=o.id WHERE o.accepted=1 AND (?1 IS NULL OR 1)"),
        [key],
        |row| {
            let accepted: i64 = row.get(1)?;
            let priced: i64 = row.get(2)?;
            Ok(dto::EstimatedCost {
                known_subtotal: row.get(0)?,
                complete: accepted > 0 && accepted == priced,
            })
        },
    ).map_err(|_| ReadError::Storage)?;
    let (observed_sessions, placeholders, incomplete_sessions, unavailable_sessions, unattributed_project): (u64,u64,u64,u64,bool) = tx.query_row(&format!("{cte} SELECT
        COUNT(CASE WHEN is_placeholder=0 THEN 1 END), COUNT(CASE WHEN is_placeholder=1 THEN 1 END),
        COUNT(CASE WHEN is_placeholder=0 AND incomplete=1 THEN 1 END),
        COUNT(CASE WHEN is_placeholder=0 AND NOT EXISTS(SELECT 1 FROM usage o WHERE o.thread_id=scope.thread_id AND accepted=1) THEN 1 END),
        EXISTS(SELECT 1 FROM scope WHERE is_placeholder=0 AND project_id='unknown:')
        FROM scope WHERE (?1 IS NULL OR 1)"), [key], |r| Ok((count(r,0)?,count(r,1)?,count(r,2)?,count(r,3)?,r.get(4)?))).map_err(|_| ReadError::Storage)?;
    let (unresolved_usage, unknown_model, source_diagnostics): (bool,bool,bool) = tx.query_row(&format!("{cte} SELECT
        EXISTS(SELECT 1 FROM usage WHERE accepted=0),
        EXISTS(SELECT 1 FROM usage WHERE accepted=1 AND model IS NULL),
        EXISTS(SELECT 1 FROM sources WHERE (thread_id IN (SELECT thread_id FROM scope) OR (?1 IS NULL AND thread_id IS NULL)) AND (diagnostic IS NOT NULL OR legacy=1 OR halted=1 OR partial=1))"), [key], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(|_| ReadError::Storage)?;
    let observed_at = tx.query_row(&format!("{cte} SELECT timestamp FROM usage WHERE time_seconds IS NOT NULL AND time_nanos IS NOT NULL AND (?1 IS NULL OR 1) ORDER BY time_seconds DESC,time_nanos DESC,thread_id ASC LIMIT 1"), [key], |r| r.get(0)).optional().map_err(|_| ReadError::Storage)?;
    Ok(dto::Summary {
        tokens,
        estimated_cost,
        observed_sessions,
        placeholders,
        observed_at,
        coverage: dto::Coverage {
            incomplete_sessions,
            unavailable_sessions,
            unresolved_usage,
            unknown_model,
            unattributed_project,
            source_diagnostics,
        },
    })
}

/// SQLite's numeric SUM would coerce durable i128 TEXT values to i64 or float.
/// Keep a single checked accumulator inside the query and return only its string.
struct CostSum;
impl Aggregate<Option<i128>, Option<String>> for CostSum {
    fn init(&self, _: &mut Context<'_>) -> rusqlite::Result<Option<i128>> {
        Ok(None)
    }

    fn step(&self, ctx: &mut Context<'_>, sum: &mut Option<i128>) -> rusqlite::Result<()> {
        let Some(encoded) = ctx.get::<Option<String>>(0)? else {
            return Ok(());
        };
        let invalid = || {
            rusqlite::Error::UserFunctionError(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Invalid or overflowing estimated cost subtotal",
            )))
        };
        let amount: i128 = encoded.parse().map_err(|_| invalid())?;
        if amount < 0 || amount.to_string() != encoded {
            return Err(invalid());
        }
        *sum = Some(sum.unwrap_or(0).checked_add(amount).ok_or_else(invalid)?);
        Ok(())
    }

    fn finalize(
        &self,
        _: &mut Context<'_>,
        sum: Option<Option<i128>>,
    ) -> rusqlite::Result<Option<String>> {
        Ok(sum.flatten().map(|amount| amount.to_string()))
    }
}

fn attribution(id: String) -> dto::Attribution {
    let (basis, value) = match id.split_once(':') {
        Some(("repository", value)) => ("confirmedRepository", Some(value.to_owned())),
        Some(("location", value)) => ("locationDerived", Some(value.to_owned())),
        Some(("model", value)) => ("observedModel", Some(value.to_owned())),
        _ => ("unavailable", None),
    };
    dto::Attribution {
        id,
        basis: basis.into(),
        value,
    }
}

fn session(
    tx: &Transaction<'_>,
    thread: &str,
    pending: bool,
) -> Result<Option<dto::Session>, ReadError> {
    let row: Option<(bool,String,Option<String>,String)> = tx.query_row(&format!("WITH {PROJECTS} SELECT is_placeholder,parent_state,parent_thread_id,project_id FROM project_sessions WHERE thread_id=?"), [thread], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(|_| ReadError::Storage)?;
    let Some((placeholder, parent_state, parent_thread_id, project)) = row else {
        return Ok(None);
    };
    Ok(Some(dto::Session {
        thread_id: thread.into(),
        placeholder,
        parent_state: if pending {
            "pending".into()
        } else {
            parent_state
        },
        parent_thread_id: if pending { None } else { parent_thread_id },
        project: attribution(project),
        direct: summary(tx, &Selection::thread(), Some(thread))?,
        inclusive: if pending {
            None
        } else {
            Some(summary(tx, &Selection::descendants(), Some(thread))?)
        },
    }))
}

fn sessions(
    tx: &Transaction<'_>,
    selection: &Selection,
    key: Option<&str>,
    page: dto::PageRequest,
    pending: bool,
) -> Result<dto::Page<dto::Session>, ReadError> {
    page.validate()?;
    let cte = selection.cte();
    let total_items = tx
        .query_row(
            &format!("{cte} SELECT COUNT(*) FROM scope WHERE (?1 IS NULL OR 1)"),
            [key],
            |r| count(r, 0),
        )
        .map_err(|_| ReadError::Storage)?;
    let mut query = tx.prepare(&format!("{cte} SELECT thread_id FROM scope WHERE (?1 IS NULL OR 1) AND (?2 IS NULL OR thread_id>?2) ORDER BY thread_id LIMIT ?3")).map_err(|_| ReadError::Storage)?;
    let mut ids = query
        .query_map(params![key, page.after, page.limit + 1], |r| {
            r.get::<_, String>(0)
        })
        .map_err(|_| ReadError::Storage)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|_| ReadError::Storage)?;
    let next_cursor = trim_page(&mut ids, page.limit);
    let items = ids
        .iter()
        .map(|id| session(tx, id, pending)?.ok_or(ReadError::Storage))
        .collect::<Result<_, _>>()?;
    Ok(dto::Page {
        items,
        next_cursor,
        total_items,
        direct: summary(tx, selection, key)?,
    })
}

fn groups(
    tx: &Transaction<'_>,
    page: dto::PageRequest,
    model: bool,
) -> Result<dto::Page<dto::Group>, ReadError> {
    page.validate()?;
    let groups = if model {
        "SELECT DISTINCT CASE WHEN model IS NULL THEN 'unknown:' ELSE 'model:' || model END AS id FROM observations
         UNION SELECT 'unknown:' WHERE EXISTS(SELECT 1 FROM sessions s WHERE is_placeholder=0 AND NOT EXISTS(SELECT 1 FROM observations o WHERE o.thread_id=s.thread_id))"
    } else {
        "SELECT DISTINCT project_id AS id FROM project_sessions WHERE is_placeholder=0"
    };
    let cte = format!("WITH {PROJECTS}, groups AS ({groups})");
    let total_items = tx
        .query_row(&format!("{cte} SELECT COUNT(*) FROM groups"), [], |r| {
            count(r, 0)
        })
        .map_err(|_| ReadError::Storage)?;
    let mut query = tx
        .prepare(&format!(
            "{cte} SELECT id FROM groups WHERE (?1 IS NULL OR id>?1) ORDER BY id LIMIT ?2"
        ))
        .map_err(|_| ReadError::Storage)?;
    let mut ids = query
        .query_map(params![page.after, page.limit + 1], |r| {
            r.get::<_, String>(0)
        })
        .map_err(|_| ReadError::Storage)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|_| ReadError::Storage)?;
    let next_cursor = trim_page(&mut ids, page.limit);
    let selection = if model {
        Selection { filter: "thread_id IN (SELECT thread_id FROM observations WHERE CASE WHEN model IS NULL THEN 'unknown:' ELSE 'model:' || model END=?1) OR (?1='unknown:' AND is_placeholder=0 AND NOT EXISTS(SELECT 1 FROM observations o WHERE o.thread_id=project_sessions.thread_id))", model: true, members: "" }
    } else {
        Selection {
            filter: "project_id=?1 AND is_placeholder=0",
            ..Selection::all()
        }
    };
    let items = ids
        .into_iter()
        .map(|id| {
            Ok(dto::Group {
                direct: summary(tx, &selection, Some(&id))?,
                attribution: attribution(id),
            })
        })
        .collect::<Result<_, ReadError>>()?;
    Ok(dto::Page {
        items,
        next_cursor,
        total_items,
        direct: summary(tx, &Selection::all(), None)?,
    })
}

fn trim_page(ids: &mut Vec<String>, limit: u32) -> Option<String> {
    if ids.len() > limit as usize {
        ids.pop();
        ids.last().cloned()
    } else {
        None
    }
}

fn count(row: &rusqlite::Row<'_>, column: usize) -> rusqlite::Result<u64> {
    let value: i64 = row.get(column)?;
    value
        .try_into()
        .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(column, value))
}
