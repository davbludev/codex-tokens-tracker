//! Durable bounded hierarchy work; shares the ingestion writer and reconciliation lane.
use super::{refresh_location, Result};
use crate::hierarchy::effective_state;
use rusqlite::{params, OptionalExtension, Transaction};

pub const WORK_LIMIT: usize = 32;

pub fn placeholder(tx: &Transaction<'_>, thread: &str) -> Result<()> {
    tx.execute(
        "INSERT OR IGNORE INTO sessions(thread_id,is_placeholder) VALUES(?,1)",
        [thread],
    )?;
    Ok(())
}

pub fn refresh_candidate(tx: &Transaction<'_>, thread: &str) -> Result<()> {
    let values: Vec<String> = tx.prepare("SELECT DISTINCT value FROM metadata_evidence WHERE thread_id=? AND kind='parent' ORDER BY value LIMIT 2")?
        .query_map([thread], |row| row.get(0))?.collect::<std::result::Result<_, _>>()?;
    let current_evidence: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM metadata_evidence WHERE thread_id=? AND kind='parent' AND origin!='legacy_projection')", [thread], |row| row.get(0))?;
    let state = match values.len() {
        0 => "unavailable",
        1 if current_evidence => "available",
        1 => "legacy",
        _ => "ambiguous",
    };
    let candidate = (state == "available").then(|| values[0].as_str());
    let (old, old_state): (Option<String>, String) = tx.query_row(
        "SELECT candidate_parent,candidate_state FROM sessions WHERE thread_id=?",
        [thread],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if old.as_deref() == candidate && old_state == state {
        return Ok(());
    }
    tx.execute(
        "UPDATE hierarchy_control SET revision=revision+1 WHERE id=1",
        [],
    )?;
    tx.execute(
        "UPDATE sessions SET candidate_parent=?,candidate_state=? WHERE thread_id=?",
        params![candidate, state, thread],
    )?;
    for seed in std::iter::once(thread).chain(old.as_deref()) {
        tx.execute("INSERT INTO hierarchy_seeds(thread_id,revision) SELECT ?,revision FROM hierarchy_control WHERE id=1 ON CONFLICT(thread_id) DO UPDATE SET revision=excluded.revision", [seed])?;
    }
    Ok(())
}

pub fn pending(tx: &Transaction<'_>) -> Result<bool> {
    Ok(tx.query_row("SELECT bootstrap_done=0 OR placeholders_done=0 OR EXISTS(SELECT 1 FROM hierarchy_seeds) OR EXISTS(SELECT 1 FROM hierarchy_job) FROM hierarchy_control WHERE id=1", [], |r| r.get(0))?)
}

