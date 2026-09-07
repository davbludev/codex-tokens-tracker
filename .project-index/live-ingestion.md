# Live ingestion

## Source reader
- Owner: `src-tauri/src/source.rs`; symbols: `Discovery`, `ingest_batch`.
- Responsibility: Traverse source trees boundedly and read atomic batches with shared file identity, fingerprints, generation recovery, and content-free tail checkpoints.
- Look here when: Changing source selection or incremental reading.

## Modern adapter and accounting
- Owner: `src-tauri/src/adapter.rs`; companion: `src-tauri/src/accounting.rs`.
- Responsibility: Project allowlisted metadata and decide chronological neighbor order and two-sided acceptance against modern thread endpoints.
- Look here when: Changing supported record shapes or acceptance rules.

## Persistence
- Owner: `src-tauri/src/storage.rs`; migrations: `src-tauri/migrations/`; symbols: `batch`, `source_state`, `restart_source`, `reconcile_pending`.
- Responsibility: Atomically commit bounded usage batches, generation/tail verification metadata, and queued chronological promotion; preserve confirmed observations and prepare one direct-session snapshot.
- Look here when: Changing recovery persistence, late-history reconciliation, or normalized storage.

## Desktop updates
- Owner: `src-tauri/src/commands.rs`; companion: `src-tauri/src/commands/runtime.rs`; entry: `src-tauri/src/lib.rs`.
- Responsibility: Parent/root native watching, debounced bounded queues, fair discovery/read/promotion/paged-presence work, recovery, and bounded progress/IPC delivery.
- Look here when: Changing live updates or runtime error reporting.

## Presentation
- Owner: `src/main.tsx`; stylesheet: `src/style.css`.
- Responsibility: Display direct tokens, observation time, coverage, and diagnostics.
- Look here when: Changing the desktop snapshot UI.

## Ingestion checks
- Owner: `src-tauri/src/tests.rs`.
- Responsibility: Accounting, chronology, migration, privacy, atomic checkpoints, recovery, import/live scheduling, and native missing-directory/append checks.
- Look here when: Verifying the supported ingestion boundary.
