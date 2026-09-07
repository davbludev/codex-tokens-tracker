# Weekly observations

## Comparable intervals and cycles
- Owner: `src-tauri/src/weekly.rs`; symbols: `Timeline`, `Estimate`, `HistoricalCycle`, `ModelsQuery`.
- Responsibility: Reduce chronological quota samples into cycles and comparable segments, enforce exact percentage eligibility, and round prepared ratios.
- Look here when: Changing reset, ambiguity, recent interval, or ratio semantics.

## Weekly read projection
- Owner: `src-tauri/src/storage/weekly.rs`; symbols: `Store::read_weekly`, `Store::read_weekly_models`.
- Responsibility: Stream core weekly samples chronologically and project immutable comparable interval costs, tokens and bounded observed-model pages in consistent read transactions.
- Look here when: Changing weekly history queries or cost alignment.

## Weekly delivery
- Owner: `src-tauri/src/commands.rs`; symbols: `usage_weekly`, `usage_weekly_models`.
- Responsibility: Deliver bounded query-time weekly data from a read-only blocking worker.
- Look here when: Connecting consumers to weekly estimates.

## Weekly checks
- Owner: `src-tauri/src/tests/weekly.rs`.
- Responsibility: Verify chronology, duplicates/conflicts, replay, pricing immutability, bounded history/models, exact thresholds and matched cost/token intervals including real metadata fixtures.
- Look here when: Validating weekly behavior.

## Weekly History interface
- Owner: `src/WeeklyHistory.tsx`, `src/weekly-data.ts`, `src/weekly.css`; symbol: `useWeeklyHistory`.
- Responsibility: Compare bounded completed cycles and one selected model page with exact prepared values, coverage labels and serialized live invalidation.
- Look here when: Changing history navigation, selection, paging or observation disclosures; shared weekly DTOs live in `src/dashboard-types.ts`.

## Weekly browser checks
- Owner: `tests/weekly-ui.mjs`.
- Responsibility: Exercise app navigation, interval displays, paging, pending selection/live changes, keyboard access, narrow layouts and failure states using mocked IPC.
- Look here when: Verifying the Weekly History interface.
