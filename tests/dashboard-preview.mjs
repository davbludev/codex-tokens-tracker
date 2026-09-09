// Screenshots the dashboard against a realistic fixture, for design review.
// Writes docs/preview/dashboard-*.png. Not a correctness check.
import { spawn } from "node:child_process";
import { mkdir } from "node:fs/promises";
import { pathToFileURL } from "node:url";
import { installDashboardFixture } from "./dashboard-fixture.mjs";

const { chromium } = await import(process.env.PLAYWRIGHT_MODULE ? pathToFileURL(process.env.PLAYWRIGHT_MODULE).href : "playwright");
const out = new URL("../docs/preview/", import.meta.url);
await mkdir(out, { recursive: true });
const server = spawn(process.execPath, ["node_modules/vite/bin/vite.js", "--host", "127.0.0.1", "--port", "1423", "--strictPort"], { windowsHide: true, stdio: "pipe" });
let browser;
try {
  await new Promise((resolve, reject) => {
    const deadline = setTimeout(() => reject(new Error("Vite did not start")), 20000);
    server.stdout.on("data", data => { if (data.toString().includes("1423")) { clearTimeout(deadline); resolve(); } });
    server.on("exit", code => { clearTimeout(deadline); reject(new Error(`Vite exited ${code}`)); });
  });
  browser = await chromium.launch({ channel: "msedge", headless: true });
  const page = await browser.newPage({ viewport: { width: 1440, height: 1000 }, deviceScaleFactor: 1.5 });
  page.on("pageerror", error => console.error("PAGE ERROR:", error.message));
  page.on("console", message => { if (message.type() === "error") console.error("CONSOLE:", message.text()); });
  await page.addInitScript(installDashboardFixture);
  await page.goto("http://127.0.0.1:1423");
  await page.locator(".dashboard-summary").waitFor({ timeout: 20000 });
  await page.waitForTimeout(1200);
  await page.screenshot({ path: new URL("dashboard-full.png", out).pathname.slice(1), fullPage: true });
  await page.screenshot({ path: new URL("dashboard-top.png", out).pathname.slice(1) });
  const shot = async (selector, name) => {
    const target = page.locator(selector).first();
    if (await target.count() === 0) return console.error("missing selector", selector);
    await target.scrollIntoViewIfNeeded();
    await page.waitForTimeout(300);
    await target.screenshot({ path: new URL(name, out).pathname.slice(1) });
  };
  await shot(".model-costs", "dashboard-model-costs.png");
  await shot(".turn-activity", "dashboard-turns.png");
  await shot(".usage-breakdowns", "dashboard-breakdowns.png");
  await shot(".weekly-overview", "dashboard-weekly.png");
  await page.locator(".dashboard-research > summary").click();
  await page.waitForTimeout(800);
  await shot(".quota-analysis", "dashboard-quota.png");
  await shot(".quota-categories", "dashboard-categories.png");
  await page.screenshot({ path: new URL("dashboard-advanced-open.png", out).pathname.slice(1), fullPage: true });
  await page.locator(".dashboard-research > summary").click();
  const narrow = await browser.newPage({ viewport: { width: 900, height: 1000 }, deviceScaleFactor: 1.5 });
  await narrow.addInitScript(installDashboardFixture);
  await narrow.goto("http://127.0.0.1:1423");
  await narrow.locator(".dashboard-summary").waitFor({ timeout: 20000 });
  await narrow.waitForTimeout(1200);
  await narrow.locator(".dashboard-research > summary").click();
  await narrow.waitForTimeout(800);
  await narrow.screenshot({ path: new URL("dashboard-narrow.png", out).pathname.slice(1), fullPage: true });
  console.log("preview screenshots written to docs/preview");
} finally {
  await browser?.close();
  server.kill();
}
