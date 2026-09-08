// Real Settings workflows through the IPC boundary; filesystem and CSV contents are covered in Rust.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { pathToFileURL } from "node:url";
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE ? pathToFileURL(process.env.PLAYWRIGHT_MODULE).href : "playwright");
const server = spawn(process.execPath, ["node_modules/vite/bin/vite.js", "--host", "127.0.0.1", "--port", "1426", "--strictPort"], { windowsHide: true, stdio: "pipe" });
let browser, page;
try {
  await new Promise((resolve, reject) => {
    const deadline = setTimeout(() => reject(new Error("Vite did not start")), 15000);
    server.stdout.on("data", data => { if (data.toString().includes("1426")) { clearTimeout(deadline); resolve(); } });
    server.on("exit", code => { clearTimeout(deadline); reject(new Error(`Vite exited ${code}`)); });
  });
  browser = await chromium.launch({ channel: "msedge", headless: true });
  page = await browser.newPage({ viewport: { width: 1280, height: 1100 } });
  page.setDefaultTimeout(10000);
  const failures = [];
  page.on("pageerror", error => failures.push(error.message));
  await page.addInitScript(() => {
    const s = window.settingsTest = {
      callbacks: {}, settings: { codex_directory_override: null, automatic_directory: "C:\\Users\\sample\\.codex", monitored_directory: "C:\\Users\\sample\\.codex", autostart: false, tray_enabled: true, close_to_tray: false },
      calls: [], settingsFailure: false, saveError: null, exportError: null, hold: false, releases: [], diagnosticsEmpty: false,
    };
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
    window.__TAURI_INTERNALS__ = {
      transformCallback: callback => { const id = Object.keys(s.callbacks).length + 1; s.callbacks[id] = callback; return id; },
      invoke: async (command, args) => {
        if (command === "plugin:event|listen") return args.handler;
        if (command === "plugin:event|unlisten") return;
        if (command === "usage_snapshot") return { sourceAvailable: true };
        if (command === "usage_dashboard") throw "storage";
        if (command === "pricing_models") return { models: [], nextCursor: null };
        s.calls.push({ command, args: structuredClone(args) });
        if (command === "tracker_settings") {
          if (s.settingsFailure) throw { code: "storage" };
          return structuredClone(s.settings);
        }
        if (command === "save_tracker_settings") {
          if (s.hold) await new Promise(resolve => s.releases.push(resolve));
          if (s.saveError) throw { code: s.saveError, message: "Do not reveal arbitrary backend error content" };
          s.settings = { ...s.settings, ...args.settings, monitored_directory: args.settings.codex_directory_override ?? s.settings.automatic_directory };
          return structuredClone(s.settings);
        }
        if (command === "tracker_diagnostics") return {
          database_path: "C:\\Users\\sample\\AppData\\Roaming\\codex-tracker\\usage.sqlite", database_size_bytes: 8192,
          tracked_session_count: 42, usage_record_count: 1234, monitored_directory: s.settings.monitored_directory,
          last_successful_ingestion_at_ms: s.diagnosticsEmpty ? null : 1788800000000,
          source_available: !s.diagnosticsEmpty, error: s.diagnosticsEmpty ? "Codex source is unavailable." : null,
        };
        if (command === "export_usage_csv") {
          if (s.hold) await new Promise(resolve => s.releases.push(resolve));
          if (s.exportError) throw s.exportError;
          return { path: args.request.destination, row_count: 42 };
        }
        throw new Error(`Unexpected command ${command}`);
      },
    };
  });
  await page.goto("http://127.0.0.1:1426/", { timeout: 30000 });
  const navigation = page.getByRole("button", { name: "Settings", exact: true });
  await navigation.focus(); await page.keyboard.press("Enter");
  const settings = page.getByRole("region", { name: "Settings", exact: true });
  const save = settings.getByRole("button", { name: "Save settings", exact: true });
  await save.waitFor();
  assert.equal(await navigation.getAttribute("aria-pressed"), "true");
  assert.match(await settings.innerText(), /8,192 bytes[\s\S]*42[\s\S]*1,234/);
  assert.match(await settings.innerText(), /These preferences are saved only/);
  assert.equal(await settings.getByText(/sampling/i).count(), 0);
  await settings.getByRole("button", { name: "Configure model prices" }).click();
  await page.getByRole("dialog", { name: "Model Pricing" }).waitFor();
  await page.getByRole("dialog").getByRole("button", { name: "Close", exact: true }).click();

  // Both local validation and a filesystem rejection keep the entered directory for correction.
  await settings.getByText("Override automatic discovery with a local directory.", { exact: true }).click();
  const directory = settings.getByRole("textbox", { name: "Codex home directory", exact: true });
  await directory.fill("relative-folder"); await save.click();
  assert.equal(await directory.getAttribute("aria-invalid"), "true");
  assert.equal(await directory.inputValue(), "relative-folder");
  assert.equal(await page.evaluate(() => window.settingsTest.calls.filter(call => call.command === "save_tracker_settings").length), 0);
  await directory.fill("C:\\Missing\\.codex");
  await page.evaluate(() => { window.settingsTest.saveError = "invalid_directory"; });
  await save.click();
  await settings.locator("#codex-directory-error").waitFor();
  assert.equal(await directory.inputValue(), "C:\\Missing\\.codex");
  assert.equal(await directory.evaluate(el => el === document.activeElement), true);
  assert.doesNotMatch(await settings.innerText(), /Do not reveal arbitrary backend error content/);
  await directory.fill("D:\\Codex history");
  await settings.getByText("Start when I sign in", { exact: true }).click();
  await settings.getByText("Close window to tray", { exact: true }).click();
  await page.evaluate(() => { const s = window.settingsTest; s.saveError = null; s.hold = true; });
  await save.click();
  await page.waitForFunction(() => window.settingsTest.releases.length === 1);
  assert.equal(await settings.getByRole("button", { name: "Saving…", exact: true }).isDisabled(), true);
  assert.equal(await directory.isDisabled(), true);
  await page.evaluate(() => { const s = window.settingsTest; s.hold = false; s.releases.splice(0).forEach(resolve => resolve()); });
  await settings.getByRole("status").filter({ hasText: "Settings saved." }).waitFor();
  assert.deepEqual(await page.evaluate(() => window.settingsTest.calls.filter(call => call.command === "save_tracker_settings").at(-1).args.settings), { codex_directory_override: "D:\\Codex history", autostart: true, tray_enabled: true, close_to_tray: true });
  await page.getByRole("button", { name: "Dashboard", exact: true }).click(); await navigation.click();
  await directory.waitFor();
  assert.equal(await directory.inputValue(), "D:\\Codex history");
  assert.equal(await settings.getByRole("checkbox", { name: /Start when I sign in/ }).isChecked(), true);
  await settings.getByText("Enable system tray", { exact: true }).click();
  assert.equal(await settings.getByRole("checkbox", { name: /Close window to tray/ }).isChecked(), false);
  assert.equal(await settings.getByRole("checkbox", { name: /Close window to tray/ }).isDisabled(), true);
  await settings.getByRole("radio", { name: /Discover automatically/ }).check(); await save.click();
  await settings.getByRole("status").filter({ hasText: "Settings saved." }).waitFor();
  assert.equal(await page.evaluate(() => window.settingsTest.settings.codex_directory_override), null);
  await page.evaluate(() => { window.settingsTest.diagnosticsEmpty = true; });
  await settings.getByRole("button", { name: "Refresh diagnostics" }).click();
  await settings.getByText("No successful ingestion recorded", { exact: true }).waitFor();
  assert.match(await settings.innerText(), /Codex source is unavailable/);

  const destination = settings.getByRole("textbox", { name: "Destination CSV file" });
  const exportButton = settings.getByRole("button", { name: "Export CSV", exact: true });
  await destination.fill("relative.csv"); await exportButton.click();
  assert.equal(await destination.getAttribute("aria-invalid"), "true");
  assert.equal(await page.evaluate(() => window.settingsTest.calls.filter(call => call.command === "export_usage_csv").length), 0);
  await destination.fill("C:\\Exports\\sessions.txt"); await exportButton.click();
  assert.equal(await destination.getAttribute("aria-invalid"), "true");
  assert.equal(await page.evaluate(() => window.settingsTest.calls.filter(call => call.command === "export_usage_csv").length), 0);
  await destination.fill("C:\\Exports\\sessions.csv");
  await page.evaluate(() => { const s = window.settingsTest; s.hold = true; s.exportError = "destination_exists"; });
  await exportButton.click();
  await page.waitForFunction(() => window.settingsTest.releases.length === 1);
  assert.equal(await settings.getByRole("button", { name: "Exporting…", exact: true }).isDisabled(), true);
  assert.equal(await destination.isDisabled(), true);
  await page.evaluate(() => { const s = window.settingsTest; s.hold = false; s.releases.splice(0).forEach(resolve => resolve()); });
  await settings.locator("#export-path-error").waitFor();
  assert.equal(await destination.inputValue(), "C:\\Exports\\sessions.csv");
  assert.equal(await destination.evaluate(el => el === document.activeElement), true);
  await page.evaluate(() => { window.settingsTest.exportError = null; });
  for (const kind of ["sessions", "model_usage", "weekly_cycles", "project_totals"]) {
    await settings.getByRole("combobox", { name: "Data to export" }).selectOption(kind);
    await destination.fill(`C:\\Exports\\${kind}-new.csv`); await exportButton.click();
    await settings.getByRole("status").filter({ hasText: `Exported 42 rows to C:\\Exports\\${kind}-new.csv` }).waitFor();
    assert.deepEqual(await page.evaluate(() => window.settingsTest.calls.filter(call => call.command === "export_usage_csv").at(-1).args.request), { kind, destination: `C:\\Exports\\${kind}-new.csv` });
  }
  if (process.env.SETTINGS_SCREENSHOT) await page.screenshot({ path: process.env.SETTINGS_SCREENSHOT.replace(".png", "-desktop.png"), fullPage: true });
  await page.setViewportSize({ width: 390, height: 850 });
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true);
  if (process.env.SETTINGS_SCREENSHOT) await page.screenshot({ path: process.env.SETTINGS_SCREENSHOT, fullPage: true });
  await page.getByRole("button", { name: "Dashboard", exact: true }).click();
  await page.evaluate(() => { window.settingsTest.settingsFailure = true; }); await navigation.click();
  await settings.getByRole("button", { name: "Retry settings" }).waitFor();
  await page.evaluate(() => { window.settingsTest.settingsFailure = false; });
  await settings.getByRole("button", { name: "Retry settings" }).click(); await save.waitFor();
  assert.deepEqual(failures, []);
  console.log("Settings UI passed: keyboard navigation, pricing entry, source validation and retry, saved preferences and remount, pending controls, automatic fallback, diagnostics empty/error states, all four CSV workflows, export failure recovery, and narrow layout. Transport mocked.");
} catch (error) { console.error(await page?.locator("body").innerText()); throw error; }
finally { await browser?.close(); server.kill(); }
