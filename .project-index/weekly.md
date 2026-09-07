# Weekly observations

## Comparable intervals and cycles
- Owner: `src-tauri/src/weekly.rs`; symbols: `Timeline`, `Estimate`, `Query`.
- Responsibility: Reduce chronological quota samples into cycles and comparable segments, enforce exact percentage eligibility, and round prepared ratios.
- Look here when: Changing reset, ambiguity, recent interval, or ratio semantics.

## Weekly read projection
- Owner: `src-tauri/src/storage/weekly.rs`; symbols: `Store::read_weekly`, `Store::weekly_at`.
- Responsibility: Stream core weekly samples in canonical source order and summarize immutable interval costs in a consistent read transaction.
- Look here when: Changing weekly history queries or cost alignment.

## Weekly delivery
- Owner: `src-tauri/src/commands.rs`; symbol: `usage_weekly`.
- Responsibility: Deliver bounded query-time weekly data from a read-only blocking worker.
- Look here when: Connecting consumers to weekly estimates.

## Weekly checks
- Owner: `src-tauri/src/tests/weekly.rs`.
- Responsibility: Verify chronology, duplicate/conflict handling, replay, history pages, exact thresholds/rounding, unpriced suppression and matched cost intervals.
- Look here when: Validating weekly behavior.