pub fn advance(tx: &Transaction<'_>) -> Result<()> {
    for _ in 0..WORK_LIMIT {
        if bootstrap(tx)? {
            continue;
        }
        let revision: i64 = tx.query_row(
            "SELECT revision FROM hierarchy_control WHERE id=1",
            [],
            |r| r.get(0),
        )?;
        let job: Option<(String, i64, i64, String, Option<String>, i64, Option<i64>, i64, bool)> = tx.query_row(
            "SELECT seed,seed_revision,revision,phase,cursor,ordinal,cycle_start,result_after,cancelled FROM hierarchy_job WHERE id=1", [],
            |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?))).optional()?;
        let Some((
            seed,
            seed_revision,
            job_revision,
            phase,
            cursor,
            ordinal,
            cycle_start,
            result_after,
            cancelled,
        )) = job
        else {
            let seed: Option<(String, i64)> = tx
                .query_row(
                    "SELECT thread_id,revision FROM hierarchy_seeds ORDER BY thread_id LIMIT 1",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            let Some((seed, seed_revision)) = seed else {
                break;
            };
            tx.execute("INSERT INTO hierarchy_job(id,seed,seed_revision,revision,phase,cursor) VALUES(1,?,?,?,'walk',?)", params![seed,seed_revision,revision,seed])?;
            continue;
        };
        if revision != job_revision && !cancelled {
            tx.execute(
                "UPDATE hierarchy_job SET phase='cleanup',cancelled=1 WHERE id=1",
                [],
            )?;
            continue;
        }
        match phase.as_str() {
            "walk" => {
                let Some(cursor) = cursor else {
                    tx.execute("UPDATE hierarchy_job SET phase='result' WHERE id=1", [])?;
                    continue;
                };
                let seen: Option<i64> = tx
                    .query_row(
                        "SELECT ordinal FROM hierarchy_visits WHERE thread_id=?",
                        [&cursor],
                        |r| r.get(0),
                    )
                    .optional()?;
                if let Some(start) = seen {
                    tx.execute(
                        "UPDATE hierarchy_job SET phase='result',cycle_start=? WHERE id=1",
                        [start],
                    )?;
                    continue;
                }
                let (parent, classified): (Option<String>, Option<i64>) = tx.query_row(
                    "SELECT candidate_parent,classified_revision FROM sessions WHERE thread_id=?",
                    [&cursor],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?;
                if classified == Some(revision) {
                    tx.execute("UPDATE hierarchy_job SET phase='result' WHERE id=1", [])?;
                    continue;
                }
                tx.execute(
                    "INSERT INTO hierarchy_visits(thread_id,ordinal) VALUES(?,?)",
                    params![cursor, ordinal],
                )?;
                tx.execute(
                    "UPDATE hierarchy_job SET cursor=?,ordinal=ordinal+1 WHERE id=1",
                    [parent],
                )?;
            }
            "result" => {
                let next: Option<(String,i64)> = tx.query_row("SELECT thread_id,ordinal FROM hierarchy_visits WHERE ordinal>? ORDER BY ordinal LIMIT 1", [result_after], |r| Ok((r.get(0)?,r.get(1)?))).optional()?;
                let Some((thread, ordinal)) = next else {
                    tx.execute("UPDATE hierarchy_job SET phase='cleanup' WHERE id=1", [])?;
                    continue;
                };
                let (parent, state): (Option<String>, String) = tx.query_row(
                    "SELECT candidate_parent,candidate_state FROM sessions WHERE thread_id=?",
                    [&thread],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?;
                let state = effective_state(
                    &state,
                    ordinal,
                    cycle_start,
                    parent.as_deref() == Some(thread.as_str()),
                );
                let effective = (state == "available").then_some(parent).flatten();
                tx.execute("UPDATE sessions SET parent_thread_id=?,parent_state=?,classified_revision=? WHERE thread_id=?", params![effective,state,revision,thread])?;
                tx.execute(
                    "UPDATE hierarchy_job SET result_after=? WHERE id=1",
                    [ordinal],
                )?;
            }
            "cleanup" => {
                let removed = tx.execute("DELETE FROM hierarchy_visits WHERE thread_id=(SELECT thread_id FROM hierarchy_visits ORDER BY ordinal LIMIT 1)", [])?;
                if removed == 0 {
                    if !cancelled {
                        tx.execute(
                            "DELETE FROM hierarchy_seeds WHERE thread_id=? AND revision=?",
                            params![seed, seed_revision],
                        )?;
                    }
                    tx.execute("DELETE FROM hierarchy_job WHERE id=1", [])?;
                }
            }
            _ => return Err(super::Error::RecoveryMetadata),
        }
    }
    Ok(())
}

fn bootstrap(tx: &Transaction<'_>) -> Result<bool> {
    let (done, after, through, parents_done, parent_after): (bool,Option<String>,Option<String>,bool,Option<String>) = tx.query_row("SELECT bootstrap_done,bootstrap_after,bootstrap_through,placeholders_done,placeholder_after FROM hierarchy_control WHERE id=1", [], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)))?;
    // Reference placeholders first so every subsequent path lookup has a node.
    if !parents_done {
        let query = if parent_after.is_some() {
            "SELECT value FROM metadata_evidence WHERE kind='parent' AND value>?1 AND value<=(SELECT placeholder_through FROM hierarchy_control WHERE id=1) ORDER BY value LIMIT 1"
        } else {
            "SELECT value FROM metadata_evidence WHERE kind='parent' AND ?1 IS NULL AND value<=(SELECT placeholder_through FROM hierarchy_control WHERE id=1) ORDER BY value LIMIT 1"
        };
        let next: Option<String> = tx
            .query_row(query, [parent_after], |r| r.get(0))
            .optional()?;
        if let Some(parent) = next {
            placeholder(tx, &parent)?;
            tx.execute(
                "UPDATE hierarchy_control SET placeholder_after=? WHERE id=1",
                [parent],
            )?;
        } else {
            tx.execute(
                "UPDATE hierarchy_control SET placeholders_done=1 WHERE id=1",
                [],
            )?;
        }
        return Ok(true);
    }
    if !done {
        let query = if after.is_some() {
            "SELECT thread_id FROM sessions WHERE thread_id>?1 AND thread_id<=?2 ORDER BY thread_id LIMIT 1"
        } else {
            "SELECT thread_id FROM sessions WHERE ?1 IS NULL AND thread_id<=?2 ORDER BY thread_id LIMIT 1"
        };
        let next: Option<String> = tx
            .query_row(query, params![after, through], |r| r.get(0))
            .optional()?;
        if let Some(thread) = next {
            refresh_candidate(tx, &thread)?;
            refresh_location(tx, &thread)?;
            tx.execute(
                "UPDATE hierarchy_control SET bootstrap_after=? WHERE id=1",
                [thread],
            )?;
        } else {
            tx.execute(
                "UPDATE hierarchy_control SET bootstrap_done=1 WHERE id=1",
                [],
            )?;
        }
        return Ok(true);
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Store;
    use std::collections::{BTreeMap, BTreeSet};

    fn mutate(store: &mut Store, thread: &str, parent: Option<&str>) {
        let tx = store.connection.transaction().unwrap();
        tx.execute(
            "INSERT OR IGNORE INTO sessions(thread_id) VALUES(?)",
            [thread],
        )
        .unwrap();
        // A controlled evidence replacement exercises both candidate insertion and removal.
        // Production ingestion retains all evidence and removes candidates through ambiguity.
        tx.execute(
            "DELETE FROM metadata_evidence WHERE thread_id=? AND kind='parent'",
            [thread],
        )
        .unwrap();
        if let Some(parent) = parent {
            placeholder(&tx, parent).unwrap();
            tx.execute("INSERT INTO metadata_evidence(thread_id,kind,value,origin,source_path,source_generation,source_offset) VALUES(?,'parent',?,'session_meta.parent_thread_id','test',0,0)",params![thread,parent]).unwrap();
        }
        refresh_candidate(&tx, thread).unwrap();
        tx.commit().unwrap();
    }

    fn settle(store: &mut Store) {
        for _ in 0..10000 {
            if !store.reconcile_pending().unwrap() {
                return;
            }
        }
        panic!("hierarchy did not settle");
    }

    // Whole-graph reference deliberately uses an independent per-node reachability test.
    fn compare(store: &mut Store, graph: &BTreeMap<String, Option<String>>) {
        settle(store);
        for (node, parent) in graph {
            let mut seen = BTreeSet::new();
            let mut cursor = parent.as_ref();
            let mut cycle_member = false;
            while let Some(next) = cursor {
                if next == node {
                    cycle_member = true;
                    break;
                }
                if !seen.insert(next) {
                    break;
                }
                cursor = graph.get(next).and_then(|value| value.as_ref());
            }
            let expected = if cycle_member { None } else { parent.clone() };
            assert_eq!(
                store.effective_parent(node).unwrap(),
                expected,
                "node {node}"
            );
        }
    }

    #[test]
    fn hierarchy_reference_matches_long_chains_fanout_and_generated_mutations() {
        let temp = tempfile::tempdir().unwrap();
        let db = temp.path().join("graph.sqlite");
        let mut store = Store::open(&db).unwrap();
        let mut graph = BTreeMap::new();
        for n in 0..90 {
            let node = format!("n{n:03}");
            let parent = (n < 89).then(|| format!("n{:03}", n + 1));
            mutate(&mut store, &node, parent.as_deref());
            graph.insert(node, parent);
        }
        assert_eq!(
            store.effective_parent("n000"),
            Err(crate::hierarchy::ReadError::HierarchyPending)
        );
        compare(&mut store, &graph);
        for n in 0..25 {
            let node = format!("fan{n:03}");
            mutate(&mut store, &node, Some("n020"));
            graph.insert(node, Some("n020".into()));
        }
        // Cycle appears, breaks, reappears; incoming descendants retain their own edges.
        for parent in [Some("n030"), None, Some("n030"), Some("n089"), None] {
            mutate(&mut store, "n089", parent);
            graph.insert("n089".into(), parent.map(str::to_owned));
            compare(&mut store, &graph);
        }
        let mut random = 17u64;
        for step in 0..180 {
            random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
            let node = format!("n{:03}", (random >> 32) % 90);
            let parent = (step % 7 != 0).then(|| format!("n{:03}", (random >> 16) % 90));
            mutate(&mut store, &node, parent.as_deref());
            graph.insert(node, parent);
            // Change evidence while persisted traversal/result/cleanup may be active.
            store.reconcile_pending().unwrap();
            if step % 9 == 0 {
                drop(store);
                store = Store::open(&db).unwrap();
            }
            if step % 13 == 0 {
                compare(&mut store, &graph);
            }
        }
        compare(&mut store, &graph);
    }

    #[test]
    fn hierarchy_cycle_orders_and_duplicate_candidates_are_stable() {
        for order in [
            [0, 1, 2],
            [2, 1, 0],
            [1, 2, 0],
            [1, 0, 2],
            [0, 2, 1],
            [2, 0, 1],
        ] {
            let temp = tempfile::tempdir().unwrap();
            let mut store = Store::open(&temp.path().join("order.sqlite")).unwrap();
            let mut graph = BTreeMap::new();
            for n in order {
                let node = format!("n{n}");
                let parent = format!("n{}", (n + 1) % 3);
                mutate(&mut store, &node, Some(&parent));
                graph.insert(node, parent.into());
                compare(&mut store, &graph);
            }
            mutate(&mut store, "child", Some("n0"));
            graph.insert("child".into(), Some("n0".into()));
            compare(&mut store, &graph);
            let revision: i64 = store
                .connection
                .query_row("SELECT revision FROM hierarchy_control", [], |r| r.get(0))
                .unwrap();
            mutate(&mut store, "child", Some("n0"));
            assert_eq!(
                store
                    .connection
                    .query_row::<i64, _, _>("SELECT revision FROM hierarchy_control", [], |r| r
                        .get(0))
                    .unwrap(),
                revision
            );
            assert_eq!(store.effective_parent("child").unwrap(), Some("n0".into()));
        }
    }

    #[test]
    fn hierarchy_restart_and_rollback_cover_each_durable_phase() {
        let temp = tempfile::tempdir().unwrap();
        let db = temp.path().join("restart.sqlite");
        let mut store = Store::open(&db).unwrap();
        for n in 0..80 {
            mutate(
                &mut store,
                &format!("n{n:03}"),
                Some(&format!("n{:03}", (n + 1) % 80)),
            );
        }
        let mut phases = BTreeSet::new();
        for _ in 0..1000 {
            let phase: Option<String> = store
                .connection
                .query_row("SELECT phase FROM hierarchy_job", [], |r| r.get(0))
                .optional()
                .unwrap();
            if let Some(phase) = phase {
                phases.insert(phase);
                let before: (String, i64, i64) = store
                    .connection
                    .query_row(
                        "SELECT phase,ordinal,result_after FROM hierarchy_job",
                        [],
                        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                    )
                    .unwrap();
                // Simulated transaction failure: dropping uncommitted work preserves the checkpoint.
                let tx = store.connection.transaction().unwrap();
                advance(&tx).unwrap();
                drop(tx);
                let after: (String, i64, i64) = store
                    .connection
                    .query_row(
                        "SELECT phase,ordinal,result_after FROM hierarchy_job",
                        [],
                        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                    )
                    .unwrap();
                assert_eq!(before, after);
            }
            let more = store.reconcile_pending().unwrap();
            drop(store);
            store = Store::open(&db).unwrap();
            if !more {
                break;
            }
        }
        assert_eq!(
            phases,
            BTreeSet::from(["walk".into(), "result".into(), "cleanup".into()])
        );
        assert_eq!(store.effective_parent("n000").unwrap(), None);
        assert_eq!(
            store
                .connection
                .query_row::<i64, _, _>("SELECT COUNT(*) FROM hierarchy_visits", [], |r| r.get(0))
                .unwrap(),
            0
        );
    }

    #[test]
    fn hierarchy_mutation_cancels_each_phase_without_losing_dirty_seed() {
        for target in ["walk", "result", "cleanup"] {
            let temp = tempfile::tempdir().unwrap();
            let db = temp.path().join("mutation.sqlite");
            let mut store = Store::open(&db).unwrap();
            let mut graph = BTreeMap::new();
            for n in 0..90 {
                let node = format!("n{n:03}");
                let parent = format!("n{:03}", (n + 1) % 90);
                mutate(&mut store, &node, Some(&parent));
                graph.insert(node, Some(parent));
            }
            let mut reached = false;
            for _ in 0..100 {
                store.reconcile_pending().unwrap();
                let phase: Option<String> = store
                    .connection
                    .query_row("SELECT phase FROM hierarchy_job", [], |r| r.get(0))
                    .optional()
                    .unwrap();
                if phase.as_deref() == Some(target) {
                    reached = true;
                    break;
                }
            }
            assert!(reached, "phase {target}");
            // Mutate the active seed itself: conditional deletion must retain this new revision.
            mutate(&mut store, "n000", None);
            graph.insert("n000".into(), None);
            assert_eq!(
                store.effective_parent("n050"),
                Err(crate::hierarchy::ReadError::HierarchyPending)
            );
            store.reconcile_pending().unwrap();
            drop(store);
            let mut store = Store::open(&db).unwrap();
            compare(&mut store, &graph);
        }
    }
}
