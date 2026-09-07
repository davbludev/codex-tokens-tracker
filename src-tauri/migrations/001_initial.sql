PRAGMA journal_mode = WAL;
CREATE TABLE IF NOT EXISTS sources (
    path TEXT PRIMARY KEY, offset INTEGER NOT NULL DEFAULT 0, ordinal INTEGER NOT NULL DEFAULT 0,
    thread_id TEXT, halted INTEGER NOT NULL DEFAULT 0, diagnostic TEXT,
    partial INTEGER NOT NULL DEFAULT 0, legacy INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS sessions (
    thread_id TEXT PRIMARY KEY, metadata TEXT, incomplete INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS turn_contexts (
    thread_id TEXT NOT NULL, turn_id TEXT NOT NULL, model TEXT,
    PRIMARY KEY(thread_id, turn_id)
);
CREATE TABLE IF NOT EXISTS observations (
    id INTEGER PRIMARY KEY, thread_id TEXT NOT NULL, endpoint TEXT NOT NULL,
    response_id TEXT, timestamp TEXT NOT NULL, normalized TEXT NOT NULL,
    adapter TEXT NOT NULL, source_path TEXT NOT NULL, source_offset INTEGER NOT NULL,
    source_ordinal INTEGER NOT NULL, model TEXT, accepted INTEGER NOT NULL,
    total INTEGER, diagnostic TEXT
);
CREATE INDEX IF NOT EXISTS observation_endpoint ON observations(thread_id, endpoint);
CREATE INDEX IF NOT EXISTS observation_response ON observations(thread_id, response_id);
CREATE INDEX IF NOT EXISTS observation_session ON observations(thread_id, accepted, id);
CREATE TABLE IF NOT EXISTS limit_samples (
    id INTEGER PRIMARY KEY, bucket TEXT, position TEXT NOT NULL, window_minutes INTEGER,
    timestamp TEXT, used_percent TEXT, resets_at INTEGER,
    normalized TEXT NOT NULL, source_path TEXT NOT NULL, source_offset INTEGER NOT NULL,
    adapter TEXT NOT NULL, UNIQUE(normalized)
);
PRAGMA user_version = 1;
