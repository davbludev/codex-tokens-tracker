ALTER TABLE sessions ADD COLUMN is_placeholder INTEGER NOT NULL DEFAULT 0;
ALTER TABLE sessions ADD COLUMN candidate_parent TEXT;
ALTER TABLE sessions ADD COLUMN candidate_state TEXT NOT NULL DEFAULT 'unavailable';
ALTER TABLE sessions ADD COLUMN classified_revision INTEGER;
ALTER TABLE sessions ADD COLUMN location_path TEXT;
ALTER TABLE sessions ADD COLUMN repository_common_directory TEXT;
ALTER TABLE sessions ADD COLUMN repository_state TEXT NOT NULL DEFAULT 'unresolved';
CREATE INDEX metadata_parent_values ON metadata_evidence(kind,value);
CREATE TABLE hierarchy_control (
    id INTEGER PRIMARY KEY CHECK(id=1), revision INTEGER NOT NULL DEFAULT 0,
    bootstrap_after TEXT, bootstrap_through TEXT, bootstrap_done INTEGER NOT NULL DEFAULT 0,
    placeholder_after TEXT, placeholder_through TEXT, placeholders_done INTEGER NOT NULL DEFAULT 0
);
INSERT INTO hierarchy_control(id,bootstrap_through) SELECT 1,MAX(thread_id) FROM sessions;
UPDATE hierarchy_control SET placeholder_through=(SELECT MAX(value) FROM metadata_evidence WHERE kind='parent');
CREATE TABLE hierarchy_seeds (thread_id TEXT PRIMARY KEY, revision INTEGER NOT NULL);
CREATE TABLE hierarchy_job (
    id INTEGER PRIMARY KEY CHECK(id=1), seed TEXT NOT NULL, seed_revision INTEGER NOT NULL,
    revision INTEGER NOT NULL, phase TEXT NOT NULL, cursor TEXT,
    ordinal INTEGER NOT NULL DEFAULT 0, cycle_start INTEGER, result_after INTEGER NOT NULL DEFAULT -1,
    cancelled INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE hierarchy_visits (thread_id TEXT PRIMARY KEY, ordinal INTEGER NOT NULL UNIQUE);
PRAGMA user_version = 5;
