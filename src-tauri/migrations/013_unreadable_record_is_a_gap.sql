-- A record too large for the bounded reader cannot be interpreted, but it also
-- cannot be assumed to have carried usage: it halted the source and rejected
-- every later record. Skip it instead, and return the retained records to
-- accounting. Where such a record did carry usage, the records after it cannot
-- bridge the thread's counters and stay unavailable on their own evidence.
UPDATE observations SET state='pending',
    diagnostic='Usage pending: historical gap or ambiguous ordering; confirmed usage retained'
WHERE accepted=0 AND state='rejected'
  AND diagnostic='Source accounting stopped after an unsupported record'
  AND source_path IN (SELECT path FROM sources
                      WHERE halted=1 AND diagnostic='Record exceeds the bounded reader limit');

UPDATE sources SET halted=0,
    diagnostic='Record exceeds the bounded reader limit; skipped without stopping accounting'
WHERE halted=1 AND diagnostic='Record exceeds the bounded reader limit';

INSERT OR IGNORE INTO reconciliation_work(observation_id)
    SELECT id FROM observations WHERE state='pending';

PRAGMA user_version=13;
