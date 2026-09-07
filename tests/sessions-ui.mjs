// Browser behavior at the native aggregate boundary; SQLite semantics have Rust tests.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { pathToFileURL } from "node:url";
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE ? pathToFileURL(process.env.PLAYWRIGHT_MODULE).href : "playwright");
const server = spawn(process.execPath, ["node_modules/vite/bin/vite.js", "--host", "127.0.0.1", "--port", "1423", "--strictPort"], { windowsHide: true, stdio: "pipe" });
let browser;
try {
  await new Promise((resolve, reject) => {
    const deadline = setTimeout(() => reject(new Error("Vite did not start")), 15000);
    server.stdout.on("data", data => { if (data.toString().includes("1423")) { clearTimeout(deadline); resolve(); } });
    server.on("exit", code => { clearTimeout(deadline); reject(new Error(`Vite exited ${code}`)); });
  });
  browser = await chromium.launch({ channel: "msedge", headless: true });
  const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
  const failures = [];
  page.on("pageerror", error => failures.push(error.message));
  await page.route("**/node_modules/.vite/deps/uplot.js*", async route => {
    const response = await route.fetch();
    const source = await response.text();
    assert.ok(source.includes("export { uPlot as default };"));
    await route.fulfill({ response, body: source.replace("export { uPlot as default };", "const ObservedPlot = new Proxy(uPlot, { construct(target, args) { const plot = Reflect.construct(target, args); window.sessionPlots ??= {}; window.sessionPlots[args[0].scales.x.time ? 'timeline' : 'models'] = plot; return plot; } }); export { ObservedPlot as default };") });
  });
  await page.route("http://127.0.0.1:1423/", route => route.fulfill({ contentType: "text/html", body: '<html><div id="root"></div><script type="module">import React from "/node_modules/.vite/deps/react.js"; import ReactDOM from "/node_modules/.vite/deps/react-dom_client.js"; import {Sessions} from "/src/Sessions.tsx"; import "/src/style.css"; import "/src/pricing.css"; ReactDOM.createRoot(document.getElementById("root")).render(React.createElement("main", null, React.createElement(Sessions)));</script></html>' }));
  await page.addInitScript(() => {
    const NativePath = window.Path2D;
    window.Path2D = class extends NativePath {
      commands = [];
      moveTo(...args) { this.commands.push("move"); super.moveTo(...args); }
      lineTo(...args) { this.commands.push("line"); super.lineTo(...args); }
    };
    const summary = (knownSubtotal, complete) => ({ tokens: Object.fromEntries(["totalTokens", "inputTokens", "cachedInputTokens", "cacheWriteTokens", "reasoningTokens", "outputTokens"].map((key, i) => [key, { knownTokens: i ? String(i * 10) : "9007199254740993", complete: true }])), estimatedCost: { knownSubtotal, complete }, coverage: { incompleteSessions: complete ? 0 : 1, unavailableSessions: 0, unresolvedUsage: false, unknownModel: false, unattributedProject: false, sourceDiagnostics: false }, observedAt: null, observedSessions: 1, placeholders: 0 });
    const state = window.sessionsTest = { calls: [], callbacks: {}, listeners: new Set(), active: 0, maxActive: 0, hold: false, holdKind: null, releases: [], fail: false, failKind: null, pending: false, revision: 1, complete: false };
    const placeholderSummary = summary(null, false);
    for (const category of Object.values(placeholderSummary.tokens)) { category.knownTokens = null; category.complete = false; }
    const detail = thread => ({ threadId: thread, title: thread === "child-00" ? "Child title" : null, placeholder: thread === "placeholder", startedAt: null, endedAt: null, durationSeconds: null,
      firstObservedAt: thread === "placeholder" ? null : "2026-01-01T00:00:00.123456789Z", lastObservedAt: thread === "placeholder" ? null : `2026-01-01T00:00:0${state.revision}.123456789Z`,
      project: { id: `location:${thread}`, value: thread === "placeholder" ? null : `C:/${thread}`, basis: "locationDerived" },
      parentThreadId: state.pending ? null : thread === "grandchild" ? "child-00" : thread.startsWith("child-") ? "session-0" : thread === "session-0" ? "placeholder" : null, parentState: state.pending ? "pending" : "unavailable",
      direct: thread === "placeholder" ? placeholderSummary : summary("1234567890123", state.complete), inclusive: state.pending ? null : summary("2234567890123", false) });
    const time = i => ({ seconds: 1767225600 + i, nanos: 123456789 });
    state.emit = () => { for (const id of state.listeners) state.callbacks[id]({ payload: {} }); };
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: (_event, id) => state.listeners.delete(id) };
    window.__TAURI_INTERNALS__ = {
      transformCallback: callback => { const id = Object.keys(state.callbacks).length + 1; state.callbacks[id] = callback; return id; },
      invoke: async (command, args) => {
        if (command === "plugin:event|listen") { state.listeners.add(args.handler); return args.handler; }
        if (command === "plugin:event|unlisten") { state.listeners.delete(args.eventId); return; }
        if (command !== "usage_aggregates") throw new Error(`Unexpected ${command}`);
        const query = args.query;
        state.calls.push(structuredClone(query)); state.active++; state.maxActive = Math.max(state.maxActive, state.active);
        let result;
        if (query.kind === "session") result = detail(query.thread);
        else if (query.kind === "children" || query.kind === "ancestors") {
          const ids = query.kind === "children" ? query.thread === "session-0" ? Array.from({ length: 51 }, (_, i) => `child-${String(i).padStart(2, "0")}`) : query.thread === "child-00" ? ["grandchild"] : [] : query.thread === "grandchild" ? ["child-00", "placeholder", "session-0"] : query.thread === "session-0" ? ["placeholder"] : [];
          const items = ids.filter(id => !query.page.after || id > query.page.after).slice(0, query.page.limit);
          result = { items: items.map(detail), totalItems: ids.length, nextCursor: items.at(-1) !== ids.at(-1) ? items.at(-1) : null, direct: summary("1234567890123", false) };
        } else if (query.kind === "sessionModels") {
          const rows = query.thread === "placeholder" ? [] : Array.from({ length: 51 }, (_, i) => ({ attribution: { id: `model-${String(i).padStart(2, "0")}`, value: i === 0 ? "unpriced-model" : `model-${String(i).padStart(2, "0")}`, basis: "observed" }, direct: summary(i === 0 && !state.complete ? null : "1234567890123", state.complete), costShare: state.complete ? "1.96" : null, costShareUnavailableReason: state.complete ? null : i === 0 ? "unavailable" : "incomplete" }));
          const items = rows.filter(row => !query.page.after || row.attribution.id > query.page.after).slice(0, query.page.limit);
          result = { scope: "direct", items, totalItems: rows.length, nextCursor: items.length === 50 ? items.at(-1).attribution.id : null, direct: summary("1234567890123", state.complete) };
        } else if (query.kind === "sessionTimeline") {
          result = { scope: "direct", timeSource: "acceptedObservationTimestamp", firstObservedAt: time(0), lastObservedAt: time(4), pointBudget: 512, binCount: 256, sourceObservationCount: 12, sourcePointCount: 10, returnedPointCount: 5, untimedObservationCount: 2, direct: summary("1234567890123", false), coverageNote: "Local accepted timeline coverage.",
            points: query.thread === "placeholder" ? [] : Array.from({ length: 5 }, (_, i) => ({ time: time(i), cumulativeTotalTokens: { knownTokens: "9007199254740993", complete: i < 2 }, cumulativeEstimatedCost: { knownSubtotal: "1234567890123", complete: i < 1 }, tokensConnectFromPrevious: i === 1 || i === 4, costConnectFromPrevious: i === 3 || i === 4 })),
            boundaries: [{ binIndex: 0, firstTime: time(0), lastTime: time(3), count: 4, kinds: ["observationStart", "unpricedUsage", "missingTokens", "tokensResumed"], overloaded: true }] };
        } else {
          const q = query.query, count = q.search === "empty" ? 0 : 25;
          result = { totalItems: count ? 2001 : 0, offset: q.offset, nextOffset: count ? q.offset + 25 : null,
            items: Array.from({ length: count }, (_, i) => ({ threadId: `${q.search ?? "session"}-${q.offset + i}`, title: null, project: { id: "location:C:/project", value: "C:/project", basis: "locationDerived" }, lastObservedAt: i ? "2026-01-01T00:00:01.123456789Z" : null, durationSeconds: null, models: i ? ["alpha", "beta"] : [], modelCount: i ? 2 : 0, unknownModel: i === 0, direct: summary(i ? "1234567890123" : null, false), directSubagentCount: i ? 1 : null, weeklyPercentageImpact: null })) };
        }
        try {
          if (state.hold || state.holdKind === query.kind) await new Promise(resolve => state.releases.push(resolve));
          if (state.fail || state.failKind === query.kind) throw "storage";
          if (state.pending && ["children", "ancestors"].includes(query.kind)) throw "hierarchyPending";
          return { hierarchyPending: state.pending, hierarchyRevision: state.revision, coverageNote: "Local accepted usage only.", data: { kind: ["children", "ancestors"].includes(query.kind) ? "sessions" : query.kind, data: result } };
        } finally { state.active--; }
      },
    };
  });
  await page.clock.install();
  await page.goto("http://127.0.0.1:1423");
  await page.getByText("2001 sessions · 1–25 shown", { exact: true }).waitFor();
  assert.equal(await page.locator(".session-row").count(), 25);
  assert.match(await page.locator(".session-row").nth(1).innerText(), /9,007,199,254,740,993/);
  assert.match(await page.locator(".session-row").nth(1).innerText(), /\$1.234567890123 \(incomplete known subtotal\)/);
  assert.match(await page.locator(".session-row").first().innerText(), /Duration: Unavailable/);
  const trigger = page.getByRole("button", { name: "Untitled: session-0", exact: true });
  await trigger.focus(); await page.keyboard.press("Enter");
  const dialog = page.getByRole("dialog", { name: "Session details", exact: true });
  await dialog.getByText("Inclusive session usage", { exact: true }).waitFor();
  assert.match(await dialog.innerText(), /\$2.234567890123/);
  const models = dialog.getByRole("region", { name: "Direct model usage", exact: true });
  const children = dialog.getByRole("region", { name: "Children", exact: true });
  await models.getByText("50. model-49", { exact: true }).waitFor();
  await children.getByRole("button", { name: "child-49", exact: true }).waitFor();
  assert.equal(await models.locator("tbody tr").count(), 50);
  assert.equal(await children.locator("li").count(), 50);
  assert.match(await dialog.locator(".session-metadata").first().innerText(), /Start\s+Unavailable\s+End\s+Unavailable\s+Duration\s+Unavailable/);
  assert.match(await models.locator("tbody tr").first().innerText(), /Unavailable — cost unavailable/);
  assert.match(await models.locator("tbody tr").nth(1).innerText(), /Unavailable — incomplete session or model cost/);
  assert.deepEqual(await models.locator("tbody tr").nth(1).locator("td").allTextContents(), ["9,007,199,254,740,993", "10", "20", "30", "40", "50", "$1.234567890123 (incomplete known subtotal)", "Unavailable — incomplete session or model cost"]);
  await dialog.getByLabel("Inspect model", { exact: false }).focus();
  await page.keyboard.press("Home");
  assert.match(await dialog.locator("#session-model-selected").innerText(), /unpriced-model[\s\S]*Unavailable — cost unavailable/);
  await page.keyboard.press("ArrowRight");
  assert.match(await dialog.locator("#session-model-selected").innerText(), /model-01[\s\S]*\$1.234567890123 \(incomplete known subtotal\)/);
  await page.waitForFunction(() => window.sessionPlots?.timeline && window.sessionPlots?.models);
  const plots = await page.evaluate(() => {
    const { timeline, models } = window.sessionPlots;
    const path = index => timeline.series[index].paths(timeline, index, 0, 4).stroke.commands;
    return { scales: timeline.series.slice(1).map(s => s.scale), tokenPaths: path(1), costPaths: path(2), modelScales: models.series.slice(1).map(s => s.scale), modelData: models.data.slice(2).map(s => s[1]), costMax: timeline.scales.cost.max, tokenMax: timeline.scales.tokens.max };
  });
  assert.deepEqual(plots.scales, ["tokens", "cost"]);
  assert.deepEqual(plots.tokenPaths, ["move", "line", "move", "move", "line"], "tokens break on missing and resumed groups");
  assert.deepEqual(plots.costPaths, ["move", "move", "move", "line", "line"], "cost connectivity is independent of token gaps");
  assert.deepEqual(plots.modelScales, ["tokens", "tokens", "tokens", "tokens", "tokens", "tokens", "cost"]);
  assert.deepEqual(plots.modelData, [10, 20, 30, 40, 50, 1.234567890123]);
  assert.ok(plots.tokenMax > 9e15 && plots.costMax < 10, "independent token and USD scales");
  const inspector = dialog.getByLabel("Inspect usage point", { exact: false });
  await inspector.focus(); await page.keyboard.press("Home"); await page.keyboard.press("ArrowRight");
  assert.match(await dialog.locator("#session-timeline-selected").innerText(), /\.123456789Z/);
  assert.match(await dialog.locator("#session-timeline-selected").innerText(), /Cost gap/);
  await page.keyboard.press("End");
  assert.match(await dialog.locator("#session-timeline-selected").innerText(), /9,007,199,254,740,993 \(incomplete\)/);
  assert.match(await dialog.innerText(), /2 untimed observations contribute to direct totals/);
  await dialog.getByText("Timeline boundaries (4)", { exact: true }).click();
  assert.match(await dialog.innerText(), /multiple boundaries grouped; connections conservatively broken/);
  await page.setViewportSize({ width: 390, height: 850 });
  await page.waitForFunction(() => window.sessionPlots.timeline.width < 390 && window.sessionPlots.models.width < 390);
  assert.equal(await dialog.evaluate(el => el.scrollWidth <= el.clientWidth), true, "charts and scrollable model table fit narrow dialog");
  await dialog.getByRole("button", { name: "Close", exact: true }).focus();
  await page.keyboard.press("Shift+Tab");
  assert.equal(await dialog.evaluate(el => el.contains(document.activeElement)), true, "native modal contains keyboard focus");
  if (process.env.SESSIONS_SCREENSHOT) {
    await page.screenshot({ path: process.env.SESSIONS_SCREENSHOT.replace(/\.png$/, "-detail-narrow.png") });
    await dialog.locator(".session-chart").screenshot({ path: process.env.SESSIONS_SCREENSHOT.replace(/\.png$/, "-models-narrow.png") });
    await dialog.locator(".session-timeline-plot").screenshot({ path: process.env.SESSIONS_SCREENSHOT.replace(/\.png$/, "-timeline-narrow.png") });
  }
  await page.setViewportSize({ width: 1280, height: 900 });
  await children.getByRole("button", { name: "Next children page" }).click();
  await children.getByRole("button", { name: "child-50", exact: true }).waitFor();
  assert.equal(await children.locator("li").count(), 1, "child pages replace, never accumulate");
  await models.getByRole("button", { name: "Next models page" }).click();
  await models.getByText("1. model-50", { exact: true }).waitFor();
  assert.equal(await models.locator("tbody tr").count(), 1, "model pages replace");
  await page.evaluate(() => { const s = window.sessionsTest; s.revision = 2; s.emit(); });
  await page.clock.runFor(200);
  await models.getByText("50. model-49", { exact: true }).waitFor();
  await children.getByRole("button", { name: "child-49", exact: true }).waitFor();
  assert.match(await dialog.locator(".session-metadata").first().innerText(), /00:00:02.123456789Z/);
  // A paged reply arriving after invalidation but before the debounce must not publish.
  await page.evaluate(() => { window.sessionsTest.holdKind = "sessionModels"; });
  await models.getByRole("button", { name: "Next models page" }).click();
  await page.waitForFunction(() => window.sessionsTest.releases.length === 1);
  await page.evaluate(() => { const s = window.sessionsTest; s.emit(); s.releases.splice(0).forEach(resolve => resolve()); });
  await page.waitForFunction(() => window.sessionsTest.active === 0);
  assert.equal(await models.locator("tbody tr").count(), 0, "invalidated model page does not reappear during debounce");
  await page.clock.runFor(200);
  await page.waitForFunction(() => window.sessionsTest.releases.length === 1);
  await page.evaluate(() => { const s = window.sessionsTest; s.holdKind = null; s.releases.splice(0).forEach(resolve => resolve()); });
  await models.getByText("50. model-49", { exact: true }).waitFor();
  await page.evaluate(() => { const s = window.sessionsTest; s.complete = true; s.emit(); });
  await page.clock.runFor(200);
  await models.getByText("1.96% of complete direct session cost", { exact: true }).first().waitFor();
  await models.getByRole("button", { name: "Next models page" }).click();
  await models.getByText("1. model-50", { exact: true }).waitFor();
  assert.match(await models.locator("tbody").innerText(), /1.96% of complete direct session cost/, "never recompute a page-local 100% share");
  await children.getByRole("button", { name: "Child title", exact: true }).click();
  await dialog.getByRole("heading", { name: "Child title", exact: true }).waitFor();
  assert.match(await dialog.innerText(), /C:\/child-00/);
  assert.equal(await dialog.getByRole("heading", { name: "Session details", exact: true }).evaluate(el => el === document.activeElement), true, "navigation moves focus to detail heading");
  await dialog.getByRole("button", { name: "grandchild", exact: true }).click();
  const ancestors = dialog.getByRole("region", { name: "Ancestors", exact: true });
  await ancestors.getByRole("button", { name: "session-0", exact: true }).waitFor();
  assert.deepEqual(await ancestors.locator("li button").allTextContents(), ["Child title", "placeholder", "session-0"], "ancestor set stays ID ordered, not a fabricated breadcrumb");
  assert.match(await dialog.innerText(), /C:\/grandchild/);
  await dialog.getByRole("button", { name: "Open parent child-00", exact: true }).click();
  await dialog.getByRole("heading", { name: "Child title", exact: true }).waitFor();
  // Disposed requests from the previous selection cannot populate the next detail.
  await page.evaluate(() => { const s = window.sessionsTest; s.holdKind = "sessionModels"; s.emit(); });
  await page.clock.runFor(200);
  await page.waitForFunction(() => window.sessionsTest.releases.length === 1);
  await dialog.getByRole("button", { name: "Open parent session-0", exact: true }).click();
  await dialog.getByRole("button", { name: "Open parent placeholder", exact: true }).click();
  await dialog.getByText("Missing-parent placeholder — this session has not been observed.", { exact: true }).waitFor();
  await page.evaluate(() => { const s = window.sessionsTest; s.holdKind = null; s.releases.splice(0).forEach(resolve => resolve()); });
  await models.getByText("No model usage available.", { exact: true }).waitFor();
  assert.equal(await models.locator("tbody tr").count(), 0, "old session models cannot populate placeholder");
  assert.match(await dialog.innerText(), /Project unavailable/);
  assert.match(await dialog.innerText(), /Effective parent\s+Unavailable/);
  assert.match(await dialog.innerText(), /No timed accepted usage available/);
  await page.setViewportSize({ width: 390, height: 850 });
  assert.equal(await dialog.evaluate(el => el.scrollWidth <= el.clientWidth), true, "detail fits narrow viewport");
  await page.keyboard.press("Escape"); await dialog.waitFor({ state: "hidden" });
  assert.equal(await trigger.evaluate(el => el === document.activeElement), true, "close restores row focus");
  await page.waitForFunction(() => window.sessionsTest.active === 0 && window.sessionsTest.listeners.size === 1);
  await page.evaluate(() => { const s = window.sessionsTest; s.pending = true; s.failKind = "sessionTimeline"; });
  await trigger.click();
  await dialog.getByText("Hierarchy pending — relationships and inclusive usage are unavailable.", { exact: true }).waitFor();
  await models.getByText("50. model-49", { exact: true }).waitFor();
  await dialog.getByRole("button", { name: "Retry timeline", exact: true }).waitFor();
  assert.match(await dialog.innerText(), /Unavailable while hierarchy is being reconciled/);
  assert.equal(await dialog.getByRole("button", { name: "Open parent placeholder", exact: true }).count(), 0);
  await page.evaluate(() => { window.sessionsTest.failKind = null; });
  await dialog.getByRole("button", { name: "Retry timeline", exact: true }).click();
  await inspector.waitFor();
  await page.evaluate(() => { const s = window.sessionsTest; s.pending = false; s.emit(); });
  await page.clock.runFor(200);
  await children.getByRole("button", { name: "Child title", exact: true }).waitFor();
  await page.keyboard.press("Escape"); await dialog.waitFor({ state: "hidden" });
  await page.waitForFunction(() => window.sessionsTest.active === 0 && window.sessionsTest.listeners.size === 1);
  await page.evaluate(() => { window.sessionsTest.maxActive = 0; });
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.getByRole("button", { name: "Next page", exact: true }).click();
  await page.getByText("2001 sessions · 26–50 shown", { exact: true }).waitFor();
  assert.equal(await page.locator(".session-row").count(), 25, "paging replaces rather than appends rows");
  await page.getByRole("button", { name: "Untitled: session-25", exact: true }).click();
  await dialog.getByText("Inclusive session usage", { exact: true }).waitFor();
  await page.evaluate(() => window.sessionsTest.emit());
  await page.clock.runFor(200);
  await page.waitForFunction(() => window.sessionsTest.active === 0);
  await page.keyboard.press("Escape"); await dialog.waitFor({ state: "hidden" });
  assert.equal(await page.getByRole("heading", { name: "Sessions", exact: true }).evaluate(el => el === document.activeElement), true, "removed live-page trigger restores focus to Sessions heading");
  await page.waitForFunction(() => window.sessionsTest.listeners.size === 1);
  await page.evaluate(() => { window.sessionsTest.maxActive = 0; });
  await page.getByText("2001 sessions · 1–25 shown", { exact: true }).waitFor();
  await page.getByLabel("Search session ID").fill("chosen");
  await page.getByLabel("Project contains").fill("project");
  await page.getByLabel("Model contains").fill("alpha");
  await page.getByLabel("From (UTC)", { exact: true }).fill("2026-01-02");
  await page.getByLabel("Through (UTC)", { exact: true }).fill("2026-01-01");
  await page.getByRole("button", { name: "Apply filters" }).click();
  await page.getByRole("alert").waitFor();
  await page.getByLabel("From (UTC)", { exact: true }).fill("2026-01-01");
  await page.getByLabel("Sort within project").selectOption("usd");
  await page.getByRole("button", { name: "Apply filters" }).click();
  await page.getByRole("button", { name: "Untitled: chosen-0", exact: true }).waitFor();
  const applied = await page.evaluate(() => window.sessionsTest.calls.at(-1).query);
  assert.deepEqual(applied, { search: "chosen", project: "project", model: "alpha", fromSeconds: 1767225600, beforeSeconds: 1767312000, sort: "usd", limit: 25, offset: 0 });
  await page.getByRole("button", { name: "Next page", exact: true }).click();
  await page.getByText("2001 sessions · 26–50 shown", { exact: true }).waitFor();
  const burst = await page.evaluate(() => { const s = window.sessionsTest; const n = s.calls.length; for (let i = 0; i < 20; i++) s.emit(); return n; });
  await page.clock.runFor(200);
  await page.getByText("2001 sessions · 1–25 shown", { exact: true }).waitFor();
  assert.equal(await page.evaluate(() => window.sessionsTest.calls.length), burst + 1);
  assert.equal(await page.getByLabel("Search session ID").inputValue(), "chosen");
  if (process.env.SESSIONS_SCREENSHOT) await page.screenshot({ path: process.env.SESSIONS_SCREENSHOT, fullPage: true });
  await page.setViewportSize({ width: 390, height: 850 });
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true, "only result region scrolls horizontally");
  if (process.env.SESSIONS_SCREENSHOT) await page.screenshot({ path: process.env.SESSIONS_SCREENSHOT.replace(/\.png$/, "-narrow.png"), fullPage: true });
  await page.evaluate(() => { window.sessionsTest.hold = true; window.sessionsTest.emit(); });
  await page.clock.runFor(200);
  await page.waitForFunction(() => window.sessionsTest.active === 1);
  await page.getByLabel("Search session ID").fill("latest");
  await page.getByRole("button", { name: "Apply filters" }).click();
  await page.evaluate(() => window.sessionsTest.releases.splice(0).forEach(resolve => resolve()));
  await page.waitForFunction(() => window.sessionsTest.calls.at(-1).query.search === "latest" && window.sessionsTest.releases.length === 1);
  assert.equal(await page.locator(".session-row").count(), 0, "old selection cannot repopulate results");
  await page.evaluate(() => { const s = window.sessionsTest; s.hold = false; s.releases.splice(0).forEach(resolve => resolve()); });
  await page.getByRole("button", { name: "Untitled: latest-0", exact: true }).waitFor();
  assert.equal(await page.evaluate(() => window.sessionsTest.maxActive), 1);
  await page.getByLabel("Search session ID").fill("empty");
  await page.getByRole("button", { name: "Apply filters" }).click();
  await page.getByText("No sessions match these filters.", { exact: true }).waitFor();
  await page.evaluate(() => { window.sessionsTest.fail = true; window.sessionsTest.emit(); });
  await page.clock.runFor(200);
  await page.getByRole("alert").waitFor();
  assert.equal(await page.getByLabel("Search session ID").inputValue(), "empty");
  await page.evaluate(() => { window.sessionsTest.fail = false; });
  await page.getByRole("button", { name: "Retry sessions" }).click();
  await page.getByRole("alert").waitFor({ state: "hidden" });
  assert.deepEqual(failures, []);
  console.log("Sessions UI passed: list regressions; multilevel/placeholder navigation; 51-child/model bounded paging; exact categories, USD and backend shares; independent chart gaps/scales; keyboard inspection and focus; live resets and stale suppression; pending hierarchy, retry, cleanup and narrow layout. Transport mocked; native WebView not exercised.");
} finally { await browser?.close(); server.kill(); }
