# Project and model analytics

## Prepared analytics contract
- Owner: `src-tauri/src/aggregates/analytics.rs`; registration: `src-tauri/src/aggregates.rs`; frontend wire: `src/analytics-types.ts`.
- Responsibility: Define direct project/model metrics, exact averages and global shares, observed cycle usage, bounded model previews and sparse history bins.
- Look here when: Changing analytics query or response shapes.

## Analytics persistence
- Owner: `src-tauri/src/storage/aggregates/analytics.rs`; checks: `src-tauri/src/tests/aggregates/analytics.rs`.
- Responsibility: Read bounded project/model pages and history in SQLite with immutable valuation, current pricing, classification and interval coverage.
- Look here when: Changing analytics aggregation or conservation guarantees.

## Comparison views
- Owner: `src/Projects.tsx`, `src/Models.tsx`; route: `src/main.tsx`.
- Responsibility: Display compact direct comparisons, unavailable metrics, paged project models, pricing and history navigation.
- Look here when: Changing project/model comparison behavior.

## Analytics presentation helpers
- Owner: `src/analytics-data.ts`, `src/AnalyticsControls.tsx`, `src/analytics.css`; live reads: [shared aggregate reads](session-details-ui.md#live-aggregate-reads).
- Responsibility: Format exact prepared averages/shares and provide common read states and paging controls.
- Look here when: Changing analytics disclosures or controls.

## Model history
- Owner: `src/ModelHistory.tsx`.
- Responsibility: Show one bounded fixed interval of sparse, non-cumulative bins with independent chart scales and exact keyboard readouts.
- Look here when: Changing range selection, plot cleanup or history inspection.

## Browser checks
- Owner: `tests/analytics-ui.mjs`.
- Responsibility: Verify comparison labels, bounded paging, live replacement, keyboard dialogs and responsive layout through mocked IPC.
- Look here when: Checking frontend analytics integration.
