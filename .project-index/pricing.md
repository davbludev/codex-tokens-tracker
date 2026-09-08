# Model pricing

## Exact pricing rules
- Owner: `src-tauri/src/pricing.rs`; symbols: `PriceInput`, `Rates`.
- Responsibility: Validate decimal rates and value token categories using explicit overlap policies and checked integer arithmetic.
- Look here when: Changing estimated USD arithmetic or category interpretation.

## Durable price history
- Owner: `src-tauri/src/storage/pricing.rs`; migration: `src-tauri/migrations/007_model_pricing.sql`.
- Responsibility: Retain detected names, assign source-time price versions, preserve observation valuations, and process durable bounded history work through the Store writer.
- Look here when: Changing price saves, initial backfill, valuation immutability, or catalog queries.

## Pricing delivery
- Owner: `src-tauri/src/commands/pricing.rs`; shared writer inbox: `src-tauri/src/commands.rs`.
- Responsibility: Page detected models and commit validated prices through the sole writer, then wake durable pricing work; expose safe requester errors.
- Look here when: Changing pricing IPC, backpressure, or save/reply ordering.

## Pricing dialog
- Owner: `src/ModelPricing.tsx`; transport and draft validation: `src/pricing.ts`; stylesheet: `src/pricing.css`.
- Responsibility: Search all detected models and configure exact rates and overlap policies in a modal dialog with preserved drafts and explicit initial backfill. The shared catalog hook coalesces live refreshes and retains partial pages.
- Look here when: Changing pricing forms, catalog loading, or accessible feedback.

## Pricing checks
- Owner: `src-tauri/src/tests/pricing.rs`.
- Responsibility: Verify exact category arithmetic, source boundaries, model conflicts, migration, replay, resumable transactional work, and writer pricing requests.
- Look here when: Validating pricing persistence and IPC.

## Pricing browser checks
- Owner: `tests/pricing-ui.mjs`.
- Responsibility: Exercise dialog keyboard behavior, exact payloads, search, catalog retry and live discovery during paging/saving, draft preservation, and accessible errors with a controlled desktop bridge.
- Look here when: Validating the pricing UI; execution prerequisites are in `docs/model-pricing.md`.
