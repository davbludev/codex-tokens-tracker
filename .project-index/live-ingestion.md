# Live ingestion

## Source reader
- Owner: `src-tauri/src/source.rs`; symbols: `sessions_directory`, `latest_rollout`, `ingest`.
- Responsibility: Discover Codex rollouts and read bounded complete lines with shared Windows access and persisted offsets.
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
- Owner: `src-tauri/src/commands.rs`; entry: `src-tauri/src/lib.rs`.
- Responsibility: Native event orchestration and bounded Tauri command/event delivery.
- Look here when: Changing live updates or runtime error reporting.

## Presentation
- Owner: `src/main.tsx`; stylesheet: `src/style.css`.
- Responsibility: Display direct tokens, observation time, coverage, and diagnostics.
- Look here when: Changing the desktop snapshot UI.

## Ingestion checks
- Owner: `src-tauri/src/tests.rs`.
- Responsibility: Fixture accounting, late-arrival permutations, bounded promotion/restart, migration, recovery metadata/privacy, transaction rollback, and native append checks.
- Look here when: Verifying the supported ingestion boundary.
