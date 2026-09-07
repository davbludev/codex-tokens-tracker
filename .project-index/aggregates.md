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
- Owner: `src-tauri/src/tests/aggregates.rs`.
- Responsibility: Verify conservation, pagination, category/cost completeness, exact sums, immutable history, pending hierarchy, placeholders/cycles, project evidence, migration and prepared delivery.
- Look here when: Verifying aggregate behavior through the storage query interface.
