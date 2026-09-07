-- Values are allowlisted projections, never raw source records.
-- Conflated historical identity/metadata halts cannot be safely classified here.
-- Preserve them and suppressed usage until an existing source-recovery trigger
-- produces a fresh generation for per-record validation. No replay is scheduled.
CREATE TABLE metadata_evidence (
    thread_id TEXT NOT NULL, kind TEXT NOT NULL, value TEXT NOT NULL,
    origin TEXT NOT NULL, turn_id TEXT NOT NULL DEFAULT '',
    source_path TEXT NOT NULL, source_generation INTEGER NOT NULL,
    source_offset INTEGER NOT NULL,
    PRIMARY KEY(thread_id,kind,value,origin,turn_id,source_path,source_generation,source_offset)
);
CREATE INDEX metadata_evidence_session ON metadata_evidence(thread_id,kind,value);
ALTER TABLE sessions ADD COLUMN parent_thread_id TEXT;
ALTER TABLE sessions ADD COLUMN parent_state TEXT NOT NULL DEFAULT 'unavailable';
ALTER TABLE sessions ADD COLUMN location_state TEXT NOT NULL DEFAULT 'unavailable';
-- Earlier adapters merged the parent candidates, so their original origin is unknown.
INSERT INTO metadata_evidence(thread_id,kind,value,origin,source_path,source_generation,source_offset)
SELECT thread_id,'parent',json_extract(metadata,'$.parent_thread_id'),'legacy_projection','',0,0
FROM sessions WHERE json_type(metadata,'$.parent_thread_id')='text' AND length(json_extract(metadata,'$.parent_thread_id'))>0;
INSERT INTO metadata_evidence(thread_id,kind,value,origin,source_path,source_generation,source_offset)
SELECT thread_id,'cwd',json_extract(metadata,'$.cwd'),'legacy_projection','',0,0
FROM sessions WHERE json_type(metadata,'$.cwd')='text' AND length(json_extract(metadata,'$.cwd'))>0;
INSERT INTO metadata_evidence(thread_id,kind,value,origin,source_path,source_generation,source_offset)
SELECT thread_id,'cli_version',json_extract(metadata,'$.cli_version'),'legacy_projection','',0,0
FROM sessions WHERE json_type(metadata,'$.cli_version')='text' AND length(json_extract(metadata,'$.cli_version'))>0;
UPDATE sessions SET parent_thread_id=(SELECT value FROM metadata_evidence e WHERE e.thread_id=sessions.thread_id AND kind='parent'),
    parent_state=CASE WHEN EXISTS(SELECT 1 FROM metadata_evidence e WHERE e.thread_id=sessions.thread_id AND kind='parent') THEN 'available' ELSE 'unavailable' END,
    location_state=CASE WHEN EXISTS(SELECT 1 FROM metadata_evidence e WHERE e.thread_id=sessions.thread_id AND kind='cwd') THEN 'available' ELSE 'unavailable' END;
PRAGMA user_version = 4;
