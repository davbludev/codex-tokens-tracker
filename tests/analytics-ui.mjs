// Browser workflows at the prepared analytics IPC boundary; accounting is checked in Rust.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { pathToFileURL } from "node:url";
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE ? pathToFileURL(process.env.PLAYWRIGHT_MODULE).href : "playwright");
const server = spawn(process.execPath, ["node_modules/vite/bin/vite.js", "--host", "127.0.0.1", "--port", "1424", "--strictPort"], { windowsHide: true, stdio: "pipe" });
let browser, page;
try {
  await new Promise((resolve, reject) => {
    const deadline = setTimeout(() => reject(new Error("Vite did not start")), 15000);
    server.stdout.on("data", data => { if (data.toString().includes("1424")) { clearTimeout(deadline); resolve(); } });
    server.on("exit", code => { clearTimeout(deadline); reject(new Error(`Vite exited ${code}`)); });
  });
  browser = await chromium.launch({ channel: "msedge", headless: true });
  page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
  page.setDefaultTimeout(10000);
  const failures = [];
  page.on("pageerror", error => { failures.push(error.message); console.error(error.message); });
  await page.route("http://127.0.0.1:1424/", route => route.fulfill({ contentType: "text/html", body: '<html><div id="root"></div><script type="module" src="/tests/analytics-entry.tsx"></script></html>' }));
  await page.addInitScript(() => {
    const s = window.analyticsTest = { callbacks: {}, listeners: new Set(), calls: [], complete: false, pending: false, revision: 1, fail: false, empty: false, hold: null, releases: [] };
    const time = i => ({ seconds: 1767225600 + i, nanos: 123456789 });
    const summary = (unused = false) => ({ tokens: Object.fromEntries(["totalTokens", "inputTokens", "cachedInputTokens", "cacheWriteTokens", "reasoningTokens", "outputTokens"].map((key, i) => [key, { knownTokens: unused ? null : i ? String(i * 10) : "9007199254740993", complete: !unused }])), estimatedCost: { knownSubtotal: unused ? null : "1234567890123", complete: !unused && s.complete }, coverage: { incompleteSessions: s.complete ? 0 : 1, unavailableSessions: unused ? 1 : 0, unresolvedUsage: false, unknownModel: false, unattributedProject: false, sourceDiagnostics: false }, observedAt: null, observedSessions: 3, placeholders: 0 });
    const attribution = i => ({ id: `model-${String(i).padStart(2, "0")}`, value: i === 1 ? null : i === 2 ? "detected-unused" : `model-${String(i).padStart(2, "0")}`, basis: i === 1 ? "unavailable" : "observed" });
    const model = i => ({ attribution: attribution(i), direct: summary(i === 2), acceptedUsageEvents: i === 2 ? 0 : s.revision, sessionsUsed: i === 2 ? 0 : 1, costShare: s.complete && i !== 2 ? "3.85" : null, costShareUnavailableReason: i === 2 ? "unavailable" : "incomplete", activePricingVersion: i < 2 ? null : { versionId: 42, effectiveAt: time(0) } });
    const modelPage = after => ({ items: Array.from({ length: after ? 1 : 25 }, (_, i) => attribution(after ? 25 : i)), totalItems: 26, nextCursor: after ? null : "model-24" });
    s.emit = () => { for (const id of s.listeners) s.callbacks[id]({ payload: {} }); };
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: (_event, id) => s.listeners.delete(id) };
    window.__TAURI_INTERNALS__ = {
      transformCallback: callback => { const id = Object.keys(s.callbacks).length + 1; s.callbacks[id] = callback; return id; },
      invoke: async (command, args) => {
        if (command === "plugin:event|listen") { s.listeners.add(args.handler); return args.handler; }
        if (command === "plugin:event|unlisten") { s.listeners.delete(args.eventId); return; }
        if (command !== "usage_aggregates") throw new Error(`Unexpected ${command}`);
        const q = args.query; s.calls.push(structuredClone(q));
        let data;
        if (q.kind === "projectModels") data = modelPage(q.page.after);
        else if (q.kind === "modelHistory") data = { attribution: attribution(0), start: q.start, end: q.end, pointBudget: q.pointBudget,
          bins: Array.from({ length: s.revision === 1 ? 2 : 1 }, (_, i) => ({ index: i * 10, start: time(i), end: time(i + 1), acceptedUsageEvents: 1, tokens: summary().tokens, estimatedCost: summary().estimatedCost })), untimedAcceptedUsageEvents: 2, coverageNote: "Local accepted usage only." };
        else {
          const items = Array.from({ length: s.empty ? 0 : q.page.after ? 1 : 25 }, (_, i) => {
            const n = q.page.after ? 25 : i;
            if (q.kind === "modelAnalytics") return model(n);
            return { attribution: { id: `project-${n}`, value: n === 1 ? null : `C:/project-${n}`, basis: n === 1 ? "unavailable" : "locationDerived" }, direct: summary(), averageSessionCost: { amount: s.complete ? "411522630041" : null, unavailableReason: s.complete ? null : "incompleteCost" }, models: { items: modelPage(null).items.slice(0, 5), totalItems: 26, nextCursor: "model-04" }, classification: s.pending ? null : { provenSubagent: summary(), parentClassificationUnavailable: summary() }, currentCycle: { label: "observedCurrentCycleUsage", cycleKey: "core", start: time(0), end: time(2), observationAgeSeconds: 20, partial: true, hasAmbiguousObservations: true, unavailableReason: null, direct: summary() } };
          });
          data = { evaluatedAt: time(5), items, totalItems: s.empty ? 0 : 26, nextCursor: q.page.after ? null : "after", direct: summary() };
        }
        if (s.hold === q.kind) await new Promise(resolve => s.releases.push(resolve));
        if (s.fail) throw "storage";
        return { hierarchyPending: s.pending, hierarchyRevision: s.revision, coverageNote: "Local accepted usage only.", data: { kind: q.kind, data } };
      },
    };
  });
  await page.clock.install();
  await page.goto("http://127.0.0.1:1424/");
  await page.getByRole("rowheader", { name: /^C:\/project-0/ }).waitFor();
  assert.equal(await page.locator("tbody tr").count(), 25);
  const projectRow = page.locator("tbody tr").first();
  assert.match(await projectRow.innerText(), /9,007,199,254,740,993/);
  assert.match(await projectRow.innerText(), /Unavailable — incomplete \/ unpriced cost/);
  assert.match(await projectRow.innerText(), /Proven subagent usage[\s\S]*Parent classification unavailable/);
  assert.match(await projectRow.innerText(), /\.123456789Z[\s\S]*Partial cycle[\s\S]*Ambiguous observations/);
  await projectRow.getByRole("button", { name: "Browse models", exact: true }).click();
  await projectRow.getByText("25 of 26 models", { exact: true }).waitFor();
  await projectRow.getByRole("button", { name: "Next project models page" }).click();
  await projectRow.getByText("1 of 26 models", { exact: true }).waitFor();
  await page.evaluate(() => window.analyticsTest.emit()); await page.clock.runFor(200);
  await projectRow.getByText("25 of 26 models", { exact: true }).waitFor();
  await page.getByRole("button", { name: "Next projects page", exact: true }).click();
  await page.getByRole("rowheader", { name: /^C:\/project-25/ }).waitFor();
  assert.equal(await page.locator("tbody tr").count(), 1);
  await page.evaluate(() => { const s = window.analyticsTest; s.complete = true; s.pending = true; s.emit(); }); await page.clock.runFor(200);
  await page.getByRole("rowheader", { name: /^C:\/project-0/ }).waitFor();
  assert.match(await projectRow.innerText(), /\$0.411522630041/);
  assert.match(await projectRow.innerText(), /Unavailable while hierarchy is being reconciled/);
  await page.getByRole("button", { name: "Switch view" }).click();
  await page.getByRole("button", { name: "model-00", exact: true }).waitFor();
  assert.equal(await page.locator("tbody tr").count(), 25);
  assert.match(await page.locator("tbody tr").first().innerText(), /3.85% of complete global direct cost/);
  assert.match(await page.locator("tbody tr").nth(1).innerText(), /Model unavailable/);
  const unused = page.locator("tbody tr").nth(2);
  assert.deepEqual((await unused.locator("td").allTextContents()).slice(0, 4), ["0", "0", "Unavailable", "Unavailable"]);
  assert.match(await unused.innerText(), /Version 42/);
  const trigger = page.getByRole("button", { name: "model-00", exact: true });
  await trigger.focus(); await page.keyboard.press("Enter");
  const dialog = page.getByRole("dialog", { name: /^Model history:/ });
  const inspector = dialog.getByLabel("Inspect populated bin", { exact: false });
  await inspector.waitFor();
  assert.match(await dialog.innerText(), /2 untimed accepted usage events/);
  await inspector.focus(); await page.keyboard.press("End");
  assert.match(await dialog.locator("#model-history-selected").innerText(), /00:00:02.123456789Z/);
  assert.match(await dialog.locator("#model-history-selected").innerText(), /\$1.234567890123/);
  await page.evaluate(() => { const s = window.analyticsTest; s.revision = 2; s.complete = false; s.emit(); }); await page.clock.runFor(200);
  await dialog.getByLabel("Inspect populated bin (1 of 1)", { exact: true }).waitFor();
  assert.match(await dialog.locator("#model-history-selected").innerText(), /incomplete known subtotal/);
  await dialog.getByRole("button", { name: "All observed time" }).click();
  await page.waitForFunction(() => window.analyticsTest.calls.at(-1).start?.seconds === 0);
  assert.equal(await page.evaluate(() => window.analyticsTest.calls.at(-1).pointBudget), 512);
  await page.setViewportSize({ width: 390, height: 850 });
  await page.clock.runFor(100);
  await page.waitForFunction(() => { const el = document.querySelector("dialog"); return el && el.scrollWidth <= el.clientWidth; });
  assert.equal(await dialog.evaluate(el => el.scrollWidth <= el.clientWidth), true);
  await dialog.getByRole("button", { name: "Close", exact: true }).focus(); await page.keyboard.press("Shift+Tab");
  assert.equal(await dialog.evaluate(el => el.contains(document.activeElement)), true);
  if (process.env.ANALYTICS_SCREENSHOT) await page.screenshot({ path: process.env.ANALYTICS_SCREENSHOT });
  await page.keyboard.press("Escape"); await dialog.waitFor({ state: "hidden" });
  assert.equal(await trigger.evaluate(el => el === document.activeElement), true);
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true);
  await page.getByRole("button", { name: "Next models page", exact: true }).click();
  await page.getByRole("button", { name: "model-25", exact: true }).waitFor();
  assert.equal(await page.locator("tbody tr").count(), 1);
  await page.getByRole("button", { name: "model-25", exact: true }).click(); await inspector.waitFor();
  await page.evaluate(() => window.analyticsTest.emit()); await page.clock.runFor(200);
  await page.keyboard.press("Escape"); await dialog.waitFor({ state: "hidden" });
  assert.equal(await page.getByRole("heading", { name: "Models", exact: true }).evaluate(el => el === document.activeElement), true);
  await page.evaluate(() => { window.analyticsTest.hold = "modelAnalytics"; });
  await page.getByRole("button", { name: "Next models page", exact: true }).click();
  await page.waitForFunction(() => window.analyticsTest.releases.length === 1);
  await page.evaluate(() => { const s = window.analyticsTest; s.emit(); s.releases.splice(0).forEach(resolve => resolve()); });
  assert.equal(await page.locator("tbody tr").count(), 0, "invalidated response cannot restore an old page");
  await page.clock.runFor(200); await page.waitForFunction(() => window.analyticsTest.releases.length === 1);
  await page.evaluate(() => { const s = window.analyticsTest; s.hold = null; s.releases.splice(0).forEach(resolve => resolve()); });
  await trigger.waitFor();
  await page.evaluate(() => { const s = window.analyticsTest; s.fail = true; s.emit(); }); await page.clock.runFor(200);
  await page.getByRole("alert").waitFor();
  assert.match(await page.getByRole("alert").innerText(), /Models could not be loaded/);
  await page.evaluate(() => { const s = window.analyticsTest; s.fail = false; s.empty = true; });
  await page.getByRole("button", { name: "Retry analytics" }).click();
  await page.getByText("No models detected yet.", { exact: true }).waitFor();
  await page.getByRole("button", { name: "Switch view" }).click();
  await page.getByText("No projects observed yet.", { exact: true }).waitFor();
  assert.deepEqual(failures, []);
  console.log("Analytics UI passed: exact/incomplete metrics, unknown and detected-unused models, project/model paging and expansion, cycle/classification disclosures, bounded sparse history, keyboard focus, narrow layout, live replacement/stale suppression, retry and empty states. Transport mocked.");
} catch (error) { console.error(await page?.locator("body").innerText()); throw error; }
finally { await browser?.close(); server.kill(); }
