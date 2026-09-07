# Metadata-only research excerpts

Sources and limits are documented in [the research report](../../docs/research/codex-sources.md).
These are allowlisted projections, not copied raw logs. IDs, paths and dates are
replaced; token values retain the observed evidence. Omitted records mean these
are **excerpts**, not complete sessions suitable for full-history aggregation.
The active context timestamp is illustrative; do not test exact latency with it.

- `completed-child.jsonl`: sample A first response and final two consecutive
  responses, plus a mirrored legacy observation. The final thread difference is
  `755520 - 690625 = 64895`, exactly the last response. The excerpt's distinct
  response sum is `30444 + 63210 + 64895 = 158549`; it is not the complete thread
  total. `session_id=root-a` must not replace `thread_id=child-a`. Its parent is
  intentionally absent: retain the edge without manufacturing parent usage.
- `active-root.jsonl`: sample C prefix ends at the first modern usage emission,
  before a legacy observation or task completion; 26587 direct tokens remain
  observable. Absence of completion means active/unknown, not zero duration.
- `scope-conflict.jsonl`: sample B last modern/legacy pair. Legacy total is
  315538 (current turn), while the thread total is 1194320. Summing both would
  double count; replacing the thread value with SQLite would lose earlier turns.
- `edge-cases.json`: explicitly synthetic replay/conflicting endpoint, decreasing
  counters, opening gaps, legacy-only, absent categories, partial trailing JSON,
  reordered weekly samples, duplicate observations, missing reset times, other
  buckets, alternate weekly position, fractional precision, and worktree mapping.
  Its expected outcomes describe future adapter/reader guarantees; they are not
  claims that those features already exist or were all observed locally.

Replaying a response or its legacy mirror must never create another usage delta.
The repository does not yet contain an ingestion implementation; the checker
validates fixture privacy and arithmetic, not database idempotence.

Run `node docs/research/check-fixtures.mjs` from the repository root. The research
probe accepts exactly one source file at a time and emits only selected metadata:
`node docs/research/inspect-codex.mjs <rollout-path>`.
