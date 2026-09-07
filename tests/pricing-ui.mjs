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
  const failures = [];
  page.on("pageerror", error => failures.push(error.message));
  await page.addInitScript(() => {
    const names = Array.from({ length: 65 }, (_, i) => `model-${String(i).padStart(3, "0")}`);
    names.push("constructor");
    window.pricingTest = { calls: [], saves: [], failPage: true, failSave: true, versions: Object.create(null) };
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
    window.__TAURI_INTERNALS__ = {
      transformCallback: () => 1,
      invoke: async (command, args) => {
        const state = window.pricingTest;
        if (command === "plugin:event|listen") return 1;
        if (command === "usage_snapshot") return { coverage: "Fixture usage", sourceAvailable: true };
        if (command === "pricing_models") {
          state.calls.push(args.after);
          if (args.after && state.failPage) { state.failPage = false; throw { code: "busy", message: "The monitor is busy. Try again shortly." }; }
          const start = args.after ? names.indexOf(args.after) + 1 : 0;
          const models = names.slice(start, start + 64).map(model => ({ model, latestPrice: state.versions[model] ?? null }));
          return { models, nextCursor: models.length === 64 ? models.at(-1).model : null };
        }
        if (command === "save_model_price") {
          state.saves.push(args);
          if (state.failSave) { state.failSave = false; throw { code: "invalid_rate", field: "input", message: "Enter a nonnegative decimal price within the supported range" }; }
          const version = { id: state.saves.length, model: args.model, configuration: args.configuration, backfillBefore: args.backfillBefore, effectiveSeconds: 1800000000, effectiveNanos: 0 };
          state.versions[args.model] = version;
          return version;
        }
        return null;
      },
    };
  });
  await page.goto("http://127.0.0.1:1421");
  const trigger = page.getByRole("button", { name: "Model Pricing", exact: true });
  await trigger.click();
  const dialog = page.getByRole("dialog", { name: "Model Pricing" });
  await dialog.waitFor();
  await page.keyboard.press("Shift+Tab");
  assert.equal(await dialog.evaluate(el => el.contains(document.activeElement)), true, "modal keeps keyboard focus inside");
  await page.getByText("The model list may be incomplete.", { exact: false }).waitFor();
  await page.getByRole("button", { name: "Retry loading" }).click();
  await page.getByText("66 detected models", { exact: true }).waitFor();
  assert.deepEqual(await page.evaluate(() => window.pricingTest.calls), [null, "model-063", "model-063"]);
  await page.getByLabel("Detected model", { exact: true }).selectOption("constructor");
  await page.getByLabel("Input (USD / 1M)", { exact: true }).fill("1e3");
  await page.getByRole("button", { name: "Save price", exact: true }).click();
  assert.equal(await page.locator("#price-input").getAttribute("aria-invalid"), "true");
  assert.equal(await page.locator("#price-input").evaluate(el => el === document.activeElement), true);
  assert.equal(await page.evaluate(() => window.pricingTest.saves.length), 0);
  for (const [id, value] of [["input", "0001.000001"], ["cachedInput", "0"], ["cacheWrite", "2.25"], ["output", "3"]]) await page.locator(`#price-${id}`).fill(value);
  await page.getByLabel("How should reasoning tokens be priced?").selectOption("separate");
  await page.getByLabel("Reasoning (USD / 1M)", { exact: true }).fill("4.000001");
  await page.getByLabel("How should reasoning tokens be priced?").selectOption("included");
  await page.getByLabel("How do cache-write tokens relate to input?").selectOption("included_input_disjoint");
  await page.getByLabel("Apply this first price", { exact: false }).check();
  await page.getByLabel("Detected model", { exact: true }).selectOption("model-000");
  await page.getByLabel("Detected model", { exact: true }).selectOption("constructor");
  assert.equal(await page.locator("#price-input").inputValue(), "0001.000001");
  await page.keyboard.press("Escape");
  await dialog.waitFor({ state: "hidden" });
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
  assert.equal(saved.configuration.cacheWritePolicy, "included_input_disjoint");
  assert.equal(saved.backfillBefore, true);
  await page.getByLabel("How should reasoning tokens be priced?").selectOption("separate");
  assert.equal(await page.locator("#price-reasoning").inputValue(), "4.000001");
  await page.getByRole("button", { name: "Save price", exact: true }).click();
  await page.getByText("Price saved.", { exact: false }).waitFor();
  assert.equal(await page.evaluate(() => window.pricingTest.saves.at(-1).backfillBefore), false);
  await page.setViewportSize({ width: 390, height: 740 });
  assert.equal(await dialog.evaluate(el => el.scrollWidth <= el.clientWidth), true, "dialog must fit narrow viewport");
  if (process.env.PRICING_SCREENSHOT) await page.screenshot({ path: process.env.PRICING_SCREENSHOT });
  assert.deepEqual(failures, []);
  console.log("Pricing UI: pagination/retry, exact payloads, keyboard/focus, drafts, validation, and future-only edits passed.");
} finally {
  await browser?.close();
  server.kill();
}
