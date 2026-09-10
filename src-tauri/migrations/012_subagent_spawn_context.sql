-- A subagent's rollout replays the metadata of the session that spawned it.
-- That second session_meta was read as a conflicting identity, which halted the
-- source and rejected every later record of the subagent's own usage. Clear
-- exactly those halts, where the same source had already recorded that identity
-- as the stream's parent, and return their retained records to accounting. The
-- acceptance rules, not this migration, decide what is counted.
UPDATE observations SET state='pending',
    diagnostic='Usage pending: historical gap or ambiguous ordering; confirmed usage retained'
WHERE accepted=0 AND state='rejected'
  AND diagnostic='Source accounting stopped after an unsupported record'
  AND source_path IN (
    SELECT s.path FROM sources s
    WHERE s.halted=1 AND s.diagnostic='Conflicting direct session identity'
      AND EXISTS(SELECT 1 FROM metadata_evidence e
                 WHERE e.thread_id=s.thread_id AND e.kind='parent' AND e.source_path=s.path));

UPDATE sources SET halted=0,
    diagnostic='Usage pending: historical gap or ambiguous ordering; confirmed usage retained'
WHERE halted=1 AND diagnostic='Conflicting direct session identity'
  AND EXISTS(SELECT 1 FROM metadata_evidence e
             WHERE e.thread_id=sources.thread_id AND e.kind='parent' AND e.source_path=sources.path);

INSERT OR IGNORE INTO reconciliation_work(observation_id)
    SELECT id FROM observations WHERE state='pending';

PRAGMA user_version=12;
