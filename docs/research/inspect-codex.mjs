// Bounded metadata-only research probe; never emits raw records or content.
// Usage: node docs/research/inspect-codex.mjs <one rollout JSONL path>
import { createReadStream } from 'node:fs';
import { createInterface } from 'node:readline';

const path = process.argv[2];
if (!path || process.argv.length !== 3) throw new Error('Supply one rollout path');
const categories = ['input_tokens', 'cached_input_tokens', 'cache_write_input_tokens',
  'output_tokens', 'reasoning_output_tokens', 'total_tokens'];
const pickUsage = value => Object.fromEntries(categories.map(key => [key, value?.[key] ?? null]));
const result = {
  records: {}, malformedSelectedRecords: 0, metadata: {}, models: [],
  usageRecords: 0, legacyRecords: 0, responseDuplicates: 0, turnChanges: 0,
  threadDecreases: 0, deltaMismatches: 0, categoryMismatches: 0,
  nonzeroCacheWrites: 0, sum: Object.fromEntries(categories.map(key => [key, 0])),
  firstUsage: null, lastUsage: null, lastLegacy: null, weeklySamples: [],
  turnStarts: 0, turnCompletions: 0,
};
const responses = new Set();
const models = new Set();
const limits = new Map();
let previous;
let currentModel = null;
let ordinal = 0;
for await (const line of createInterface({ input: createReadStream(path), crlfDelay: Infinity })) {
  ordinal++;
  // Skip content records before JSON parsing. The local envelope places type before payload.
  const envelope = line.slice(0, line.indexOf('"payload"'));
  const envelopeType = envelope.match(/"type"\s*:\s*"([^"]+)"/)?.[1];
  if (!envelopeType) continue;
  result.records[envelopeType] = (result.records[envelopeType] ?? 0) + 1;
  if (!['session_meta', 'turn_context', 'token_usage_record', 'event_msg'].includes(envelopeType)) continue;
  if (envelopeType === 'event_msg'
    && !/"payload"\s*:\s*\{\s*"type"\s*:\s*"(token_count|task_started|task_complete)"/.test(line)) continue;
  let row;
  try { row = JSON.parse(line); } catch { result.malformedSelectedRecords++; continue; }
  const p = row.payload;
  if (!p || typeof p !== 'object') continue;
  if (row.type === 'session_meta') {
    result.metadata = { cliVersion: p.cli_version ?? null, idPresent: !!p.id,
      rootSessionIdPresent: !!p.session_id, parentPresent: !!p.parent_thread_id,
      cwdPresent: !!p.cwd, titlePresent: !!p.title,
      gitFields: Object.keys(p.git ?? {}), timestamp: p.timestamp ?? null };
  } else if (row.type === 'turn_context') {
    currentModel = p.model ?? null;
    if (currentModel) models.add(currentModel);
  } else if (row.type === 'token_usage_record') {
    result.usageRecords++;
    if (p.response_id && responses.has(p.response_id)) result.responseDuplicates++;
    if (p.response_id) responses.add(p.response_id);
    const usage = pickUsage(p.usage);
    const thread = pickUsage(p.thread_token_usage);
    for (const key of categories) {
      if (usage[key] === null) result.sum[key] = null;
      else if (result.sum[key] !== null) result.sum[key] += usage[key];
      if (previous && thread[key] !== null && previous.thread[key] !== null) {
        if (thread[key] < previous.thread[key]) result.threadDecreases++;
        if (thread[key] - previous.thread[key] !== usage[key]) result.deltaMismatches++;
      }
    }
    if (previous && previous.turn !== p.turn_id) result.turnChanges++;
    if (usage.total_tokens !== usage.input_tokens + usage.output_tokens
      || usage.cached_input_tokens > usage.input_tokens
      || usage.reasoning_output_tokens > usage.output_tokens) result.categoryMismatches++;
    if (usage.cache_write_input_tokens > 0) result.nonzeroCacheWrites++;
    const snapshot = { ordinal, timestamp: row.timestamp, model: currentModel, usage,
      turn: pickUsage(p.turn_token_usage), thread };
    result.firstUsage ??= snapshot;
    result.lastUsage = snapshot;
    previous = { thread, turn: p.turn_id };
  } else if (row.type === 'event_msg') {
    if (p.type === 'task_started') result.turnStarts++;
    if (p.type === 'task_complete') result.turnCompletions++;
    if (p.type !== 'token_count') continue;
    result.legacyRecords++;
    if (p.info) result.lastLegacy = { ordinal, timestamp: row.timestamp,
      total: pickUsage(p.info.total_token_usage), last: pickUsage(p.info.last_token_usage) };
    const rate = p.rate_limits;
    if (!rate) continue;
    for (const position of ['primary', 'secondary']) {
      const window = rate[position];
      if (!window) continue;
      const sample = { limitId: rate.limit_id ?? null, position,
        windowMinutes: window.window_minutes ?? null, usedPercent: window.used_percent ?? null,
        resetsAt: window.resets_at ?? null };
      const key = JSON.stringify(sample);
      const existing = limits.get(key);
      if (existing) { existing.count++; existing.lastTimestamp = row.timestamp; }
      else limits.set(key, { ...sample, count: 1, firstTimestamp: row.timestamp, lastTimestamp: row.timestamp });
    }
  }
}
result.models = [...models];
result.weeklySamples = [...limits.values()];
if (!result.usageRecords) result.sum = Object.fromEntries(categories.map(key => [key, null]));
if (!Object.keys(result.records).length) throw new Error('No recognized envelopes; unsupported layout or empty source');
console.log(JSON.stringify(result, null, 2));
