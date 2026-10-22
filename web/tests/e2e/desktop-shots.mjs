// Screenshots of the running desktop app for a PR, over WebView2's CDP port
// (TESTING.md). Not a Playwright test: run it by hand on Windows.
//
//   WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222 pnpm tauri dev
//   node web/tests/e2e/desktop-shots.mjs <out dir> <library root>
import { chromium } from "@playwright/test";
import { mkdirSync } from "node:fs";
import path from "node:path";

const [outDir, root] = process.argv.slice(2);
if (!outDir || !root) {
  console.error("usage: desktop-shots.mjs <out dir> <library root>");
  process.exit(1);
}
mkdirSync(outDir, { recursive: true });
const browser = await chromium.connectOverCDP("http://127.0.0.1:9222");
const context = browser.contexts()[0];
const page = context.pages().find((p) => p.url().includes("localhost:5173")) ?? context.pages()[0];
const origin = new URL(page.url()).origin;
console.log("page:", page.url());

const shot = (name) => page.screenshot({ path: path.join(outDir, `desktop-windows-${name}.png`) });

await page.evaluate((r) => {
  localStorage.setItem("yaoplm.settings", JSON.stringify({ root: r, view: "grid" }));
  localStorage.setItem("yaoplm.language", "en");
  localStorage.setItem("yaoplm.theme", "light");
}, root);

await page.goto(`${origin}/devices`, { waitUntil: "networkidle" });
await page.waitForSelector('[data-testid="device-card"]');
await shot("devices");

await page.goto(`${origin}/library`, { waitUntil: "networkidle" });
await page.waitForSelector('[data-testid="game-grid"]');
await shot("library");

await page.getByRole("listitem", { name: /Retail Game/ }).click();
await page.waitForSelector('[data-testid="game-title"]');
await shot("game");

await page.goto(`${origin}/library/add`, { waitUntil: "networkidle" });
await page.waitForSelector('[data-testid="drop-zone"]');
await shot("add");

await page.goto(`${origin}/settings`, { waitUntil: "networkidle" });
const mode = await page.locator("dd").filter({ hasText: /Desktop app|Приложение/ }).first().textContent();
console.log("mode:", mode);
await browser.close();
