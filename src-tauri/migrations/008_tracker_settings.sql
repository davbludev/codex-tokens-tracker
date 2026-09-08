CREATE TABLE IF NOT EXISTS tracker_settings (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    codex_directory_override TEXT,
    autostart INTEGER NOT NULL DEFAULT 0 CHECK (autostart IN (0, 1)),
    tray_enabled INTEGER NOT NULL DEFAULT 0 CHECK (tray_enabled IN (0, 1)),
    close_to_tray INTEGER NOT NULL DEFAULT 0 CHECK (close_to_tray IN (0, 1)),
    CHECK (close_to_tray = 0 OR tray_enabled = 1)
);
INSERT OR IGNORE INTO tracker_settings(id) VALUES (1);
CREATE TABLE IF NOT EXISTS ingestion_status (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    last_successful_ingestion_at_ms INTEGER
);
INSERT OR IGNORE INTO ingestion_status(id) VALUES (1);
PRAGMA user_version = 8;
