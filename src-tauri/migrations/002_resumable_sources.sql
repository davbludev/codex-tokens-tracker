ALTER TABLE sources ADD COLUMN generation INTEGER NOT NULL DEFAULT 0;
ALTER TABLE sources ADD COLUMN identity TEXT;
ALTER TABLE sources ADD COLUMN known_size INTEGER NOT NULL DEFAULT 0;
ALTER TABLE sources ADD COLUMN tail_length INTEGER NOT NULL DEFAULT 0;
ALTER TABLE sources ADD COLUMN tail_discarding INTEGER NOT NULL DEFAULT 0;
ALTER TABLE sources ADD COLUMN verification_start INTEGER NOT NULL DEFAULT 0;
ALTER TABLE sources ADD COLUMN verification_length INTEGER NOT NULL DEFAULT 0;
ALTER TABLE sources ADD COLUMN verification_hash BLOB;
ALTER TABLE observations ADD COLUMN source_generation INTEGER NOT NULL DEFAULT 0;
ALTER TABLE observations ADD COLUMN state TEXT NOT NULL DEFAULT 'rejected';
ALTER TABLE observations ADD COLUMN time_seconds INTEGER;
ALTER TABLE observations ADD COLUMN time_nanos INTEGER;
ALTER TABLE observations ADD COLUMN endpoint_order TEXT;
ALTER TABLE observations ADD COLUMN start_order TEXT;
UPDATE observations SET state='accepted' WHERE accepted=1;
UPDATE sources SET known_size=offset;
CREATE INDEX observation_chronology ON observations(thread_id,state,time_seconds,time_nanos,endpoint_order);
CREATE INDEX observation_source_order ON observations(thread_id,state,time_seconds,time_nanos,source_path,source_generation,source_offset);
CREATE INDEX observation_pending_endpoint ON observations(thread_id,state,endpoint_order);
CREATE INDEX observation_pending_start ON observations(thread_id,state,start_order,time_seconds,time_nanos);
CREATE INDEX observation_source_pending ON observations(source_path,source_generation,state);
CREATE TABLE reconciliation_work (
    observation_id INTEGER PRIMARY KEY
);
PRAGMA user_version = 2;
