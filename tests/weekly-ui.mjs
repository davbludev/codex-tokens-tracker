// Prepared IPC boundary checks; real observation accounting and replay are covered in Rust.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { pathToFileURL } from "node:url";
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE ? pathToFileURL(process.env.PLAYWRIGHT_MODULE).href : "playwright");
const server = spawn(process.execPath, ["node_modules/vite/bin/vite.js", "--host", "127.0.0.1", "--port", "1425", "--strictPort"], { windowsHide: true, stdio: "pipe" });
let browser, page;
try {
  await new Promise((resolve, reject) => {
    const deadline = setTimeout(() => reject(new Error("Vite did not start")), 15000);
    server.stdout.on("data", data => { if (data.toString().includes("1425")) { clearTimeout(deadline); resolve(); } });
    server.on("exit", code => { clearTimeout(deadline); reject(new Error(`Vite exited ${code}`)); });
  });
  browser = await chromium.launch({ channel: "msedge", headless: true });
  page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
  page.setDefaultTimeout(10000);
  const failures = [];
  page.on("pageerror", error => failures.push(error.message));
  await page.addInitScript(() => {
    const s = window.weeklyTest = { callbacks: {}, listeners: new Set(), calls: [], hold: false, releases: [], fail: false, empty: false, missing: false, active: 0, maxActive: 0 };
    const time = n => ({ seconds: 1767225600 + n, nanos: 123456789 });
    const cost = { knownSubtotal: "1234567890123", complete: true, acceptedObservations: 2 };
    const tokens = Object.fromEntries(["totalTokens", "inputTokens", "cachedInputTokens", "cacheWriteTokens", "outputTokens", "reasoningTokens"].map((key, i) => [key, { knownTokens: i === 3 ? null : i ? "10" : "9007199254740993", complete: i !== 3 }]));
    const estimate = n => ({ start: time(n + 1), end: time(n + 2), consumedPercentagePoints: "2", estimatedCost: cost, effectiveUsdPerPercent: "0.617283945061", estimatedFullWeekUsd: "61.728394506150", unavailableReason: null });
    const cycle = n => {
      const e = estimate(n);
      if (n === 1) Object.assign(e, { start: null, end: null, consumedPercentagePoints: null, estimatedCost: null, effectiveUsdPerPercent: null, estimatedFullWeekUsd: null, unavailableReason: "insufficientObservations" });
      if (n === 2) Object.assign(e, { consumedPercentagePoints: "0.5", effectiveUsdPerPercent: null, estimatedFullWeekUsd: null, unavailableReason: "belowOnePercentagePoint" });
      if (n === 3) Object.assign(e, { estimatedCost: { ...cost, complete: false }, effectiveUsdPerPercent: null, estimatedFullWeekUsd: null, unavailableReason: "unpricedUsage" });
      if (n === 4) Object.assign(e, { start: null, end: null, estimatedCost: null, effectiveUsdPerPercent: null, estimatedFullWeekUsd: null, unavailableReason: "ambiguousObservation" });
      return { key: `${n}:000000000`, firstObservation: { time: time(n), usedPercent: "10", remainingPercent: "90", resetsAt: null }, lastObservation: { time: time(n + 2), usedPercent: "12", remainingPercent: "88", resetsAt: n === 0 ? 1800000000 : null }, detectedReset: n !== 24, hasAmbiguousObservations: n === 0 || n === 4, fullCycleCostKnown: false, estimate: e, tokens: e.start ? tokens : null };
    };
    s.emit = () => { for (const id of s.listeners) s.callbacks[id]({ payload: { sourceAvailable: true } }); };
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: (_event, id) => s.listeners.delete(id) };
    window.__TAURI_INTERNALS__ = {
      transformCallback: callback => { const id = Object.keys(s.callbacks).length + 1; s.callbacks[id] = callback; return id; },
      invoke: async (command, args) => {
        if (command === "plugin:event|listen") { s.listeners.add(args.handler); return args.handler; }
        if (command === "plugin:event|unlisten") { s.listeners.delete(args.eventId); return; }
        if (command === "usage_snapshot") return { sourceAvailable: true };
        if (command === "pricing_models") return { models: [], nextCursor: null };
        if (command === "usage_dashboard") throw "storage";
        if (!["usage_weekly", "usage_weekly_models"].includes(command)) throw new Error(`Unexpected command ${command}`);
        s.calls.push({ command, query: structuredClone(args.query) }); s.active++; s.maxActive = Math.max(s.maxActive, s.active);
        const q = args.query;
        let result;
        if (command === "usage_weekly") result = { history: s.empty ? [] : q.before ? [cycle(25)] : Array.from({ length: 25 }, (_, n) => cycle(n)), nextCursor: s.empty || q.before ? null : "24:000000000", excludedSamples: 3 };
        else {
          const c = cycle(Number(q.cycleKey.split(":")[0]));
          result = s.missing ? null : { cycleKey: q.cycleKey, estimate: c.estimate, items: c.estimate.start ? Array.from({ length: q.page.after ? 1 : 25 }, (_, n) => ({ id: `model:${n}`, model: n === 0 && !q.page.after ? null : `${q.cycleKey}-model-${q.page.after ? 25 : n}`, tokens, estimatedCost: cost })) : [], nextCursor: !c.estimate.start || q.page.after ? null : "model:24" };
        }
        if (s.hold) await new Promise(resolve => s.releases.push(resolve));
        s.active--;
        if (s.fail) throw "storage";
        return result;
      },
    };
  });
  await page.clock.install();
  await page.goto("http://127.0.0.1:1425/");
  const navigation = page.getByRole("button", { name: "Weekly History", exact: true });
  await navigation.focus(); await page.keyboard.press("Enter");
  const cycles = page.getByRole("region", { name: "Completed cycle comparison" });
  const models = page.getByRole("region", { name: "Selected cycle models" });
  await models.waitFor();
  assert.equal(await navigation.getAttribute("aria-pressed"), "true");
  assert.equal(await cycles.locator("tbody tr").count(), 25);
  assert.equal(await models.locator("tbody tr").count(), 25);
  assert.match(await cycles.innerText(), /Since observation began — partial first cycle/);
  assert.match(await cycles.innerText(), /Insufficient comparable observations[\s\S]*Less than 1 percentage point observed[\s\S]*Unpriced usage — estimate unavailable[\s\S]*Ambiguous observation/);
  assert.match(await cycles.locator("tbody tr").first().innerText(), /00:00:00.123456789Z[\s\S]*00:00:01.123456789Z[\s\S]*9,007,199,254,740,993[\s\S]*\$1.234567890123[\s\S]*\$0.617283945061/);
  assert.match(await models.innerText(), /Model unavailable/);
  await page.getByRole("button", { name: "Next cycle models page" }).click();
  await models.getByRole("rowheader", { name: "0:000000000-model-25", exact: true }).waitFor();
  assert.equal(await models.locator("tbody tr").count(), 1);
  await page.getByRole("button", { name: "Next cycles page" }).click();
  await models.getByRole("rowheader", { name: "25:000000000-model-1", exact: true }).waitFor();
  assert.equal(await cycles.locator("tbody tr").count(), 1);
  await page.evaluate(() => window.weeklyTest.emit()); await page.clock.runFor(200);
  await models.getByRole("rowheader", { name: "0:000000000-model-1", exact: true }).waitFor();
  assert.equal(await cycles.locator("tbody tr").count(), 25);
  await cycles.locator("tbody tr").nth(1).getByRole("button").click();
  await page.getByText("Model breakdown unavailable without a comparable interval.", { exact: true }).waitFor();
  // Selection changes during a held response must never display the old cycle's models.
  await page.evaluate(() => { window.weeklyTest.hold = true; });
  await cycles.locator("tbody tr").nth(2).getByRole("button").click();
  await page.waitForFunction(() => window.weeklyTest.releases.length === 1);
  await cycles.locator("tbody tr").nth(3).getByRole("button").click();
  await page.evaluate(() => window.weeklyTest.releases.splice(0).forEach(resolve => resolve()));
  await page.waitForFunction(() => window.weeklyTest.releases.length === 1);
  assert.equal(await models.count(), 0);
  await page.evaluate(() => { const s = window.weeklyTest; s.hold = false; s.releases.splice(0).forEach(resolve => resolve()); });
  await models.getByRole("rowheader", { name: "3:000000000-model-1", exact: true }).waitFor();
  // Live invalidation also suppresses a held old history page and restarts both cursors.
  await page.evaluate(() => { window.weeklyTest.hold = true; });
  await page.getByRole("button", { name: "Next cycles page" }).click();
  await page.waitForFunction(() => window.weeklyTest.releases.length === 1);
  await page.evaluate(() => { const s = window.weeklyTest; s.emit(); s.releases.splice(0).forEach(resolve => resolve()); });
  assert.equal(await cycles.count(), 0);
  await page.clock.runFor(200); await page.waitForFunction(() => window.weeklyTest.releases.length === 1);
  await page.evaluate(() => { const s = window.weeklyTest; s.hold = false; s.releases.splice(0).forEach(resolve => resolve()); });
  await models.getByRole("rowheader", { name: "0:000000000-model-1", exact: true }).waitFor();
  assert.equal(await page.evaluate(() => window.weeklyTest.maxActive), 1);
  if (process.env.WEEKLY_SCREENSHOT) await page.screenshot({ path: process.env.WEEKLY_SCREENSHOT.replace(".png", "-desktop.png"), fullPage: true });
  await page.setViewportSize({ width: 390, height: 850 });
  await cycles.focus(); await page.keyboard.press("ArrowRight");
  assert.equal(await cycles.evaluate(el => el === document.activeElement), true);
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true);
  if (process.env.WEEKLY_SCREENSHOT) await page.screenshot({ path: process.env.WEEKLY_SCREENSHOT, fullPage: true });
  await page.evaluate(() => { window.weeklyTest.missing = true; });
  await cycles.locator("tbody tr").nth(2).getByRole("button").click();
  await page.getByRole("button", { name: "Reload cycles" }).waitFor();
  await page.evaluate(() => { const s = window.weeklyTest; s.missing = false; s.fail = true; s.emit(); }); await page.clock.runFor(200);
  await page.getByRole("alert").filter({ hasText: "Weekly history could not be loaded" }).waitFor();
  await page.evaluate(() => { window.weeklyTest.fail = false; window.weeklyTest.empty = true; });
  await page.getByRole("button", { name: "Retry analytics" }).click();
  await page.getByText("No completed weekly cycles observed yet.", { exact: true }).waitFor();
  assert.equal(await page.evaluate(() => window.weeklyTest.calls.every(({ query }) => (query.page?.limit ?? query.limit) === 25)), true);
  assert.deepEqual(failures, []);
  console.log("Weekly UI passed: real app navigation, exact interval metrics/reasons, partial/reset disclosures, both bounded pages, unknown models, keyboard/narrow layout, serialized obsolete-response suppression, live cursor reset, missing cycle, retry and empty states. Transport mocked.");
} catch (error) { console.error(await page?.locator("body").innerText()); throw error; }
finally { await browser?.close(); server.kill(); }
