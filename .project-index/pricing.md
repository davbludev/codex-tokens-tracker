# Model pricing

## Exact pricing rules
- Owner: `src-tauri/src/pricing.rs`; symbols: `PriceInput`, `Rates`.
- Responsibility: Validate decimal rates and value token categories using explicit overlap policies and checked integer arithmetic.
- Look here when: Changing estimated USD arithmetic or category interpretation.

## Durable price history
- Owner: `src-tauri/src/storage/pricing.rs`; migration: `src-tauri/migrations/007_model_pricing.sql`.
- Responsibility: Retain detected names, assign source-time price versions, preserve observation valuations, and process durable bounded history work through the Store writer.
- Look here when: Changing price saves, initial backfill, valuation immutability, or catalog queries.

## Pricing checks
- Owner: `src-tauri/src/tests/pricing.rs`.
- Responsibility: Verify exact category arithmetic, source boundaries, model conflicts, migration, replay, and resumable transactional pricing work.
- Look here when: Validating the pricing foundation.
