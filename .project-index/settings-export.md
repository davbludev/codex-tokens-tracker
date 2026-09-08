# Settings and export

## Source preferences
- Owner: `src-tauri/src/settings.rs`; symbols: `Config`, `resolve`, `validate_override`.
- Responsibility: Resolve automatic and saved Codex homes and validate new overrides and dependent preferences.
- Look here when: Changing directory selection or saved preference contracts.

## Durable preferences and diagnostics
- Owner: `src-tauri/src/storage/settings.rs`; migration: `src-tauri/migrations/008_tracker_settings.sql`.
- Responsibility: Save preferences on the ingestion writer and read database size, observed-session/usage counts, and the last committed complete-line ingestion time.
- Look here when: Changing settings persistence or import/resume diagnostics.

## Desktop settings delivery
- Owner: `src-tauri/src/commands/settings.rs`; companion: `src-tauri/src/commands.rs`.
- Responsibility: Prepare replacement source watches before saving and switching, expose diagnostics, and admit/drain background CSV work through shutdown.
- Look here when: Changing live source switching or settings/export commands.

## CSV reports
- Owner: `src-tauri/src/export.rs`; checks: `src-tauri/src/export/tests.rs`.
- Responsibility: Stream sessions, models, projects, and weekly cycles from one read snapshot with exact costs, explicit coverage, and CSV escaping.
- Look here when: Changing exported columns, file delivery, or bounded report generation.

## Settings interface
- Owner: `src/Settings.tsx`; transport: `src/settings-data.ts`; stylesheet: `src/settings.css`; checks: `tests/settings-ui.mjs`.
- Responsibility: Edit source and active startup/tray preferences, open pricing, inspect diagnostics, and request CSV exports with accessible outcomes.
- Look here when: Changing Settings workflows or frontend command contracts.
