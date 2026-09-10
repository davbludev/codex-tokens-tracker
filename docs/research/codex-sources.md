# Local Codex accounting evidence and the first adapter boundary

Research for [#2](https://github.com/davbludev/codex-tokens-tracker/issues/2), under
[spec #1](https://github.com/davbludev/codex-tokens-tracker/issues/1), observed
2026-09-07. This is a bounded local sample, not a guarantee for every Codex version.
No product code exists yet. The artifacts contain only allowlisted metadata and
usage projections; no prompt/message content, instructions, raw logs, personal
paths, real session IDs, or titles are retained.

## Sources and reproducible evidence

The configured `CODEX_HOME` was unset. The observed default is
`<user>/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`; an `archived_sessions`
directory also exists but was not sampled. Discover the configured home first,
then the default, with an explicit future path override. Directory existence
does not establish source compatibility. Do not scan unrelated databases or files.

`inspect-codex.mjs <one-rollout-path>` streams one selected source and emits only
counts, field presence, model identifiers, token categories and rate-limit
metadata. It skips content record types before parsing; it never emits or copies
raw records. Its envelope recognizer is a research aid, not the production
parser. Counts cover complete selected JSON records, not filesystem atomicity.
Run it separately for each source below. Dates/times identify local discovery
paths without publishing actual session IDs:

| Sample | Local rollout filename prefix | Evidence |
| --- | --- | --- |
| A | `2026/09/07/rollout-2026-09-07T04-25-37-` | Two completed turns; 15 modern and 15 legacy usage records; version 0.153.4. |
| B | `2026/09/07/rollout-2026-09-07T05-25-30-` | Four completed turns; 29 modern and 29 legacy records; version 0.153.4; counter-scope conflict. |
| C | `2026/09/07/rollout-2026-09-07T05-32-07-` | Active root; one started and no completed turn; 23 modern records at capture. |
| D | `2026/08/16/rollout-2026-08-16T00-33-39-` | 114 completed turns; 115 legacy observations; no modern records; version 0.148.0-alpha.9. |
| E | `2026/09/07/rollout-2026-09-07T05-34-19-` | Active child of C; independently queried for SQLite lag. |

Read-only `state_5.sqlite` schema/selected metadata queries covered `threads`,
`thread_spawn_edges`, `projects` and `project_roots`. Never select `*`: threads
also contains forbidden `first_user_message` and `preview` columns. Query only
IDs, rollout paths, usage counters, timestamps, model, project IDs and edge
metadata. `session_index.jsonl` exposes `id`, `thread_name`, `updated_at`; only
field presence was inspected, not title values. SQLite is optional enrichment,
not an accounting authority. Its filename/schema are version-specific.

## Field/source matrix

| Field | Source and observed reliability | Unavailable/limits |
| --- | --- | --- |
| Session identity | `session_meta.id` and modern `thread_id` identify the direct thread. `session_id` can identify its root session. | Do not merge all child usage under root `session_id`. Missing/conflicting IDs are diagnostics. |
| Title | `session_index.thread_name`, SQLite `threads.title/name` are potential display metadata. | No rollout title observed; enrichment is optional, may be stale, and titles were not retained. Never derive a title from messages. |
| Project/workspace | `session_meta.cwd`, `turn_context.cwd/workspace_roots`; optional `threads.project_id`, `project_roots.path`. | A cwd is an observed location, not proof of shared project identity. All observed database thread project IDs were null. |
| Time/duration | UTC envelope timestamps, metadata start, `task_started/task_complete` by turn. SQLite has seconds and optional millisecond fields. | These are observed event times, not proven server response intervals. Do not call elapsed wall time active model time. Unfinished/missing endpoints leave duration unavailable or explicitly ongoing. |
| Reasoning effort | `turn_context.effort`, observed as `low`, `medium`, `high` and `xhigh`. | Recorded per turn beside the model. Absent from turns imported before schema version 11; history is never re-imported to recover it. |
| Model | `turn_context.model` joined by turn; SQLite has a current model hint. A/B/E used `gpt-6-astra`, C `gpt-5.6-terra`, D `codex-auto-review`. | Usage records lack a model field. Missing context or model transitions without reliable linkage remain unknown; do not price earlier usage at the latest model. |
| Parent/child | `session_meta.parent_thread_id`, `source.subagent.thread_spawn.parent_thread_id`; SQLite `thread_spawn_edges`. | C→E edge matched both sources; edge status can remain open after task completion. Missing parents remain unresolved nodes, without invented usage. |
| Subagent spawn context | A subagent's rollout opens with its own `session_meta` and then replays the `session_meta` of the session that spawned it, with that parent's own `id`, `cwd` and `source`. | Observed in 113 of 1034 local rollouts, always exactly two records in that order, and always the parent the first record already named. The replay is a second identity in the file, not a second identity for the stream: usage records after it all carried the subagent's own `thread_id`. It is not an observation of the parent's own session; the parent's rollout carries that. |
| Oversized records | Rollouts contain single JSONL records far above a megabyte: 101 of them across 58 of 1034 local rollouts, from 2.1 MB to 6.7 MB. | Every one was `event_msg/item_completed` (73), `response_item` (24) or `compacted` (4) — record types that carry no usage and no rate limits. No `token_usage_record` was oversized. A reader bound must therefore skip such a record, never stop accounting for the rest of the file; usage hidden inside one would still be caught, because the records after it cannot bridge the thread's counters. |
| Tokens | Modern `usage`, `turn_token_usage`, `thread_token_usage`; legacy `event_msg/token_count.info.last_token_usage/total_token_usage`. | These are overlapping representations; no observation-to-API-call equivalence was established. |
| Weekly percentage/reset | `token_count.rate_limits.limit_id` plus either position's `window_minutes`, `used_percent`, `resets_at`. | Samples are account-wide. No independent session attribution field was found. Missing samples/reset timestamps remain unavailable. |

## Accounting observations and accepted scope

A's per-record sums are input 753397, cached input 715648, cache write 0,
output 2123, reasoning 439, total 755520. All six sums equal its final thread
counters and SQLite/legacy total. B's per-record sums are input 1187184,
cached input 1004160, cache write 0, output 7136, reasoning 186, total 1194320.
All six equal its final thread counters, but legacy and SQLite report only
315538, equal to its last turn. B's final pair is at 11:36:22.469Z/.471Z;
their matching last usage is 53160. This is a scope conflict, not merely lag.
A and B have no duplicate response IDs, negative thread differences, or
mismatches between adjacent thread differences and per-record categories.
Their turn counters reset at turn boundaries; thread counters did not.

D's final legacy total 15407529 equals SQLite. This confirms agreement in
that sample, not legacy authority across versions or turn/resume behavior.
The observed nested forms are exactly the modern three usage objects and
legacy `info.last_token_usage`/`info.total_token_usage`. No alternate standalone
`usage` record or differently nested variant was established. Unknown shapes
need a diagnostic and their own adapter evidence, never recursive extraction
of every object with token-like keys.

E at 11:51:03.447Z had modern total 2266341 (emitted 11:51:02.946Z), while
the subsequent read-only SQLite query returned 2197211, matching the preceding
legacy sample at 11:50:36.293Z. The difference 69130 equals the new record's
usage. This demonstrates active SQLite lag; the observation is not an atomic
cross-file snapshot or a bound on eventual catch-up time. An ordinary .NET
reader also failed on the active file's sharing lock; Node's shared reader
succeeded. Production Windows reading must allow concurrent writer access.

The accepted first-slice source is modern per-record `usage`, reconciled with
`thread_token_usage`. Thread/turn/legacy/SQLite values are never additional
deltas. Legacy-only usage is unavailable for this adapter, with a visible
compatibility diagnostic. This narrows #3's supported sources; historical
compatibility remains an explicit later research requirement, not silently
complete history.

Within a supported non-resetting stream, retain session identity, source
identity and ordinal/offset, source timestamp, response/turn IDs, adapter
version, category availability and cumulative endpoint. A durable unique
observation identity must survive replay/reimport. The same thread endpoint
with identical payload is a duplicate; conflicting payload is a diagnostic,
not another delta. Response IDs provide additional correlation. Conflicting
identity/provenance must not be overwritten. A decreasing endpoint, missing
reconciliation category or unexplained difference stops acceptance of that
unsupported portion; do not synthesize a reset or negative correction.
An opening record contributes its explicit usage only: never infer an opening
delta from its cumulative endpoint. Report incomplete coverage.

## Token categories and pricing ambiguity

Every modern record inspected satisfies `total = input + output`, cached
input ≤ input, and reasoning ≤ output. This supports treating cached/reasoning
as informational subsets for these observed counters, not additional tokens.
It does not independently prove provider billing semantics or applicability to
other models/versions. Cache-write values were all zero; their overlap cannot
be established from this sample. Preserve every category and missing/null
state; never convert absence to zero or add all six categories together.

For later Model Pricing, a per-model explicit reasoning interpretation can be
`included in output`, `separately priced`, or `unknown`. Unknown overlap or
cache-write semantics must leave dependent cost unavailable until configured
or supported by evidence; never guess or bill reasoning twice. This is a
boundary for #6, not pricing implementation or a claim about actual charges.
Preserve the immutable-history ADR and domain vocabulary.

## Weekly and project identity evidence

All observed rate-limit buckets were `codex`, with weekly duration 10080
minutes in `primary`; `secondary` was null. Duration, not position, identifies
weekly. Other bucket/window combinations are synthetic compatibility fixtures,
not locally confirmed shapes. Samples displayed integer percentage values
(serialized as floating-point in some logs); no sub-percent resolution was
demonstrated. Preserve source precision without implying extra accuracy.
`resets_at` was epoch seconds. Its availability is optional, not an identity.

B observed 96%, then 97%, then 0% at 11:29:54.592Z with a changed reset time.
That decrease establishes a new cycle under #1 even before the prior expected
reset. Repeated unchanged percentages have distinct observation times: they
can still establish freshness and are not all duplicates. Exact repeated
observations are idempotent. Process distinct limits separately in source-time
order; cross-file arrival order is not chronology. Reordered/duplicate samples,
missing resets and fractional precision are synthetic cases below. Equal-time
conflicting samples should remain ambiguous. Session weekly percentage impact
is unavailable; concurrent activity prevents allocating account percentages
to a session. Later USD/percent uses only comparable observations, the required
1-point denominator, and known priced usage, never a guessed allocation.

The database contained 11 projects/12 roots, including two roots under one
explicit project ID, but zero mapped threads. Several existing roots had a
`.git` directory; no `.git` worktree pointer file was found in that bounded set.
Recorded repository URL/branch/hash are hints, not enough to merge clones.
Use explicit project-root mappings when unambiguous. A future metadata-only
worktree resolver may read a selected root's `.git` pointer and its target's
`commondir`, resolve paths, and group by the common directory without invoking
Git or changing tracked repositories. That resolution is illustrated only by
a synthetic fixture; missing/inaccessible/ambiguous pointers leave roots
separate. Do not read repository contents or execute config/hooks.

## Fixtures, checks and #3 plan

[Fixture documentation](../../fixtures/codex/README.md) separates observed
projections from synthetic edge cases. The checker validates field allowlists,
anonymization, selected arithmetic, category overlap evidence and fixture
expected outcomes. It does not implement or prove database idempotence,
normalization, watcher recovery, pricing or UI behavior.

Validation performed: `node docs/research/check-fixtures.mjs` passed, both
research scripts passed `node --check`, and a focused probe regression on A
reproduced 15 modern/15 legacy observations, two completed turns, 755520 tokens
and zero category-delta mismatches. No application build, linter or product
test suite exists yet. Initial probes encountered an active-file sharing lock
and an envelope-order assumption; shared reading and recognizing the envelope
before `payload` resolved them. These were probe limitations, not missing usage.

1. Scaffold the required Tauri 2/Rust/SQLite + React/TypeScript/Vite vertical
   slice; there is no existing runtime or test harness to extend.
2. Keep discovery/checkpointed reader, pure versioned adapter, persistence and
   query/IPC boundaries separate. Decode only allowed metadata/usage fields.
   Buffer incomplete trailing records; a malformed record is a per-source
   diagnostic and must not stop unrelated files.
3. Adapter emits modern direct usage deltas and optional metadata/limit samples
   with the provenance above. Validate nonnegative integer counters and known
   source relationships. Persist delta and checkpoint atomically with durable
   duplicate/conflict detection; SQLite enrichment cannot replace usage.
4. Query one bounded direct token total and show it with freshness/coverage and
   readable unavailable/error states. Verify one live append, restart/replay,
   incomplete-tail completion and duplicate/conflict behavior. Do not implement
   later pricing, analytics, watcher hardening or tray work in #3.

Research stops here: modern evidence is sufficient for a safe first slice.
Remaining limits are legacy scope/resume compatibility, unobserved nested
forms, cache-write/category billing semantics, actual worktree-pointer
resolution, and rate-limit precision/conflicting simultaneous samples.
