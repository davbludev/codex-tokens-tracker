# Codex usage

A local Tauri 2 desktop application showing observed direct token usage for one
identified Codex session. Rust reads metadata and usage, reconciles modern token
records, persists them in SQLite, and sends a bounded snapshot to React.

Install Node.js/npm, Rust, and the Windows Tauri prerequisites (Microsoft C++
build tools and WebView2). Then run:

```powershell
npm ci
npm run tauri -- dev
```

Build a standalone Windows executable with
`npm run tauri -- build --debug --no-bundle`; the result is
`src-tauri/target/debug/codex-tokens-tracker.exe`.

The monitor discovers `CODEX_HOME/sessions`, or the user's `.codex/sessions`.
It watches before initially reading the latest modified rollout, then reads
changed/new rollout files incrementally. SQLite lives in Tauri's application
data directory. The monitor makes no external API calls and retains no prompts,
messages, or raw-log copies.

This is the [#3](https://github.com/davbludev/codex-tokens-tracker/issues/3)
vertical slice: displayed values cover observed direct usage, excluding children.
Historical import, replacement/truncation recovery, full watcher resilience,
aggregation, pricing, and weekly calculations remain later tasks. Missing sources
require starting Codex and restarting the monitor. Legacy-only counters are
unavailable. Unknown envelopes produce a coverage notice. Invalid modern usage
stops accounting for that source; a historical gap remains pending and can be
promoted when connecting observations arrive, preserving confirmed usage.

The #4 storage foundation adds versioned migration, bounded atomic batches,
canonical timestamp ordering, and resumable promotion of connected historical
observations. Source recovery state stores complete-line offsets, tail length,
file generation/identity, size, and bounded verification digests, never raw tail
text. Reader/watch coordination does not yet use these recovery interfaces;
historical discovery, event recovery, and import progress remain the next #4 area.

Checks:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib
npm run build
node docs/research/check-fixtures.mjs
```

The Rust checks cover explicit deltas, cross-file replay, restart/checkpoints,
transaction rollback, incomplete lines, malformed sources, identity conflicts,
missing categories, native append events, and rate-limit sample precision. They
also cover late-history arrival permutations, pending duplicates and conflicts,
equal-time ordering, bounded promotion across restart, schema migration, and
content-free recovery metadata.
