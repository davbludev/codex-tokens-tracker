// Checks research evidence integrity, not a production adapter implementation.
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const fixtureRoot = new URL('../../fixtures/codex/', import.meta.url);
const categories = ['input_tokens', 'cached_input_tokens', 'cache_write_input_tokens',
  'output_tokens', 'reasoning_output_tokens', 'total_tokens'];
const allowed = new Set(['timestamp', 'type', 'payload', 'id', 'session_id',
  'parent_thread_id', 'cwd', 'cli_version', 'turn_id', 'model', 'thread_id',
  'root_turn_id', 'response_id', 'usage', 'turn_token_usage', 'thread_token_usage',
  'info', 'last_token_usage', 'total_token_usage', ...categories]);
function checkProjection(value, key = '') {
  if (value === null) return;
  if (typeof value === 'object') {
    for (const [childKey, child] of Object.entries(value)) {
      assert(allowed.has(childKey), `Forbidden fixture key: ${childKey}`);
      checkProjection(child, childKey);
    }
  } else if (typeof value === 'string') {
    if (key === 'timestamp') assert.match(value, /^2026-01-01T[\d:.]+Z$/);
    else if (key === 'cwd') assert.match(value, /^C:\\example\\[a-z]+$/);
    else assert.match(value, /^[a-zA-Z0-9_.-]+$/);
    assert.doesNotMatch(value, /[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/i);
  } else {
    assert(Number.isSafeInteger(value) && value >= 0, `Invalid counter ${key}`);
  }
}
assert.throws(() => checkProjection({ prompt: 'forbidden' }), /Forbidden fixture key/);
assert.throws(() => checkProjection({ cwd: 'C:\\Users\\private' }));
async function readRecords(name) {
  const text = await readFile(new URL(name, fixtureRoot), 'utf8');
  const records = text.trimEnd().split('\n').map(line => JSON.parse(line));
  for (const record of records) {
    checkProjection(record);
    assert(['session_meta', 'turn_context', 'token_usage_record', 'event_msg'].includes(record.type));
    if (record.type === 'event_msg') assert(['token_count', 'task_started', 'task_complete'].includes(record.payload.type));
    if (record.type !== 'token_usage_record') continue;
    for (const key of ['usage', 'turn_token_usage', 'thread_token_usage']) {
      const value = record.payload[key];
      assert.deepEqual(Object.keys(value).sort(), [...categories].sort());
      assert.equal(value.total_tokens, value.input_tokens + value.output_tokens);
      assert(value.cached_input_tokens <= value.input_tokens);
      assert(value.reasoning_output_tokens <= value.output_tokens);
    }
  }
  return records;
}
const child = await readRecords('completed-child.jsonl');
const active = await readRecords('active-root.jsonl');
const conflict = await readRecords('scope-conflict.jsonl');
const childUsage = child.filter(row => row.type === 'token_usage_record').map(row => row.payload);
assert.equal(childUsage.reduce((sum, row) => sum + row.usage.total_tokens, 0), 158549);
for (const category of categories) {
  assert.equal(childUsage[2].thread_token_usage[category] - childUsage[1].thread_token_usage[category], childUsage[2].usage[category]);
}
assert.equal(childUsage[0].thread_id, child[0].payload.id);
assert.equal(childUsage[0].session_id, child[0].payload.parent_thread_id);
assert.notEqual(childUsage[0].thread_id, childUsage[0].session_id);
const mirror = child.find(row => row.payload.type === 'token_count').payload.info;
assert.deepEqual(mirror.last_token_usage, childUsage[1].usage);
assert.equal(active.at(-1).payload.usage.total_tokens, 26587);
assert(!active.some(row => row.payload.type === 'task_complete'));
assert.equal(conflict[1].payload.thread_token_usage.total_tokens, 1194320);
assert.equal(conflict[2].payload.info.total_token_usage.total_tokens, 315538);
assert.deepEqual(conflict[1].payload.turn_token_usage, conflict[2].payload.info.total_token_usage);
assert.deepEqual(conflict[1].payload.usage, conflict[2].payload.info.last_token_usage);

const edges = JSON.parse(await readFile(new URL('edge-cases.json', fixtureRoot), 'utf8'));
assert.match(edges.provenance, /^Synthetic/);
assert.doesNotMatch(JSON.stringify(edges), /[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}|[CD]:\\\\(?:Users|Projects)\\\\/i);
assert.deepEqual(edges.replay.observations[0], edges.replay.observations[1]);
assert.equal(edges.replay.expectedAcceptedUsage, 100);
assert.equal(edges.replay.expectedDuplicates, 1);
assert.equal(edges.conflict.observations[0].endpoint, edges.conflict.observations[1].endpoint);
assert.notEqual(edges.conflict.observations[0].usage, edges.conflict.observations[1].usage);
assert(edges.counterReset.observations[1].endpoint < edges.counterReset.observations[0].endpoint);
assert.equal(edges.openingGap.expectedAcceptedUsage, edges.openingGap.observation.usage);
assert.notEqual(edges.openingGap.expectedAcceptedUsage, edges.openingGap.observation.endpoint);
assert.equal(edges.missingCategory.usage.cached_input_tokens, null);
assert.throws(() => JSON.parse(edges.incompleteLine.prefix));
assert.doesNotThrow(() => JSON.parse(edges.incompleteLine.prefix + edges.incompleteLine.suffix));
const weekly = edges.weekly.samplesInArrivalOrder;
assert.deepEqual(weekly[1], weekly[2]);
assert(weekly[0].timestamp > weekly[1].timestamp);
assert.equal(weekly[0].window_minutes, weekly[1].window_minutes);
assert.notEqual(weekly[0].position, weekly[1].position);
assert(weekly[0].used_percent < weekly[1].used_percent);
assert.equal(weekly[0].resets_at, null);
assert.equal(edges.weekly.expectedCoreWeeklyUniqueObservations, 2);
assert.equal(edges.weekly.expectedResetCount, 1);
assert.equal(edges.worktrees.roots[0].commonDirectory, edges.worktrees.roots[1].commonDirectory);
assert.equal(edges.worktrees.roots[2].commonDirectory, null);
assert.equal(edges.worktrees.expectedProjectGroups, 2);
assert.equal(edges.parentChild.sessions.reduce((sum, row) => sum + row.direct, 0), 135);
assert.equal(edges.parentChild.expectedGlobal, 135);
assert.equal(edges.parentChild.expectedRootInclusive, 135);
assert.equal(edges.parentChild.expectedChildInclusive, 35);
console.log('PASS: metadata projections, counter evidence and synthetic contract-case integrity');
