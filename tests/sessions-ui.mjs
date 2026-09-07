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
  await page.route("http://127.0.0.1:1423/", route => route.fulfill({ contentType: "text/html", body: '<html><div id="root"></div><script type="module">import React from "/node_modules/.vite/deps/react.js"; import ReactDOM from "/node_modules/.vite/deps/react-dom_client.js"; import {Sessions} from "/src/Sessions.tsx"; import "/src/style.css"; import "/src/pricing.css"; ReactDOM.createRoot(document.getElementById("root")).render(React.createElement("main", null, React.createElement(Sessions)));</script></html>' }));
  await page.addInitScript(() => {
    const summary = (knownSubtotal, complete) => ({ tokens: { totalTokens: { knownTokens: "9007199254740993", complete: true } }, estimatedCost: { knownSubtotal, complete } });
    const state = window.sessionsTest = { calls: [], callbacks: {}, listeners: new Set(), active: 0, maxActive: 0, hold: false, releases: [], fail: false };
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
        if (query.kind === "session") result = { threadId: query.thread, project: { value: "C:/project", basis: "locationDerived" }, parentThreadId: null, parentState: "unavailable", direct: summary("1234567890123", false), inclusive: summary("2234567890123", false) };
        else {
          const q = query.query, count = q.search === "empty" ? 0 : 25;
          result = { totalItems: count ? 2001 : 0, offset: q.offset, nextOffset: count ? q.offset + 25 : null,
            items: Array.from({ length: count }, (_, i) => ({ threadId: `${q.search ?? "session"}-${q.offset + i}`, title: null, project: { id: "location:C:/project", value: "C:/project", basis: "locationDerived" }, lastObservedAt: i ? "2026-01-01T00:00:01.123456789Z" : null, durationSeconds: null, models: i ? ["alpha", "beta"] : [], modelCount: i ? 2 : 0, unknownModel: i === 0, direct: summary(i ? "1234567890123" : null, false), directSubagentCount: i ? 1 : null, weeklyPercentageImpact: null })) };
        }
        try {
          if (state.hold) await new Promise(resolve => state.releases.push(resolve));
          if (state.fail) throw "storage";
          return { hierarchyPending: false, hierarchyRevision: 1, coverageNote: "Local accepted usage only.", data: { kind: query.kind, data: result } };
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
  await page.keyboard.press("Escape"); await dialog.waitFor({ state: "hidden" });
  assert.equal(await trigger.evaluate(el => el === document.activeElement), true, "close restores row focus");
  await page.getByRole("button", { name: "Next page", exact: true }).click();
  await page.getByText("2001 sessions · 26–50 shown", { exact: true }).waitFor();
  assert.equal(await page.locator(".session-row").count(), 25, "paging replaces rather than appends rows");
  await page.getByRole("button", { name: "Previous page", exact: true }).click();
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
  console.log("Sessions UI: filters/date validation, bounded paging, exact/unavailable values, keyboard details, live reset, stale suppression, empty/error and responsive layout passed.");
} finally { await browser?.close(); server.kill(); }
