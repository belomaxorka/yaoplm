// Screenshots of the running desktop app for a PR, over WebView2's CDP port
// (TESTING.md). Not a Playwright test: run it by hand on Windows.
//
//   WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222 pnpm tauri dev
//   node web/tests/e2e/desktop-shots.mjs <out dir> <library root> [<mirror root>]
//
// With a mirror root (the `mirror` folder of `make_library`) the cover
// sources read it instead of the network, and the script checks that a
// cover really loads through the asset protocol (ART-2).
import { chromium } from "@playwright/test";
import { mkdirSync } from "node:fs";
import path from "node:path";

const [outDir, root, mirror] = process.argv.slice(2);
if (!outDir || !root) {
  console.error("usage: desktop-shots.mjs <out dir> <library root> [<mirror root>]");
  process.exit(1);
}
mkdirSync(outDir, { recursive: true });
const browser = await chromium.connectOverCDP("http://127.0.0.1:9222");
const context = browser.contexts()[0];
const page = context.pages().find((p) => p.url().includes("localhost:5173")) ?? context.pages()[0];
const origin = new URL(page.url()).origin;
console.log("page:", page.url());

const shot = (name) => page.screenshot({ path: path.join(outDir, `desktop-windows-${name}.png`) });

await page.evaluate(
  ([r, m]) => {
    const artSources = m
      ? {
          xlenore: { enabled: true, mirror: `${m}\\xlenore` },
          libretro: { enabled: true, mirror: `${m}\\libretro` },
        }
      : undefined;
    // Advanced mode: the picture sets are one of its tools.
    localStorage.setItem("yaoplm.settings", JSON.stringify({ root: r, view: "grid", simpleMode: false, ...(artSources ? { artSources } : {}) }));
    localStorage.setItem("yaoplm.language", "en");
    localStorage.setItem("yaoplm.theme", "light");
  },
  [root, mirror ?? ""],
);

await page.goto(`${origin}/devices`, { waitUntil: "networkidle" });
await page.waitForSelector('[data-testid="device-card"]');
await shot("devices");

await page.goto(`${origin}/library`, { waitUntil: "networkidle" });
await page.waitForSelector('[data-testid="game-grid"]');
// A cover from the device must load through asset: (not the placeholder).
const covered = page.locator('[data-testid="game-cover"][data-has-cover="true"] img').first();
await covered.waitFor({ state: "visible", timeout: 15_000 });
const loaded = await covered.evaluate((img) => img.complete && img.naturalWidth > 0);
console.log("cover loaded through the asset protocol:", loaded, await covered.getAttribute("src"));
if (!loaded) process.exitCode = 1;
await shot("library");

// Picture sets come from native pickers or a drop on the desktop (ART-5).
await page.getByTestId("library-more").click();
await page.getByTestId("import-pictures").click();
await page.waitForSelector('[data-testid="art-import-dialog"]');
await page.waitForTimeout(400);
await shot("art-import");
await page.keyboard.press("Escape");

await page.getByRole("listitem", { name: /Retail Game/ }).click();
await page.waitForSelector('[data-testid="game-title"]');
await page.waitForSelector('[data-testid="art-slots"]');
await shot("game");

if (mirror) {
  // The cover's variants from the mirrors, up to the plan dialog.
  await page.getByTestId("art-choose-COV").click();
  await page.getByTestId("art-variant").first().waitFor({ timeout: 30_000 });
  await shot("art-choose");
  await page.getByTestId("art-variant-use").click();
  await page.getByTestId("plan-dialog").waitFor({ timeout: 30_000 });
  await shot("art-plan");
  await page.keyboard.press("Escape");
}

await page.goto(`${origin}/settings`, { waitUntil: "networkidle" });
await page.waitForSelector('[data-testid="about"]');
await shot("settings");
const mode = await page.locator("dd").filter({ hasText: /Desktop app|Приложение/ }).first().textContent();
console.log("mode:", mode);
await browser.close();
