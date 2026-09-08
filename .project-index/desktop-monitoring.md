# Desktop monitoring

## Desktop preferences
- Owner: `src-tauri/src/desktop.rs`; symbols: `Platform`, `save_preferences`.
- Responsibility: Apply opt-in OS startup and tray changes with rollback when activation or durable save fails.
- Look here when: Changing desktop preference consistency or platform failures.

## Native desktop
- Owner: `src-tauri/src/desktop/native.rs`; entry: `src-tauri/src/lib.rs`.
- Responsibility: Tauri tray menu, window hide/open, autostart registration, and coalesced weekly refresh.
- Look here when: Changing native window/tray actions or startup integration.

## Tray metrics
- Owner: `src-tauri/src/desktop/presentation.rs`; symbol: `Summary`.
- Responsibility: Present the weekly projection with exact menu values, compact tooltips, and coverage/unavailable states.
- Look here when: Changing tray metric wording or display rules.

## Monitoring lifecycle
- Owner: `src-tauri/src/commands/monitoring.rs`; symbol: `Runtime`.
- Responsibility: Apply pause/resume and exit requests between atomic writer batches and track shutdown completion.
- Look here when: Changing catch-up, shutdown, or lifecycle checks.
