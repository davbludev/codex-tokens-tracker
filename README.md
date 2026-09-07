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

The monitor discovers `sessions` and `archived_sessions` under `CODEX_HOME`,
or the user's `.codex` directory. Native watches are registered before bounded
historical discovery. Small read batches alternate with discovery and pending
accounting work; live events are queued and debounced during import. Progress
shows discovered files, read batches, and queued files. SQLite lives in Tauri's application
data directory. The monitor makes no external API calls and retains no prompts,
messages, or raw-log copies.

Displayed values cover observed direct usage, excluding children, for the session
with the latest parsed source observation time. Historical arrival order never
selects an older session. Query output and IPC are bounded; the one-thread token
SUM still scales with that thread's accepted history. Pricing, weekly calculations,
and broader analytics remain later tasks. Missing source directories are watched
through an existing parent and become available without restarting. Legacy-only counters are
unavailable. Unknown envelopes produce a coverage notice. Invalid modern usage
stops accounting for that source; a historical gap remains pending and can be
promoted when connecting observations arrive, preserving confirmed usage.

The [#4](https://github.com/davbludev/codex-tokens-tracker/issues/4) recovery path uses versioned migration, bounded atomic batches,
canonical timestamp ordering, and resumable promotion of connected historical
observations. Source recovery state stores complete-line offsets, tail length,
file generation/identity, size, and bounded verification digests, never raw tail
text. Replacement, truncation, and verification failures replay a new generation
without deleting confirmed usage. Readers use Windows sharing and same-handle
file identity (device/inode on Unix), plus SHA-256 of a fixed prefix and the last
observed boundary, each at most 4 KiB. This bounded fingerprint is not a full-file
integrity check: an in-place rewrite confined to an unchanged file's middle can
escape it. Incomplete tails are reconstructed from the source, with records over
2 MiB discarded until their newline; each read unit consumes at most 256 KiB of
new bytes and 64 complete records. Discovery skips symlinks and excessive nesting.
Creation, rename, watcher errors, and overflow schedule recovery; no recurring
full scan runs. A startup/recovery pass checks tracked paths in pages of 64 with
a fixed upper key; only confirmed missing files clear obsolete unfinished tails.
Permission and sharing failures retain their checkpoints. When all scheduled
work finishes, the monitor blocks on native events.

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
content-free recovery metadata, historical/live overlap, oversized tails,
same-size rewrites/replacement/truncation, archive moves, fair scheduling,
debounce/error recovery, chronological snapshot queries, and native Windows
missing-directory creation followed by live append. Tracked-source checks cover
paginated/interrupted recovery, directory removal/rename, restored files, and
permission-error classification; a native 600-file burst exercises queue overflow.
