// Focused browser contract check. Uses an installed Playwright via PLAYWRIGHT_MODULE
// (absolute package entry path) or a locally available playwright package.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { pathToFileURL } from "node:url";
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE ? pathToFileURL(process.env.PLAYWRIGHT_MODULE).href : "playwright");
const server = spawn(process.execPath, ["node_modules/vite/bin/vite.js", "--host", "127.0.0.1", "--port", "1421", "--strictPort"], { windowsHide: true, stdio: "pipe" });
let browser;
try {
  await new Promise((resolve, reject) => {
    const deadline = setTimeout(() => reject(new Error("Vite did not start")), 15000);
    server.stdout.on("data", data => { if (data.toString().includes("1421")) { clearTimeout(deadline); resolve(); } });
    server.on("exit", code => { clearTimeout(deadline); reject(new Error(`Vite exited ${code}`)); });
  });
  browser = await chromium.launch({ channel: "msedge", headless: true });
  const page = await browser.newPage({ viewport: { width: 900, height: 850 } });
  page.setDefaultTimeout(10000);
  const failures = [];
  page.on("pageerror", error => failures.push(error.message));
  await page.addInitScript(() => {
    const names = Array.from({ length: 65 }, (_, i) => `model-${String(i).padStart(3, "0")}`);
    names.push("constructor"); names.sort();
    const versions = Object.create(null);
    versions["model-000"] = { id: 800, model: "model-000", configuration: { input: "1", cachedInput: "0.5", cacheWrite: "2", output: "3", reasoning: null, reasoningPolicy: "included", cacheWritePolicy: "additional" }, backfillBefore: false, effectiveSeconds: 1700000000, effectiveNanos: 0 };
    const state = window.pricingTest = { names, calls: [], saves: [], backfills: [], callbacks: {}, listeners: {}, releases: [], holdPage: false, holdSave: false, activeReads: 0, maxReads: 0, failPage: true, failSave: true, versions, backfillAvailable: new Set(["model-000"]) };
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
    window.__TAURI_INTERNALS__ = {
      transformCallback: callback => { const id = Object.keys(state.callbacks).length + 1; state.callbacks[id] = callback; return id; },
      invoke: async (command, args) => {
        const state = window.pricingTest;
        if (command === "plugin:event|listen") { state.listeners[args.handler] = args.event; return args.handler; }
        if (command === "plugin:event|unlisten") { delete state.listeners[args.eventId]; return; }
        if (command === "usage_snapshot") return { coverage: "Fixture usage", sourceAvailable: true };
        if (command === "pricing_models") {
          state.calls.push(args.after);
          if (args.after && state.failPage) { state.failPage = false; throw { code: "busy", message: "The monitor is busy. Try again shortly." }; }
          const start = args.after ? names.indexOf(args.after) + 1 : 0;
          const models = names.slice(start, start + 64).map(model => ({ model, latestPrice: state.versions[model] ?? null, backfillAvailable: state.backfillAvailable.has(model) }));
          state.activeReads++; state.maxReads = Math.max(state.maxReads, state.activeReads);
          if (args.after && state.holdPage) { state.holdPage = false; await new Promise(resolve => state.releases.push(resolve)); }
          state.activeReads--;
          return { models, nextCursor: models.length === 64 ? models.at(-1).model : null };
        }
        if (command === "save_model_price") {
          state.saves.push(args);
          if (state.holdSave) { state.holdSave = false; await new Promise(resolve => state.releases.push(resolve)); }
          if (state.failSave) { state.failSave = false; throw { code: "invalid_rate", field: "input", message: "Enter a nonnegative decimal price within the supported range" }; }
          const version = { id: state.saves.length, model: args.model, configuration: args.configuration, backfillBefore: args.backfillBefore, effectiveSeconds: 1800000000, effectiveNanos: 0 };
          state.versions[args.model] = version;
          state.backfillAvailable.delete(args.model);
          return version;
        }
        if (command === "backfill_model_price") {
          state.backfills.push(args);
          state.backfillAvailable.delete(args.model);
          return state.versions[args.model];
        }
        return null;
      },
    };
  });
  await page.goto("http://127.0.0.1:1421", { timeout: 30000 });
  const trigger = page.getByRole("button", { name: "Model Pricing", exact: true });
  const listenersBeforeOpen = await page.evaluate(() => Object.keys(window.pricingTest.listeners).length);
  await trigger.click();
  const dialog = page.getByRole("dialog", { name: "Model Pricing" });
  await dialog.waitFor();
  await page.keyboard.press("Shift+Tab");
  assert.equal(await dialog.evaluate(el => el.contains(document.activeElement)), true, "modal keeps keyboard focus inside");
  await page.getByText("The model list may be incomplete.", { exact: false }).waitFor();
  await page.getByRole("button", { name: "Retry loading" }).click();
  await page.getByText("66 detected models", { exact: true }).waitFor();
  assert.deepEqual(await page.evaluate(() => window.pricingTest.calls), [null, "model-062", "model-062"]);
  const choose = name => dialog.getByRole("list", { name: "Detected models", exact: true }).getByRole("button", { name: new RegExp(`^${name} (?:Configured|Unpriced)$`) }).click();
  await choose("constructor");
  await page.getByLabel("Input (USD / 1M)", { exact: true }).fill("1e3");
  await page.getByRole("button", { name: "Save price", exact: true }).click();
  assert.equal(await page.locator("#price-input").getAttribute("aria-invalid"), "true");
  assert.equal(await page.locator("#price-input").evaluate(el => el === document.activeElement), true);
  assert.equal(await page.evaluate(() => window.pricingTest.saves.length), 0);
  for (const [id, value] of [["input", "0001.000001"], ["cachedInput", "0"], ["cacheWrite", "2.25"], ["output", "3"]]) await page.locator(`#price-${id}`).fill(value);
  assert.equal(await page.locator("#price-reasoning").count(), 0, "reasoning is priced inside output; no separate field");
  await page.getByLabel("Apply this first price", { exact: false }).check();
  // Metadata-only model discovery during a paged refresh must trigger a full trailing scan.
  await page.evaluate(() => { window.pricingTest.holdPage = true; });
  await dialog.getByRole("button", { name: "Reload models" }).click();
  await page.waitForFunction(() => window.pricingTest.releases.length === 1);
  await page.evaluate(() => {
    const s = window.pricingTest;
    s.names.push("a-future-model"); s.names.sort();
    for (let i = 0; i < 3; i++) for (const [id, event] of Object.entries(s.listeners)) if (event === "usage-updated") s.callbacks[id]({ payload: { coverage: "Metadata imported", sourceAvailable: true } });
    s.releases.splice(0).forEach(resolve => resolve());
  });
  await dialog.getByText("67 detected models", { exact: true }).waitFor();
  assert.equal(await dialog.getByRole("button", { name: "constructor Unpriced", exact: true }).getAttribute("aria-pressed"), "true");
  assert.equal(await page.locator("#price-input").inputValue(), "0001.000001");
  assert.equal(await page.evaluate(() => window.pricingTest.maxReads), 1);
  const search = dialog.getByRole("searchbox", { name: "Search detected models" });
  await search.fill("FUTURE");
  assert.deepEqual(await dialog.getByRole("list", { name: "Detected models", exact: true }).getByRole("button").allTextContents(), ["a-future-modelUnpriced"]);
  await search.fill("");
  await choose("model-000");
  await choose("constructor");
  assert.equal(await page.locator("#price-input").inputValue(), "0001.000001");
  await page.keyboard.press("Escape");
  await dialog.waitFor({ state: "hidden" });
  await page.waitForFunction(expected => Object.keys(window.pricingTest.listeners).length === expected, listenersBeforeOpen);
  assert.equal(await trigger.evaluate(el => el === document.activeElement), true);
  await trigger.click();
  await page.getByRole("button", { name: "Save price", exact: true }).waitFor();
  await page.getByRole("button", { name: "Save price", exact: true }).click();
  await page.getByRole("alert").filter({ hasText: "Enter a nonnegative decimal price within" }).waitFor();
  assert.equal(await page.locator("#price-input").inputValue(), "0001.000001");
  await page.getByRole("button", { name: "Save price", exact: true }).click();
  await page.getByText("Price saved.", { exact: false }).waitFor();
  assert.equal(await dialog.isVisible(), true);
  assert.equal(await page.getByRole("checkbox").count(), 0);
  const saved = await page.evaluate(() => window.pricingTest.saves.at(-1));
  assert.equal(saved.configuration.input, "0001.000001");
  assert.equal(saved.configuration.reasoning, null);
  assert.equal(saved.configuration.reasoningPolicy, "included");
  assert.equal(saved.configuration.cacheWritePolicy, "additional");
  assert.equal(saved.backfillBefore, true);
  await choose("model-000");
  await dialog.getByRole("button", { name: "Backfill older unpriced usage", exact: true }).click();
  await dialog.getByText("Use this model’s first saved price", { exact: false }).waitFor();
  assert.equal(await page.evaluate(() => window.pricingTest.backfills.length), 0);
  await dialog.getByRole("button", { name: "Confirm backfill", exact: true }).click();
  await dialog.getByText("Historical backfill scheduled.", { exact: false }).waitFor();
  assert.deepEqual(await page.evaluate(() => window.pricingTest.backfills), [{ model: "model-000" }]);
  await choose("constructor");
  assert.equal(await page.locator("#price-cacheWrite").inputValue(), "2.25", "draft survives switching models after a backfill");
  await page.evaluate(() => { window.pricingTest.holdSave = true; });
  await page.getByRole("button", { name: "Save price", exact: true }).click();
  await page.waitForFunction(() => window.pricingTest.releases.length === 1);
  const callsBeforeSaveEvent = await page.evaluate(() => window.pricingTest.calls.length);
  await page.evaluate(() => {
    const s = window.pricingTest;
    s.names.push("another-future-model"); s.names.sort();
    for (const [id, event] of Object.entries(s.listeners)) if (event === "usage-updated") s.callbacks[id]({ payload: { sourceAvailable: true } });
  });
  assert.equal(await page.evaluate(() => window.pricingTest.calls.length), callsBeforeSaveEvent, "catalog refresh waits for save");
  await page.evaluate(() => window.pricingTest.releases.splice(0).forEach(resolve => resolve()));
  await page.getByText("Price saved.", { exact: false }).waitFor();
  await dialog.getByText("68 detected models", { exact: true }).waitFor();
  assert.equal(await page.evaluate(() => window.pricingTest.saves.at(-1).backfillBefore), false);
  if (process.env.PRICING_SCREENSHOT) {
    await dialog.evaluate(element => { element.scrollTop = 0; });
    await page.screenshot({ path: process.env.PRICING_SCREENSHOT.replace(".png", "-desktop.png") });
  }
  await page.setViewportSize({ width: 390, height: 740 });
  assert.equal(await dialog.evaluate(el => el.scrollWidth <= el.clientWidth), true, "dialog must fit narrow viewport");
  if (process.env.PRICING_SCREENSHOT) await page.screenshot({ path: process.env.PRICING_SCREENSHOT });
  assert.deepEqual(failures, []);
  console.log("Pricing UI: pagination/retry, live future-model discovery during paging/saving, search, subscription cleanup, exact payloads, keyboard/focus, drafts, validation, and future-only edits passed.");
} finally {
  await browser?.close();
  server.kill();
}
