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
  const page = await browser.newPage({ viewport: { width: 1100, height: 950 } });
  const failures = [];
  page.on("pageerror", error => { failures.push(error.message); console.error(error.message); });
  // Observe the native chart contract without adding a production test API.
  await page.route("**/node_modules/.vite/deps/uplot.js*", async route => {
    const response = await route.fetch();
    const source = await response.text();
    assert.ok(source.includes("export { uPlot as default };"));
    await route.fulfill({ response, body: source.replace("export { uPlot as default };", "const ObservedPlot = new Proxy(uPlot, { construct(target, args) { const plot = Reflect.construct(target, args); window.dashboardPlot = plot; return plot; } }); export { ObservedPlot as default };") });
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
    const response = {
      evaluatedAt: time(1800001000), tokenScope: "locallyObservedGlobalAllHistory",
      weekly: { evaluatedAt: time(1800001000), currentCycle: { key: "cycle", firstObservation: observation, lastObservation: observation, detectedReset: false, hasAmbiguousObservations: false, fullCycleCostKnown: false }, observationAgeSeconds: 100, overall: estimate, recent: estimate, unmatchedCost: cost("500000000000"), unmatchedCostStart: time(1800000900), history: [], nextCursor: null, excludedSamples: 0, sessionWeeklyPercentageImpact: null, coverageNote: "Local observations only." },
      global: { tokens: Object.fromEntries(["totalTokens", "inputTokens", "cachedInputTokens", "cacheWriteTokens", "reasoningTokens", "outputTokens"].map(key => [key, category])), estimatedCost: cost(), coverage: { incompleteSessions: 1, unavailableSessions: 0, unresolvedUsage: true, unknownModel: false, unattributedProject: false, sourceDiagnostics: false }, observedAt: null, observedSessions: 2, placeholders: 0 },
      chart: { range: "currentCycle", start: time(1800000000), end: time(1800001000), binCount: 10, returnedObservationCount: 3, sourceObservationCount: 3, coverageNote: "Accepted observation segments.",
        points: [
          { time: time(1800000000), segmentId: "a", weeklyUsedPercent: "40", cumulativeEstimatedCost: cost("0"), effectiveUsdPerPercent: null, unavailableReason: "belowOnePercentagePoint", connectFromPrevious: false },
          { time: time(1800000900), segmentId: "a", weeklyUsedPercent: "42.000000001", cumulativeEstimatedCost: cost(), effectiveUsdPerPercent: "0.617283945", unavailableReason: null, connectFromPrevious: true },
          { time: time(1800001000), segmentId: "b", weeklyUsedPercent: "1", cumulativeEstimatedCost: cost("0"), effectiveUsdPerPercent: null, unavailableReason: "insufficientObservations", connectFromPrevious: false },
        ], boundaries: [{ binIndex: 9, firstTime: time(1800001000), lastTime: time(1800001000), count: 1, kinds: ["reset"], overloaded: false }],
      },
    };
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
        const result = structuredClone(state.response); result.chart.range = args.query.range;
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
  await page.getByText("42.000000001% / 57.999999999%", { exact: true }).waitFor();
  if (process.env.DASHBOARD_APP_MOUNT) {
    const pricingTrigger = page.getByRole("button", { name: "Model Pricing", exact: true });
    await pricingTrigger.click();
    const dialog = page.getByRole("dialog", { name: "Model Pricing", exact: true });
    await dialog.waitFor();
    await dialog.getByText("0 detected models", { exact: true }).waitFor();
    await dialog.getByRole("button", { name: "Close", exact: true }).click();
    await dialog.waitFor({ state: "hidden" });
    assert.equal(await pricingTrigger.evaluate(el => el === document.activeElement), true, "pricing close restores shell focus");
    await page.getByText("42.000000001% / 57.999999999%", { exact: true }).waitFor();
  }
  await page.getByText("Since observation began · partial cycle", { exact: true }).waitFor();
  await page.getByText("Coverage warning:", { exact: false }).waitFor();
  assert.match(await page.locator(".dashboard-tokens").innerText(), /9,007,199,254,740,993/);
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
  const slider = page.getByLabel("Inspect observation", { exact: false });
  await slider.focus(); await page.keyboard.press("Home"); await page.keyboard.press("ArrowRight");
  assert.match(await page.locator("#dashboard-selected").innerText(), /\$1.234567890123/);
  assert.match(await page.locator("#dashboard-selected").innerText(), /\.123456789Z/);
  assert.match(await page.locator("#dashboard-selected").innerText(), /42.000000001%/);
  await page.keyboard.press("End");
  assert.match(await page.locator("#dashboard-selected").innerText(), /observation boundary; no connection/);
  await page.locator(".u-over").hover({ position: { x: 20, y: 50 } });
  await page.getByRole("tooltip").waitFor();
  assert.match(await page.getByRole("tooltip").innerText(), /\.123456789Z/);
  await page.mouse.move(0, 0);
  if (process.env.DASHBOARD_SCREENSHOT) await page.screenshot({ path: process.env.DASHBOARD_SCREENSHOT, fullPage: true });
  const wide = await page.locator(".uplot").evaluate(el => el.clientWidth);
  await page.setViewportSize({ width: 390, height: 850 });
  await page.waitForFunction(width => document.querySelector(".uplot").clientWidth < width, wide);
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true, "dashboard fits narrow viewport");
  if (process.env.DASHBOARD_SCREENSHOT) await page.screenshot({ path: process.env.DASHBOARD_SCREENSHOT.replace(/\.png$/, "-narrow.png"), fullPage: true });
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
  const currentRange = page.getByRole("button", { name: "Current cycle", exact: true });
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
  await page.evaluate(() => { const s = window.dashboardTest; s.response.chart.points = []; s.response.chart.boundaries = []; s.response.weekly.overall.estimatedCost.complete = false; s.response.weekly.overall.effectiveUsdPerPercent = null; s.response.weekly.overall.unavailableReason = "unpricedUsage"; s.callbacks[s.listener]({ payload: {} }); });
  await page.clock.runFor(200);
  await page.getByText("No weekly observations in this range.", { exact: true }).waitFor();
  await page.getByText("Unpriced usage — estimate unavailable", { exact: true }).first().waitFor();
  assert.match(await page.locator(".dashboard-metrics").innerText(), /incomplete known subtotal/);
  await page.evaluate(() => { const s = window.dashboardTest; s.fail = true; s.callbacks[s.listener]({ payload: {} }); });
  await page.clock.runFor(200);
  await page.getByRole("alert").waitFor();
  assert.equal(await allRange.evaluate(el => el === document.activeElement), true, "range control remains focused after an error");
  await page.evaluate(() => { window.dashboardTest.fail = false; });
  await page.getByRole("button", { name: "Retry dashboard" }).click();
  await page.getByRole("alert").waitFor({ state: "hidden" });
  assert.deepEqual(failures, []);
  console.log("Dashboard UI: exact keyboard/hover readout, responsive canvas, serialized live/range refresh, partial/coverage/unpriced/no-data/error states passed.");
} finally { await browser?.close(); server.kill(); }
