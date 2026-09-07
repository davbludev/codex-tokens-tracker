# Usage aggregates

## Prepared accounting data
- Owner: `src-tauri/src/aggregates.rs`; delivery: `src-tauri/src/commands.rs`.
- Responsibility: Define bounded query requests, category and estimated-cost completeness, prepared summaries, attribution and typed delivery errors.
- Look here when: Changing the aggregate command contract or DTO serialization.

## Aggregate queries
- Owner: `src-tauri/src/storage/aggregates.rs`; migration: `src-tauri/migrations/006_aggregate_queries.sql`.
- Responsibility: Query accepted direct usage and unique effective descendants in one read transaction; sum immutable cost with checked i128 arithmetic; deliver paged sessions, children, ancestors, projects and models with full-selection totals.
- Look here when: Changing grouping, pagination, freshness, or hierarchy readiness in aggregates.

## Aggregate checks
- Owner: `src-tauri/src/tests/aggregates.rs`; detail checks: `src-tauri/src/tests/aggregates/session_detail.rs`.
- Responsibility: Verify conservation, pagination, completeness, exact sums/shares, immutable history, hierarchy readiness, placeholders/cycles, project evidence, migration and bounded timeline gaps.
- Look here when: Verifying aggregate behavior through the storage query interface.

## Session explorer queries
- Owner: `src-tauri/src/storage/aggregates/session_list.rs`; contract: `src-tauri/src/aggregates/session_list.rs`.
- Responsibility: Filter and page observed sessions by project with deterministic direct metric ordering and bounded row metadata.
- Look here when: Changing session search, date semantics, exact USD ordering, or unavailable row fields.

## Session explorer interface
- Owner: `src/Sessions.tsx`; reads: `src/sessions-data.ts`; details: `src/SessionDetail.tsx`.
- Responsibility: Render one compact project-grouped page, apply filters, serialize live refreshes, and open direct/inclusive details.
- Look here when: Changing session navigation, live paging, accessible filters, or row presentation.

## Session detail projection
- Owner: `src-tauri/src/aggregates/session_detail.rs`; wire: `src/sessions-types.ts`.
- Responsibility: Define direct model/timeline data, exact cost shares, cumulative projections and bounded gap-preserving sampling.
- Look here when: Changing session detail contracts or chart continuity.

## Session detail queries
- Owner: `src-tauri/src/storage/aggregates/session_detail.rs`.
- Responsibility: Read accepted observation bounds, paged session model summaries and chronological usage groups in the aggregate snapshot.
- Look here when: Changing detail query scopes, cost attribution, or untimed usage disclosure.
