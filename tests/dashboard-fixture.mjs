// Realistic dashboard payload for design review screenshots. Not a correctness
// fixture: `dashboard-ui.mjs` keeps its own minimal contract data.
export function installDashboardFixture() {
  const time = seconds => ({ seconds, nanos: 0 });
  // Anchored to the viewer's clock so observation ages and axis dates stay real.
  const now = Math.floor(Date.now() / 1000 / 1800) * 1800;
  const cost = (knownSubtotal, complete = true, acceptedObservations = 12) => ({ knownSubtotal, complete, acceptedObservations });
  const category = (value, complete = true) => ({ knownTokens: value === null ? null : String(value), complete });
  const tokens = (input, cached, writes, output, reasoning) => ({
    totalTokens: category(input + output), inputTokens: category(input), cachedInputTokens: category(cached),
    cacheWriteTokens: category(writes), outputTokens: category(output), reasoningTokens: category(reasoning),
  });
  // Trillionths of USD.
  const usd = value => String(BigInt(Math.round(value * 1e6)) * 1000000n);

  const bins = 96;
  const start = now - 7 * 86400;
  const width = (now - start) / bins;
  const points = [];
  let totalInput = 0, totalCached = 0, totalWrites = 0, totalOutput = 0, totalReasoning = 0, totalCost = 0;
  let seed = 7;
  const random = () => (seed = (seed * 1103515245 + 12345) % 2147483648) / 2147483648;
  for (let index = 0; index < bins; index++) {
    const hour = new Date((start + index * width) * 1000).getUTCHours();
    const active = hour >= 7 && hour <= 22 && random() > 0.28;
    if (!active) continue;
    const input = Math.round(120000 + random() * 900000);
    const cached = Math.round(input * (0.55 + random() * 0.3));
    const writes = Math.round(input * 0.08 * random());
    const output = Math.round(4000 + random() * 40000);
    const reasoning = Math.round(output * (0.3 + random() * 0.4));
    totalInput += input; totalCached += cached; totalWrites += writes; totalOutput += output; totalReasoning += reasoning;
    const amount = (input - cached) * 1.25e-6 + cached * 0.125e-6 + writes * 1.55e-6 + output * 10e-6;
    totalCost += amount;
    points.push({
      index, start: time(Math.round(start + index * width)), end: time(Math.round(start + (index + 1) * width)),
      tokens: tokens(input, cached, writes, output, reasoning), estimatedCost: cost(usd(amount)),
      observedSessions: 1 + Math.round(random() * 3),
    });
  }

  const observationPercent = "63.4";
  const estimate = (from, to, points_, amount) => ({
    start: time(from), end: time(to), consumedPercentagePoints: String(points_), estimatedCost: cost(usd(amount)),
    effectiveUsdPerPercent: (amount / points_).toFixed(6), estimatedFullWeekUsd: (amount / points_ * 100).toFixed(6), unavailableReason: null,
  });

  const models = [
    { key: "model:gpt-5.6-codex", label: "gpt-5.6-codex", share: 0.52 },
    { key: "model:gpt-5.6-codex-mini", label: "gpt-5.6-codex-mini", share: 0.21 },
    { key: "model:gpt-5.3-codex", label: "gpt-5.3-codex", share: 0.14 },
    { key: "model:o5-mini", label: "o5-mini", share: 0.08 },
    { key: "model:gpt-4.9-turbo", label: "gpt-4.9-turbo", share: 0.05 },
  ];
  const breakdown = (key, label, kind, share, complete = true) => ({
    key, label, kind, tokens: category(Math.round((totalInput + totalOutput) * share), complete),
    estimatedCost: cost(usd(totalCost * share), complete),
  });
  const modelBreakdowns = [
    ...models.map(model => breakdown(model.key, model.label, "model", model.share)),
    breakdown("other:", "Other models", "other", 0.008),
    breakdown("unknown:", "Unknown model", "unknown", 0.002, false),
  ];
  const projects = [["codex-tokens-tracker", 0.44], ["glival", 0.23], ["portfolio-site", 0.15], ["scratchpad", 0.09], ["infra-scripts", 0.06]];
  const projectBreakdowns = [
    ...projects.map(([label, share]) => breakdown("repository:D:/Projects/" + label + "/.git", label, "project", share)),
    breakdown("other:", "Other projects", "other", 0.02),
    breakdown("unknown:", "Unattributed project", "unknown", 0.01, false),
  ];

  const modelCosts = models.map((model, position) => {
    const input = Math.round(totalInput * model.share), cached = Math.round(totalCached * model.share);
    const writes = Math.round(totalWrites * model.share), output = Math.round(totalOutput * model.share);
    const rate = [1, 0.25, 3.5, 0.6, 0.9][position];
    const inputUsd = (input - cached) * 1.25e-6 * rate, cachedUsd = cached * 0.125e-6 * rate;
    const writeUsd = writes * 1.55e-6 * rate, outputUsd = output * 10e-6 * rate;
    return {
      key: model.key, label: model.label, kind: "model",
      tokens: tokens(input, cached, writes, output, Math.round(totalReasoning * model.share)),
      estimatedCost: cost(usd(inputUsd + cachedUsd + writeUsd + outputUsd)),
      categories: { input: usd(inputUsd), cachedInput: usd(cachedUsd), cacheWrites: usd(writeUsd), output: usd(outputUsd), reason: null },
      acceptedObservations: 40 + position * 17, observedSessions: 12 - position * 2,
    };
  });
  modelCosts.push({
    key: "unknown:", label: "Unknown model", kind: "unknown",
    tokens: tokens(Math.round(totalInput * 0.002), Math.round(totalCached * 0.002), 0, Math.round(totalOutput * 0.002), 0),
    estimatedCost: cost(null, false, 3),
    categories: { input: null, cachedInput: null, cacheWrites: null, output: null, reason: "Unpriced usage: no applicable model price" },
    acceptedObservations: 3, observedSessions: 1,
  });

  // The per-model prices differ from the flat rate used to build the bins, so
  // the recorded range cost is rescaled to what the models actually add up to.
  const modelTotal = modelCosts.reduce((sum, row) => sum + Number(row.estimatedCost.knownSubtotal ?? 0) / 1e12, 0);
  for (const point of points) point.estimatedCost = cost(usd(Number(point.estimatedCost.knownSubtotal) / 1e12 * modelTotal / totalCost));
  const summary = { tokens: tokens(totalInput, totalCached, totalWrites, totalOutput, totalReasoning), estimatedCost: cost(usd(modelTotal), false), observedSessions: 38 };

  // Turns by model and reasoning, aligned to the same bins as the usage charts
  // and to the per-model totals, so the table foots to the Cost by model total.
  // Share of a model's work, and how many tokens a turn at that effort takes.
  const efforts = [["high", 0.34, 96000], ["medium", 0.4, 61000], ["low", 0.26, 38000]];
  const allocate = (total, weights) => {
    const whole = weights.reduce((sum, weight) => sum + weight, 0) || 1;
    let used = 0n;
    return weights.map((weight, index) => {
      const part = index === weights.length - 1
        ? (total - used > 0n ? total - used : 0n)
        : BigInt(Math.round(Number(total) * weight / whole));
      used += part;
      return part;
    });
  };
  const combinations = models.flatMap((model, position) => efforts.map(([effort, weight, perTurn]) => ({
    key: `model:${model.label}|effort:${effort}`, label: `${model.label} · ${effort}`,
    model: model.label, effort, kind: "combination", weight, perTurn,
    tokens: BigInt(Math.round(Number(modelCosts[position].tokens.totalTokens.knownTokens) * weight)),
    cost: BigInt(Math.round(Number(modelCosts[position].estimatedCost.knownSubtotal) * weight)),
  })));
  combinations.sort((left, right) => Number(right.tokens - left.tokens));
  const rest = combinations.slice(7);
  const ordered = [
    ...combinations.slice(0, 7),
    {
      key: "other:", label: `Other combinations (${rest.length})`, model: null, effort: null, kind: "other", perTurn: 64000,
      tokens: rest.reduce((sum, row) => sum + row.tokens, 0n), cost: rest.reduce((sum, row) => sum + row.cost, 0n),
    },
    {
      key: "unknown:|effort:", label: "Unknown model · reasoning unavailable", model: null, effort: null, kind: "unattributed", perTurn: 24000,
      tokens: BigInt(modelCosts.at(-1).tokens.totalTokens.knownTokens), cost: null,
    },
  ];
  const turnSeries = ordered.map((row, index) => {
    // Turn counts follow the same busy profile as the token bins.
    const busy = points.map(point => 0.35 + Math.abs(Math.sin(point.index * (1.3 + index * 0.37))));
    const shares = allocate(row.tokens, busy);
    const amounts = row.cost === null ? busy.map(() => null) : allocate(row.cost, busy);
    // A plausible number of turns for the tokens the combination carried.
    const turns = Math.max(1, Math.round(Number(row.tokens) / row.perTurn));
    const perBin = allocate(BigInt(turns), busy).map(Number);
    return {
      key: row.key, label: row.label, model: row.model, effort: row.effort, kind: row.kind, turns,
      acceptedObservations: turns + Math.round(turns * 0.35),
      observedSessions: row.kind === "other" ? null : Math.max(1, Math.round(turns / 7)),
      tokens: category(String(row.tokens)),
      estimatedCost: cost(row.cost === null ? null : String(row.cost), row.cost !== null),
      points: points.map((point, position) => ({
        index: point.index, turns: perBin[position], tokens: category(String(shares[position])),
        estimatedCost: cost(amounts[position] === null ? null : String(amounts[position]), amounts[position] !== null),
      })),
    };
  });
  const turnActivity = {
    start: time(start), end: time(now), binCount: bins,
    totalTurns: turnSeries.reduce((sum, row) => sum + row.turns, 0),
    combinations: combinations.length + 1,
    turnsWithoutIdentity: 2,
    series: turnSeries,
    coverageNote: "A turn is one exchange with the model, counted once where it first appears in this range, with all of its tokens and cost counted there too. Reasoning effort comes from the turn's own context; turns recorded before this application started tracking it report it as unavailable. A session can run turns of several combinations, so the folded remainder reports no session count rather than a wrong one.",
  };

  // One set of per-interval component amounts drives the categories, the
  // hypotheses, the weekly ratio and the weekly chart, so every panel that
  // shows "per 1%" agrees. "All three" counts cached input, cache writes and
  // reasoning on top of uncached input and visible output, so it equals the
  // interval's total cost. Each component wobbles independently, so the
  // hypothesis comparison has something to compare.
  const components = { input: 0.425, inputWithWrites: 0.4, visibleOutput: 0.23, cached: 0.07, writes: 0.062, reasoning: 0.15 };
  const consumedPoints = n => (1.2 + n * 0.35).toFixed(3);
  const componentAmounts = n => {
    const scale = Number(consumedPoints(n)) * 0.8;
    const wobble = offset => 1 + 0.22 * Math.sin(n * 1.7 + offset);
    return Object.fromEntries(Object.entries(components).map(([key, value], index) => [key, value * scale * wobble(index * 1.1)]));
  };
  const interval = n => {
    const consumed = consumedPoints(n);
    const parts = componentAmounts(n);
    const scale = Number(consumed) * 0.8 * (1 + 0.18 * Math.sin(n * 2.1));
    const end = now - (8 - n) * 43200;
    return {
      start: time(end - 43200), end: time(end), consumedPercentagePoints: consumed,
      tokens: tokens(Math.round(900000 * scale), Math.round(560000 * scale), Math.round(40000 * scale), Math.round(38000 * scale), Math.round(15000 * scale)),
      hypotheses: Array.from({ length: 16 }, (_, index) => {
        const mask = index % 8, included = index >= 8;
        const optional = (mask & 1 ? parts.cached : 0) + (mask & 2 ? parts.writes : 0) + (mask & 4 ? parts.reasoning : 0);
        const base = (included ? parts.inputWithWrites : parts.input) + parts.visibleOutput;
        const share = (included ? 0.86 : 0.9) + (mask & 1 ? 0.055 : 0) + (mask & 2 ? 0.04 : 0) + (mask & 4 ? 0.015 : 0);
        return { mask, writesIncluded: included, tokens: String(Math.round(1500000 * scale * share)), estimatedUsd: usd(base + optional), tokenReason: null, priceReason: null };
      }),
      categories: {
        input: usd(parts.input), cachedInput: usd(parts.cached), cacheWrites: usd(parts.writes),
        output: usd(parts.visibleOutput + parts.reasoning), reason: null,
      },
    };
  };
  const intervals = Array.from({ length: 9 }, (_, n) => interval(n));
  const comparablePoints = intervals.reduce((sum, row) => sum + Number(row.consumedPercentagePoints), 0);
  const comparableCost = intervals.reduce((sum, row) => sum + Object.values(componentAmounts(intervals.indexOf(row))).reduce((total, value) => total + value, 0) - componentAmounts(intervals.indexOf(row)).inputWithWrites, 0);
  const usdPerPoint = comparableCost / comparablePoints;
  // The device joined this cycle partway through, so the charted segment starts
  // where its first trustworthy observation did.
  const chartStart = now - 9 * 43200;
  const openingPercent = Number(observationPercent) - comparablePoints;
  const observation = { time: time(now - 240), usedPercent: observationPercent, remainingPercent: (100 - Number(observationPercent)).toFixed(1), resetsAt: now + 3 * 86400 };
  const first = { time: time(chartStart), usedPercent: openingPercent.toFixed(3), remainingPercent: (100 - openingPercent).toFixed(3), resetsAt: now + 3 * 86400 };

  const response = {
    evaluatedAt: time(now), tokenScope: "All locally observed history; direct session usage counted once.",
    weekly: {
      evaluatedAt: time(now),
      currentCycle: { key: "cycle", firstObservation: first, lastObservation: observation, detectedReset: false, hasAmbiguousObservations: false, fullCycleCostKnown: false },
      observationAgeSeconds: 240,
      overall: estimate(chartStart, now, comparablePoints, comparableCost),
      recent: estimate(now - 900, now, 0.9, 0.72),
      unmatchedCost: cost(usd(0.42)), unmatchedCostStart: time(now), history: [], nextCursor: null,
      excludedSamples: 2, sessionWeeklyPercentageImpact: null,
      coverageNote: "Local observations only; account-wide usage may exceed what this device recorded.",
    },
    global: {
      tokens: summary.tokens, estimatedCost: summary.estimatedCost,
      coverage: { incompleteSessions: 2, unavailableSessions: 0, unresolvedUsage: false, unknownModel: true, unattributedProject: true, sourceDiagnostics: false },
      observedAt: null, observedSessions: 38, placeholders: 1,
    },
    localUsage: {
      start: time(start), end: time(now), binCount: bins, summary, points, untimedObservations: 4,
      coverageNote: "Accepted direct local usage in start-exclusive/end-inclusive bins.",
    },
    breakdowns: {
      metric: "tokens", models: modelBreakdowns, projects: projectBreakdowns, modelCosts,
      categoryTotals: Object.fromEntries([...["input", "cachedInput", "cacheWrites", "output"].map(key =>
        [key, String(modelCosts.reduce((sum, row) => sum + BigInt(row.categories[key] ?? "0"), 0n))]), ["reason", null]]),
    },
    chart: {
      range: "last7Days", start: time(chartStart), end: time(now), binCount: 64,
      returnedObservationCount: 24, sourceObservationCount: 24, coverageNote: "Since observation began.",
      points: Array.from({ length: 24 }, (_, index) => {
        const fraction = index / 23;
        return {
          time: time(Math.round(chartStart + fraction * (now - chartStart))),
          segmentId: "a", weeklyUsedPercent: (openingPercent + fraction * comparablePoints).toFixed(3),
          cumulativeEstimatedCost: cost(usd(fraction * comparableCost)),
          effectiveUsdPerPercent: index === 0 ? null : usdPerPoint.toFixed(6),
          unavailableReason: index === 0 ? "insufficientObservations" : null,
          connectFromPrevious: index > 0,
        };
      }),
      boundaries: [{ binIndex: 0, firstTime: time(chartStart), lastTime: time(chartStart), count: 1, kinds: ["observationStart"], overloaded: false }],
    },
    turnActivity,
    quotaAnalysis: { intervals, totalIntervals: intervals.length },
  };

  const state = window.dashboardTest = { calls: [], callbacks: {}, listeners: new Set(), response };
  state.callbacks.broadcast = event => { for (const handler of state.listeners) state.callbacks[handler](event); };
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: (_event, id) => state.listeners.delete(id) };
  window.__TAURI_INTERNALS__ = {
    transformCallback: callback => { const id = Object.keys(state.callbacks).length + 1; state.callbacks[id] = callback; return id; },
    invoke: async (command, args) => {
      if (command === "plugin:event|listen") { state.listeners.add(args.handler); return args.handler; }
      if (command === "usage_snapshot") return { threadId: null, directTokens: null, observedAt: null, sourceAvailable: true, coverage: "Preview fixture", diagnostic: null };
      if (command === "pricing_models") return { models: [], nextCursor: null };
      if (command !== "usage_dashboard") return null;
      state.calls.push(args.query);
      const result = structuredClone(state.response);
      result.chart.range = args.query.range;
      result.breakdowns.metric = args.query.breakdownMetric ?? "tokens";
      return result;
    },
  };
}
