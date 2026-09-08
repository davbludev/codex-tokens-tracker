// Focused dashboard checks against the native bridge contract, independent of app mounting.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { pathToFileURL } from "node:url";
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE ? pathToFileURL(process.env.PLAYWRIGHT_MODULE).href : "playwright");
const server = spawn(process.execPath, ["node_modules/vite/bin/vite.js", "--host", "127.0.0.1", "--port", "1422", "--strictPort"], { windowsHide: true, stdio: "pipe" });
let browser;
try {
  await new Promise((resolve, reject) => {
    const deadline = setTimeout(() => reject(new Error("Vite did not start")), 15000);
    server.stdout.on("data", data => { if (data.toString().includes("1422")) { clearTimeout(deadline); resolve(); } });
    server.on("exit", code => { clearTimeout(deadline); reject(new Error(`Vite exited ${code}`)); });
  });
  browser = await chromium.launch({ channel: "msedge", headless: true });
  const page = await browser.newPage({ viewport: { width: 1200, height: 800 } });
  const failures = [];
  page.on("pageerror", error => { failures.push(error.message); console.error(error.message); });
  // Observe the native chart contract without adding a production test API.
  await page.route("**/node_modules/.vite/deps/uplot.js*", async route => {
    const response = await route.fetch();
    const source = await response.text();
    assert.ok(source.includes("export { uPlot as default };"));
    await route.fulfill({ response, body: source.replace("export { uPlot as default };", "const ObservedPlot = new Proxy(uPlot, { construct(target, args) { const plot = Reflect.construct(target, args); if (plot.series.some(series => series.scale === 'weekly')) window.dashboardPlot = plot; if (args[2]?.classList.contains('quota-analysis-plot')) window.hypothesisPlot = plot; if (args[2]?.classList.contains('quota-categories-plot')) window.categoryPlot = plot; return plot; } }); export { ObservedPlot as default };") });
  });
  if (!process.env.DASHBOARD_APP_MOUNT) await page.route("http://127.0.0.1:1422/", route => route.fulfill({ contentType: "text/html", body: '<html><div id="root"></div><script type="module">import React from "/node_modules/.vite/deps/react.js"; import ReactDOM from "/node_modules/.vite/deps/react-dom_client.js"; import {Dashboard} from "/src/Dashboard.tsx"; import "/src/style.css"; ReactDOM.createRoot(document.getElementById("root")).render(React.createElement(Dashboard));</script></html>' }));
  await page.addInitScript(() => {
    const NativePath = window.Path2D;
    window.Path2D = class extends NativePath {
      moves = 0; lines = 0;
      moveTo(...args) { this.moves++; super.moveTo(...args); }
      lineTo(...args) { this.lines++; super.lineTo(...args); }
    };
    const time = seconds => ({ seconds, nanos: 123456789 });
    const cost = (knownSubtotal = "1234567890123", complete = true) => ({ knownSubtotal, complete, acceptedObservations: 2 });
    const estimate = { start: time(1800000000), end: time(1800000900), consumedPercentagePoints: "2.000000001", estimatedCost: cost(), effectiveUsdPerPercent: "0.617283945", estimatedFullWeekUsd: "61.7283945", unavailableReason: null };
    const observation = { time: time(1800000900), usedPercent: "42.000000001", remainingPercent: "57.999999999", resetsAt: 1800600000 };
    const category = { knownTokens: "9007199254740993", complete: true };
    const tokens = (total, input = total, output = 0) => Object.fromEntries(["totalTokens", "inputTokens", "cachedInputTokens", "cacheWriteTokens", "reasoningTokens", "outputTokens"].map(key => [key, { knownTokens: String(key === "totalTokens" ? total : key === "inputTokens" ? input : key === "outputTokens" ? output : 0), complete: true }]));
    const summary = { tokens: tokens(12451993, 11203000, 1248993), estimatedCost: cost("1234567890123"), observedSessions: 24 };
    const breakdown = (key, label, kind, total, knownSubtotal) => ({ key, label, kind, tokens: { knownTokens: String(total), complete: true }, estimatedCost: cost(knownSubtotal) });
    const response = {
      evaluatedAt: time(1800001000), tokenScope: "locallyObservedGlobalAllHistory",
      weekly: { evaluatedAt: time(1800001000), currentCycle: { key: "cycle", firstObservation: observation, lastObservation: observation, detectedReset: false, hasAmbiguousObservations: false, fullCycleCostKnown: false }, observationAgeSeconds: 100, overall: estimate, recent: estimate, unmatchedCost: cost("500000000000"), unmatchedCostStart: time(1800000900), history: [], nextCursor: null, excludedSamples: 0, sessionWeeklyPercentageImpact: null, coverageNote: "Local observations only." },
      global: { tokens: Object.fromEntries(["totalTokens", "inputTokens", "cachedInputTokens", "cacheWriteTokens", "reasoningTokens", "outputTokens"].map(key => [key, category])), estimatedCost: cost(), coverage: { incompleteSessions: 1, unavailableSessions: 0, unresolvedUsage: true, unknownModel: false, unattributedProject: false, sourceDiagnostics: false }, observedAt: null, observedSessions: 2, placeholders: 0 },
      localUsage: { start: time(1799994000), end: time(1800001000), binCount: 10, summary, untimedObservations: 2, coverageNote: "Each accepted direct session observation is counted once. Untimed usage is excluded from the chart.", points: [
        { index: 0, start: time(1799994000), end: time(1799994700), tokens: tokens(1200000, 1000000, 200000), estimatedCost: cost("100000000000"), observedSessions: 3 },
        { index: 2, start: time(1799995400), end: time(1799996100), tokens: tokens(2400000, 2200000, 200000), estimatedCost: cost("234567890123"), observedSessions: 6 },
        { index: 5, start: time(1799997500), end: time(1799998200), tokens: tokens(5400000, 5000000, 400000), estimatedCost: cost("600000000000"), observedSessions: 8 },
        { index: 8, start: time(1799999600), end: time(1800000300), tokens: tokens(3451993, 3003000, 448993), estimatedCost: cost("300000000000"), observedSessions: 7 },
      ] },
      breakdowns: { metric: "tokens", models: [
        breakdown("astra", "gpt-6-astra", "model", 6000000, "500000000000"), breakdown("sol", "gpt-5.6-sol", "model", 2400000, "300000000000"), breakdown("terra", "gpt-5.6-terra", "model", 1800000, "200000000000"), breakdown("luna", "gpt-5.6-luna", "model", 1200000, "150000000000"), breakdown("future", "future-model", "model", 600000, "70000000000"), breakdown("other", "Other models", "other", 400000, "14000000000"), breakdown("unknown", "Unknown model", "unknown", 51993, "567890123"),
      ], projects: [
        breakdown("tracker", "codex-tokens-tracker", "project", 6000000, "600000000000"), breakdown("website", "Website", "project", 2400000, "300000000000"), breakdown("tools", "CLI tools", "project", 1800000, "200000000000"), breakdown("docs", "Documentation", "project", 1200000, "80000000000"), breakdown("prototype", "Prototype", "project", 600000, "40000000000"), breakdown("other", "Other projects", "other", 400000, "14000000000"), breakdown("unknown", "Unattributed project", "unknown", 51993, "567890123"),
      ] },
      chart: { range: "currentCycle", start: time(1800000000), end: time(1800001000), binCount: 10, returnedObservationCount: 3, sourceObservationCount: 3, coverageNote: "Accepted observation segments.",
        points: [
          { time: time(1800000000), segmentId: "a", weeklyUsedPercent: "40", cumulativeEstimatedCost: cost("0"), effectiveUsdPerPercent: null, unavailableReason: "belowOnePercentagePoint", connectFromPrevious: false },
          { time: time(1800000900), segmentId: "a", weeklyUsedPercent: "42.000000001", cumulativeEstimatedCost: cost(), effectiveUsdPerPercent: "0.617283945", unavailableReason: null, connectFromPrevious: true },
          { time: time(1800001000), segmentId: "b", weeklyUsedPercent: "1", cumulativeEstimatedCost: cost("0"), effectiveUsdPerPercent: null, unavailableReason: "insufficientObservations", connectFromPrevious: false },
        ], boundaries: [{ binIndex: 9, firstTime: time(1800001000), lastTime: time(1800001000), count: 1, kinds: ["reset"], overloaded: false }],
      },
    };
    response.quotaAnalysis = { totalIntervals: 3, intervals: [1, 2, 3].map(n => ({ start: time(1800000000 + n * 100), end: time(1800000050 + n * 100), consumedPercentagePoints: String(n), tokens: tokens(160), hypotheses: Array.from({ length: 16 }, (_, index) => ({ mask: index % 8, writesIncluded: index >= 8, tokens: String((115 + (index % 8 & 1 ? 20 : 0) + (index % 8 & 2 ? 10 : 0) + (index % 8 & 4 ? 15 : 0) - (index >= 8 ? 10 : 0)) * n), estimatedUsd: String(BigInt((300 + (index % 8 & 1 ? 10 : 0) + (index % 8 & 2 ? 30 : 0) + (index % 8 & 4 ? 90 : 0) - (index >= 8 ? 20 : 0)) * n) * 1000000n), tokenReason: null, priceReason: null })), categories: n === 3 ? { input: null, cachedInput: null, cacheWrites: null, output: null, reason: "No local usage observations" } : { input: String(100000000000n * BigInt(n)), cachedInput: String(20000000000n * BigInt(n)), cacheWrites: String(30000000000n * BigInt(n)), output: String(50000000000n * BigInt(n)), reason: null } })) };
    const state = window.dashboardTest = { calls: [], callbacks: {}, listeners: new Set(), listener: "broadcast", active: 0, maxActive: 0, hold: false, releases: [], fail: false, response };
    state.callbacks.broadcast = event => { for (const handler of state.listeners) state.callbacks[handler](event); };
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: (_event, id) => state.listeners.delete(id) };
    window.__TAURI_INTERNALS__ = {
      transformCallback: callback => { const id = Object.keys(state.callbacks).length + 1; state.callbacks[id] = callback; return id; },
      invoke: async (command, args) => {
        if (command === "plugin:event|listen") { state.listeners.add(args.handler); return args.handler; }
        if (command === "usage_snapshot") return { sourceAvailable: true, coverage: "Fixture local usage", diagnostic: null };
        if (command === "pricing_models") return { models: [], nextCursor: null };
        if (command !== "usage_dashboard") return null;
        state.calls.push(args.query); state.active++; state.maxActive = Math.max(state.maxActive, state.active);
        const result = structuredClone(state.response); result.chart.range = args.query.range; result.breakdowns.metric = args.query.breakdownMetric;
        try {
          if (state.hold) await new Promise(resolve => state.releases.push(resolve));
          if (state.fail) throw "storage";
          return result;
        } finally { state.active--; }
      },
    };
  });
  await page.clock.install();
  await page.goto("http://127.0.0.1:1422");
  await page.locator(".dashboard-summary").waitFor();
  assert.equal(await page.getByRole("button", { name: "7 days", exact: true }).getAttribute("aria-pressed"), "true", "dashboard defaults to seven days");
  assert.equal(await page.locator(".dashboard-summary .dashboard-metric").count(), 4);
  await page.waitForFunction(() => document.querySelectorAll(".usage-chart .uplot").length === 2);
  assert.equal(await page.locator(".weekly-details").getAttribute("open"), null, "coverage details start collapsed");
  assert.equal(await page.locator(".dashboard-history-details").count(), 0);
  assert.equal(await page.locator(".hypothesis-method").getAttribute("open"), null);
  const cards = page.locator(".hypothesis-card");
  assert.equal(await cards.count(), 8);
  assert.deepEqual(await cards.locator(".hypothesis-badges").evaluateAll(elements => elements.map(el => el.textContent)), ["InputOutput", "InputOutputCached input", "InputOutputCache writes", "InputOutputReasoning", "InputOutputCached inputCache writes", "InputOutputCached inputReasoning", "InputOutputCache writesReasoning", "InputOutputCached inputCache writesReasoning"]);
  await page.waitForFunction(() => window.hypothesisPlot?.series.length === 9);
  await page.waitForFunction(() => window.categoryPlot?.series.length === 5, undefined, { timeout: 10000 });
  assert.deepEqual(await page.evaluate(() => [window.categoryPlot.data[1][0], window.categoryPlot.data[4][0], window.categoryPlot.data[1][2]]), [0.2, 0.1, null], "stacked running totals per observed 1%, top series first; an interval without local usage draws no bar");
  assert.deepEqual(await page.evaluate(() => window.categoryPlot.series.slice(1).map(s => s.label)), ["Output", "Cache writes", "Cached input", "Input"]);
  const categoryCards = page.locator(".category-card");
  assert.equal(await categoryCards.count(), 4);
  assert.match(await categoryCards.first().innerText(), /Input[\s\S]*\$0.100000[\s\S]*50%[\s\S]*\$10.000000/, "input is half of every interval's cost");
  assert.match(await categoryCards.nth(3).innerText(), /Output[\s\S]*\$0.050000[\s\S]*25%/);
  assert.match(await page.locator(".category-total").innerText(), /\$0.200000 USD \/ 1% · \$20.000000 USD \/ 100%/);
  assert.equal(await page.locator(".quota-categories .hypothesis-unavailable").count(), 0, "intervals without local usage never block the statistics");
  await page.locator(".quota-categories").getByText("2 / 3 priced intervals", { exact: true }).waitFor();
  assert.match(await page.locator(".quota-categories > p").last().innerText(), /over the 2 priced intervals; 1 with no local usage excluded/);
  await page.locator(".quota-categories-plot").focus(); await page.keyboard.press("End");
  assert.match(await page.locator(".category-tooltip").innerText(), /\+3 percentage points · No local usage observations[\s\S]*Cached input[\s\S]*Unavailable USD \/ 1%/);
  await page.keyboard.press("ArrowLeft");
  assert.match(await page.locator(".category-tooltip").innerText(), /\+2 percentage points[\s\S]*Cached input[\s\S]*\$0.020000 USD \/ 1%/);
  const weighted = await page.evaluate(async () => {
    const { combinationStats, perPercent } = await import("/src/quota-combinations.ts");
    const base = structuredClone(window.dashboardTest.response.quotaAnalysis.intervals.slice(0, 2));
    base[0].consumedPercentagePoints = "1"; base[1].consumedPercentagePoints = "3";
    for (const [index, tokens] of ["100", "900"].entries()) {
      base[index].hypotheses[0].tokens = tokens;
      base[index].hypotheses[0].estimatedUsd = String(BigInt(tokens) * 1000000000000n);
    }
    const complete = combinationStats(base, 0, "additional");
    base[1].hypotheses[0].estimatedUsd = null;
    base[1].hypotheses[0].priceReason = "Unpriced usage";
    const partial = combinationStats(base, 0, "additional");
    return { complete, partial, exact: perPercent("9007199254740993", "1") };
  });
  assert.equal(weighted.complete.tokens, "250.00", "weighted by observed percentage points, not mean of ratios");
  assert.equal(weighted.complete.usd, "250.000000");
  assert.equal(weighted.complete.fullUsd, "25000.000000");
  assert.equal(weighted.partial.tokens, "250.00");
  assert.equal(weighted.partial.usd, null, "a priced subset must not masquerade as a complete average");
  assert.equal(weighted.partial.fullUsd, null);
  assert.equal(weighted.exact, "9007199254740993.00");
  const axisLabels = await page.evaluate(() => window.hypothesisPlot.axes[1].values(window.hypothesisPlot, [0.0003, 0.0004]));
  assert.deepEqual(axisLabels, ["3.00e-4", "4.00e-4"], "small monetary axis labels remain distinguishable");
  assert.equal(await page.locator(".quota-analysis .uplot").count(), 1);
  assert.match(await cards.first().innerText(), /115.00/);
  assert.match(await cards.first().innerText(), /\$0.000300/);
  assert.match(await cards.first().innerText(), /\$0.030000/);
  const seriesBefore = await page.evaluate(() => window.hypothesisPlot.series.slice(1).map(s => ({ label: s.label, color: s.stroke(), noPath: s.paths() === null })));
  assert.ok(seriesBefore.every(s => s.noPath), "independent intervals are points, never connected across gaps");
  const hypothesisMetric = page.getByRole("group", { name: "Hypothesis chart metric" });
  await hypothesisMetric.getByRole("button", { name: "Tokens / 1%", exact: true }).focus();
  await page.keyboard.press("Enter");
  await page.waitForFunction(() => window.hypothesisPlot.data[1][0] === 115);
  const checkbox = cards.first().getByRole("checkbox");
  await checkbox.focus(); await page.keyboard.press("Space");
  await page.waitForFunction(() => window.hypothesisPlot.series.length === 8);
  assert.deepEqual(await page.evaluate(() => window.hypothesisPlot.series.slice(1).map(s => ({ label: s.label, color: s.stroke(), noPath: s.paths() === null }))), seriesBefore.slice(1), "series colors remain stable when another series is hidden");
  await page.keyboard.press("Space");
  await page.waitForFunction(() => window.hypothesisPlot.series.length === 9);
  await hypothesisMetric.getByRole("button", { name: "USD / 1%", exact: true }).click();
  await page.waitForFunction(() => window.hypothesisPlot.data[1][0] === 0.0003);
  await page.locator(".quota-analysis-plot").focus(); await page.keyboard.press("Home");
  assert.match(await page.locator(".hypothesis-tooltip").innerText(), /\+1 percentage points/);
  assert.match(await page.locator(".hypothesis-tooltip").innerText(), /115.00 tokens \/ 1%/);
  await page.keyboard.press("ArrowRight");
  assert.match(await page.locator(".hypothesis-tooltip").innerText(), /\+2 percentage points/);
  await page.locator(".hypothesis-method > summary").click();
  await page.getByLabel("Cache-write interpretation").selectOption("included");
  assert.match(await cards.first().innerText(), /105.00/);
  assert.match(await cards.first().innerText(), /\$0.000280/);
  await page.getByLabel("Cache-write interpretation").selectOption("additional");
  await page.locator(".hypothesis-method > summary").click();
  assert.match(await page.locator(".dashboard-coverage").innerText(), /2 observations have no timestamp/);
  for (const [title, unknown, remainder] of [["By model", "Unknown model", "Other models"], ["By project", "Unattributed project", "Other projects"]]) {
    const panel = page.getByRole("region", { name: title, exact: true });
    assert.equal(await panel.locator(".breakdown-list > li").count(), 7, "bounded top five plus remainder and unattributed rows");
    await panel.getByText(unknown, { exact: true }).waitFor();
    await panel.getByText(remainder, { exact: true }).waitFor();
  }
  if (process.env.DASHBOARD_APP_MOUNT) {
    const nav = page.getByRole("navigation", { name: "Usage views" });
    const names = ["Dashboard", "Sessions", "Projects", "Models", "Weekly History", "Settings"];
    assert.equal(await nav.getByRole("button").count(), names.length);
    for (const name of names) {
      const button = nav.getByRole("button", { name, exact: true });
      await button.focus();
      assert.equal(await button.evaluate(el => el === document.activeElement), true, name + " navigation is keyboard reachable");
      assert.equal(await button.getAttribute("aria-pressed"), String(name === "Dashboard"));
    }
    const pricingTrigger = page.getByRole("button", { name: "Model Pricing", exact: true });
    await pricingTrigger.click();
    const dialog = page.getByRole("dialog", { name: "Model Pricing", exact: true });
    await dialog.waitFor();
    await dialog.getByText("0 detected models", { exact: true }).waitFor();
    await dialog.getByRole("button", { name: "Close", exact: true }).click();
    await dialog.waitFor({ state: "hidden" });
    assert.equal(await pricingTrigger.evaluate(el => el === document.activeElement), true, "pricing close restores shell focus");
  }
  await page.locator(".dashboard-toolbar").scrollIntoViewIfNeeded();
  if (process.env.DASHBOARD_SCREENSHOT) await page.screenshot({ path: process.env.DASHBOARD_SCREENSHOT, fullPage: true });
  if (process.env.DASHBOARD_SCREENSHOT) await page.locator(".quota-analysis").screenshot({ path: process.env.DASHBOARD_SCREENSHOT.replace(/\.png$/, "-comparison.png") });
  const wide = await page.locator(".usage-chart .uplot").evaluateAll(elements => elements.map(el => el.clientWidth));
  await page.setViewportSize({ width: 390, height: 850 });
  await page.waitForFunction(widths => Array.from(document.querySelectorAll(".usage-chart .uplot")).every((el, index) => el.clientWidth < widths[index]), wide);
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true, "dashboard fits narrow viewport");
  if (process.env.DASHBOARD_SCREENSHOT) await page.screenshot({ path: process.env.DASHBOARD_SCREENSHOT.replace(/\.png$/, "-narrow.png"), fullPage: true });
  await page.evaluate(() => {
    const s = window.dashboardTest;
    for (const interval of s.response.quotaAnalysis.intervals) {
      interval.hypotheses[0].tokens = "900719925474099312345";
      interval.hypotheses[0].estimatedUsd = null;
      interval.hypotheses[0].priceReason = "Unpriced usage: no applicable model price";
      interval.hypotheses[1].tokens = null;
      interval.hypotheses[1].estimatedUsd = null;
      interval.hypotheses[1].tokenReason = "Invalid input subset subtraction";
      interval.categories = { input: null, cachedInput: null, cacheWrites: null, output: null, reason: "Unpriced usage: no applicable model price" };
    }
    s.response.breakdowns.models[0].label = "long-model-name-with-a-very-long-version-and-localized-identifier-123456789";
    s.callbacks[s.listener]({ payload: {} });
  });
  await page.clock.runFor(200);
  await cards.first().getByText(/USD unavailable — Unpriced usage/).waitFor();
  await cards.nth(1).getByText(/Unavailable — Invalid input subset subtraction/).waitFor();
  await page.locator(".quota-categories").getByText(/USD unavailable: no comparable priced intervals/).waitFor();
  assert.equal(await page.locator(".quota-categories .uplot").count(), 0, "no chart when every interval is unpriced");
  assert.match(await categoryCards.first().innerText(), /Unavailable/);
  assert.match(await page.locator(".quota-categories .hypothesis-unavailable").innerText(), /USD unavailable — Unpriced usage: no applicable model price \(0 \/ 3 priced\)/);
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true, "large exact numbers, unavailable reasons and long model names fit 390px: " + JSON.stringify(await page.locator("body *").evaluateAll(elements => elements.filter(el => el.getBoundingClientRect().right > innerWidth + 1).map(el => ({ tag: el.tagName, class: el.className, width: el.getBoundingClientRect().width })).slice(0, 12))));
  await page.setViewportSize({ width: 1200, height: 800 });
  await page.locator(".weekly-details > summary").click();
  await page.getByText("42.000000001% / 57.999999999%", { exact: true }).waitFor();
  await page.getByText("Since observation began · partial cycle", { exact: true }).waitFor();
  assert.match(await page.locator(".dashboard-unmatched").innerText(), /\$0.5/);
  const scales = await page.evaluate(() => {
    const plot = window.dashboardPlot;
    const path = plot.series[1].paths(plot, 1, 0, 2).stroke;
    return { names: plot.series.slice(1).map(s => s.scale), weekly: plot.scales.weekly.max, cost: plot.scales.cost.max, ratio: plot.scales.ratio.max, moves: path.moves, lines: path.lines };
  });
  assert.deepEqual(scales.names, ["weekly", "cost", "ratio"]);
  assert.equal(scales.weekly, 100);
  assert.ok(scales.cost > 1.234567890123 && scales.cost < 10);
  assert.ok(scales.ratio > 0.617283945 && scales.ratio < scales.cost);
  assert.equal(scales.moves, 2, "reset starts a new path");
  assert.equal(scales.lines, 1, "only observations within the segment connect");
  for (const kind of ["tokens", "cost"]) {
    const chart = page.locator(".usage-chart-" + kind);
    await chart.locator(".chart-inspector > summary").click();
    const control = chart.getByRole("slider");
    await control.focus(); await page.keyboard.press("Home"); await page.keyboard.press("ArrowRight");
    const readout = await chart.locator(".chart-inspector [role=status]").innerText();
    assert.match(readout, /\.123456789Z/, "local chart inspection preserves exact time");
    assert.match(readout, kind === "tokens" ? /2,400,000/ : /\$0.234567890123/, "local chart keyboard inspection preserves exact native amounts");
    await page.keyboard.press("End");
    assert.match(await chart.locator(".chart-inspector [role=status]").innerText(), kind === "tokens" ? /3,451,993/ : /\$0.3/);
  }
  await page.locator(".quota-inspector > summary").click();
  const slider = page.getByLabel("Inspect observation", { exact: false });
  await slider.focus(); await page.keyboard.press("Home"); await page.keyboard.press("ArrowRight");
  assert.match(await page.locator("#dashboard-selected").innerText(), /\$1.234567890123/);
  assert.match(await page.locator("#dashboard-selected").innerText(), /\.123456789Z/);
  assert.match(await page.locator("#dashboard-selected").innerText(), /42.000000001%/);
  await page.keyboard.press("End");
  assert.match(await page.locator("#dashboard-selected").innerText(), /observation boundary; no connection/);
  await page.locator(".dashboard-plot .u-over").hover({ position: { x: 20, y: 50 } });
  await page.locator(".dashboard-tooltip").waitFor();
  assert.match(await page.locator(".dashboard-tooltip").innerText(), /\.123456789Z/);
  await page.mouse.move(0, 0);
  const beforeBurst = await page.evaluate(() => { const s = window.dashboardTest; const count = s.calls.length; for (let i = 0; i < 20; i++) s.callbacks[s.listener]({ payload: {} }); return count; });
  await page.clock.runFor(200);
  assert.equal(await page.evaluate(() => window.dashboardTest.calls.length), beforeBurst + 1, "event burst coalesces into one read");
  await page.waitForFunction(() => window.dashboardTest.active === 0);
  const beforeMinute = await page.evaluate(() => window.dashboardTest.calls.length);
  await page.clock.runFor(60_000);
  assert.equal(await page.evaluate(() => window.dashboardTest.calls.length), beforeMinute + 1, "rolling estimates refresh once per minute");
  await page.evaluate(() => { const s = window.dashboardTest; s.hold = true; s.callbacks[s.listener]({ payload: {} }); });
  await page.clock.runFor(200);
  await page.waitForFunction(() => window.dashboardTest.active === 1);
  const currentRange = page.getByRole("button", { name: "7 days", exact: true });
  await currentRange.focus();
  await page.keyboard.press("Enter");
  assert.equal(await currentRange.evaluate(el => el === document.activeElement), true, "reselecting the active range preserves focus while loading");
  const dayRange = page.getByRole("button", { name: "24 hours", exact: true });
  await dayRange.focus();
  await page.keyboard.press("Enter");
  assert.equal(await dayRange.evaluate(el => el === document.activeElement), true, "switching range preserves keyboard focus while loading");
  const allRange = page.getByRole("button", { name: "All", exact: true });
  await allRange.focus();
  await page.keyboard.press("Enter");
  await page.evaluate(() => { const s = window.dashboardTest; for (let i = 0; i < 20; i++) s.callbacks[s.listener]({ payload: {} }); });
  await page.clock.runFor(200);
  await page.evaluate(() => { window.dashboardTest.releases.splice(0).forEach(resolve => resolve()); });
  await page.waitForFunction(() => window.dashboardTest.calls.at(-1).range === "all" && window.dashboardTest.releases.length === 1);
  assert.equal(await page.locator(".dashboard-metrics").count(), 0, "old range response cannot repopulate the dashboard");
  await page.evaluate(() => { const s = window.dashboardTest; s.hold = false; s.releases.splice(0).forEach(resolve => resolve()); });
  await page.waitForFunction(() => window.dashboardTest.calls.at(-1).range === "all" && window.dashboardTest.active === 0);
  await page.getByRole("button", { name: "All", exact: true }).waitFor();
  await page.locator(".dashboard-metrics").waitFor();
  assert.equal(await allRange.evaluate(el => el === document.activeElement), true, "range control remains focused after success");
  assert.equal(await page.getByRole("button", { name: "All", exact: true }).getAttribute("aria-pressed"), "true");
  assert.equal(await page.evaluate(() => window.dashboardTest.maxActive), 1);
  assert.equal(await page.evaluate(() => window.dashboardTest.calls.some(call => "pointBudget" in call)), false);
  const metric = page.getByRole("group", { name: "Breakdown metric" });
  const costMetric = metric.getByRole("button", { name: "Estimated cost", exact: true });
  const tokenMetric = metric.getByRole("button", { name: "Tokens", exact: true });
  await page.evaluate(() => { const s = window.dashboardTest; s.hold = true; s.response.breakdowns.models[0].label = "Stale ranking"; });
  await costMetric.focus(); await page.keyboard.press("Enter");
  await page.waitForFunction(() => window.dashboardTest.active === 1 && window.dashboardTest.calls.at(-1).breakdownMetric === "cost");
  assert.equal(await costMetric.evaluate(el => el === document.activeElement), true, "metric remains keyboard focused while loading");
  await page.evaluate(() => { window.dashboardTest.response.breakdowns.models[0].label = "Latest ranking"; });
  await tokenMetric.focus(); await page.keyboard.press("Enter");
  await page.evaluate(() => window.dashboardTest.releases.splice(0).forEach(resolve => resolve()));
  await page.waitForFunction(() => window.dashboardTest.calls.at(-1).breakdownMetric === "tokens" && window.dashboardTest.releases.length === 1);
  assert.equal(await page.getByText("Stale ranking", { exact: true }).count(), 0, "an obsolete metric response never publishes");
  await page.evaluate(() => { const s = window.dashboardTest; s.hold = false; s.releases.splice(0).forEach(resolve => resolve()); });
  await page.getByText("Latest ranking", { exact: true }).waitFor();
  assert.equal(await tokenMetric.getAttribute("aria-pressed"), "true");
  assert.equal(await tokenMetric.evaluate(el => el === document.activeElement), true, "metric remains focused after success");
  await costMetric.click();
  await page.waitForFunction(() => window.dashboardTest.active === 0 && window.dashboardTest.calls.at(-1).breakdownMetric === "cost");
  assert.equal(await costMetric.getAttribute("aria-pressed"), "true", "cost selector applies the backend ranking metric");
  assert.equal(await page.locator(".breakdown-cost").count(), 2);
  assert.equal(await page.evaluate(() => window.dashboardTest.maxActive), 1, "range, metric and live refreshes share one read queue");
  await page.evaluate(() => {
    const s = window.dashboardTest;
    s.response.chart.points = []; s.response.chart.boundaries = []; s.response.weekly.currentCycle = null;
    s.response.weekly.overall.estimatedCost.complete = false; s.response.weekly.overall.effectiveUsdPerPercent = null; s.response.weekly.overall.unavailableReason = "unpricedUsage";
    s.response.localUsage.points[1].tokens.totalTokens.knownTokens = "9007199254740993";
    s.response.localUsage.summary.tokens.totalTokens.knownTokens = s.response.localUsage.points.reduce((sum, point) => sum + BigInt(point.tokens.totalTokens.knownTokens), 0n).toString();
    s.callbacks[s.listener]({ payload: {} });
  });
  await page.clock.runFor(200);
  await page.getByText("No weekly observations in this range.", { exact: true }).waitFor();
  await page.getByText("Awaiting quota data", { exact: true }).waitFor();
  assert.equal(await page.locator(".usage-chart .uplot").count(), 2, "local charts remain available without quota observations");
  await page.getByText("Unpriced usage — estimate unavailable", { exact: true }).first().waitFor();
  await page.locator(".weekly-details > summary").click();
  assert.match(await page.locator(".weekly-details").innerText(), /incomplete known subtotal/);
  await page.locator(".usage-chart-tokens .chart-inspector > summary").click();
  const exactTokenSlider = page.locator(".usage-chart-tokens").getByRole("slider");
  await exactTokenSlider.focus(); await page.keyboard.press("Home"); await page.keyboard.press("ArrowRight");
  assert.match(await page.locator(".usage-chart-tokens .chart-inspector [role=status]").innerText(), /9,007,199,254,740,993/, "local token inspection does not round integers above the safe floating point limit");
  await page.evaluate(() => {
    const s = window.dashboardTest;
    for (const usage of [s.response.localUsage.summary, ...s.response.localUsage.points, ...s.response.breakdowns.models, ...s.response.breakdowns.projects]) usage.estimatedCost = { knownSubtotal: null, complete: false };
    s.callbacks[s.listener]({ payload: {} });
  });
  await page.clock.runFor(200);
  await page.getByText("Add prices to see estimated cost", { exact: true }).waitFor();
  assert.equal(await page.locator(".usage-chart-tokens .uplot").count(), 1, "unpriced usage retains its token chart");
  assert.equal(await page.locator(".usage-chart-cost .uplot").count(), 0, "unknown cost is not plotted as zero");
  assert.match(await page.locator(".dashboard-summary .metric-cost").innerText(), /Unpriced/);
  assert.match(await page.locator(".dashboard-coverage").innerText(), /Some usage is unpriced/);
  if (process.env.DASHBOARD_APP_MOUNT) await page.locator(".usage-chart-cost").getByRole("button", { name: "Configure prices" }).waitFor();
  await page.evaluate(() => {
    const s = window.dashboardTest;
    s.response.localUsage.summary.estimatedCost.knownSubtotal = "100000000000";
    s.response.localUsage.points[0].estimatedCost.knownSubtotal = "100000000000";
    s.callbacks[s.listener]({ payload: {} });
  });
  await page.clock.runFor(200);
  await page.locator(".usage-chart-cost .uplot").waitFor();
  assert.match(await page.locator(".usage-chart-cost .chart-footnote").innerText(), /Incomplete known subtotal/);
  await allRange.focus();
  await page.evaluate(() => { const s = window.dashboardTest; s.fail = true; s.callbacks[s.listener]({ payload: {} }); });
  await page.clock.runFor(200);
  await page.getByRole("alert").waitFor();
  assert.equal(await allRange.evaluate(el => el === document.activeElement), true, "range control remains focused after an error");
  await page.evaluate(() => { window.dashboardTest.fail = false; });
  await page.getByRole("button", { name: "Retry dashboard" }).click();
  await page.getByRole("alert").waitFor({ state: "hidden" });
  await page.evaluate(() => { const s = window.dashboardTest; s.response.localUsage.points = []; s.response.breakdowns.models = []; s.response.breakdowns.projects = []; s.callbacks[s.listener]({ payload: {} }); });
  await page.clock.runFor(200);
  await page.locator(".usage-chart").getByText("No recorded activity", { exact: true }).first().waitFor();
  assert.equal(await page.locator(".usage-chart").getByText("No recorded activity", { exact: true }).count(), 2);
  assert.equal(await page.getByText("No recorded usage in this range.", { exact: true }).count(), 2);
  assert.deepEqual(failures, []);
  console.log("Dashboard UI: eight hypotheses, category cost panel, shared chart, stable colors, weighted exact metrics, unavailable states, keyboard controls, 390px overflow and preserved upper dashboard checks passed.");
} finally { await browser?.close(); server.kill(); }
