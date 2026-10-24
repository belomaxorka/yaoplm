// The UI walkthrough (UI-5) in the running desktop app, over WebView2's
// CDP port (TESTING.md). Not a Playwright test: run it by hand on Windows.
//
//   WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222 pnpm tauri dev
//   node web/tests/e2e/desktop-walkthrough.mjs <out dir> <library root>
//
// Every screen in simple and advanced mode at 800×600 and 1280×800 (the
// page viewport, emulated): no sideways scrolling, no interactive elements
// on top of each other, no text out of its box, dialogs that fit, no
// console errors. Screenshots go to <out dir>. The app's own settings are
// put back afterwards: this runs in the owner's WebView2 profile.
import { chromium } from "@playwright/test";
import { mkdirSync } from "node:fs";
import path from "node:path";

const [outDir, root] = process.argv.slice(2);
if (!outDir || !root) {
  console.error("usage: desktop-walkthrough.mjs <out dir> <library root>");
  process.exit(1);
}
mkdirSync(outDir, { recursive: true });
const browser = await chromium.connectOverCDP("http://127.0.0.1:9222");
const context = browser.contexts()[0];
const page = context.pages().find((p) => p.url().includes("localhost:5173")) ?? context.pages()[0];
const origin = new URL(page.url()).origin;
const errors = [];
page.on("pageerror", (error) => errors.push(error.message));
page.on("console", (message) => {
  if (message.type() === "error") errors.push(message.text());
});

const saved = await page.evaluate(() => ({
  settings: localStorage.getItem("yaoplm.settings"),
  language: localStorage.getItem("yaoplm.language"),
  theme: localStorage.getItem("yaoplm.theme"),
}));

/** The same checks as layoutProblems() in walkthrough.spec.ts. */
const problems = () =>
  page.evaluate(() => {
    const found = [];
    const describe = (el) => `<${el.tagName.toLowerCase()}> "${(el.getAttribute("aria-label") ?? el.textContent ?? "").trim().slice(0, 40)}"`;
    const doc = document.documentElement;
    if (doc.scrollWidth > doc.clientWidth + 1) found.push(`the page scrolls sideways by ${doc.scrollWidth - doc.clientWidth} px`);
    for (const el of document.querySelectorAll("body *")) {
      const style = getComputedStyle(el);
      if ((style.overflowX === "auto" || style.overflowX === "scroll") && el.scrollWidth > el.clientWidth + 1) {
        found.push(`${describe(el)} scrolls sideways`);
      }
    }
    for (const rootEl of [document.getElementById("main"), ...document.querySelectorAll('[role="dialog"]')]) {
      if (!rootEl) continue;
      const walker = document.createTreeWalker(rootEl, NodeFilter.SHOW_TEXT);
      for (let node = walker.nextNode(); node; node = walker.nextNode()) {
        if (!node.textContent?.trim()) continue;
        const range = document.createRange();
        range.selectNodeContents(node);
        const text = range.getBoundingClientRect();
        if (text.width === 0) continue;
        for (let el = node.parentElement; el && el !== rootEl.parentElement; el = el.parentElement) {
          const style = getComputedStyle(el);
          if (style.overflowX !== "visible") break;
          if (style.display === "contents") continue;
          const outer = el.getBoundingClientRect();
          const over = Math.max(text.right - outer.right, outer.left - text.left);
          if (over > 1) {
            found.push(`${describe(el)} text spills ${Math.round(over)} px`);
            break;
          }
        }
      }
    }
    const selector = 'a[href], button, input, select, textarea, [role="button"], [role="radio"], [role="checkbox"], [role="switch"], [role="tab"], [role="combobox"], [role="menuitem"]';
    const dialog = document.querySelector('[role="dialog"], [role="alertdialog"]');
    const shown = [...document.querySelectorAll(selector)].filter((el) => {
      const r = el.getBoundingClientRect();
      return r.width > 0 && r.height > 0 && getComputedStyle(el).visibility !== "hidden" && !el.closest("[aria-hidden=true], [inert]") && (!dialog || dialog.contains(el));
    });
    for (let i = 0; i < shown.length; i++) {
      const ra = shown[i].getBoundingClientRect();
      for (let j = i + 1; j < shown.length; j++) {
        if (shown[i].contains(shown[j]) || shown[j].contains(shown[i])) continue;
        const rb = shown[j].getBoundingClientRect();
        if (Math.min(ra.right, rb.right) - Math.max(ra.left, rb.left) > 1 && Math.min(ra.bottom, rb.bottom) - Math.max(ra.top, rb.top) > 1) {
          found.push(`${describe(shown[i])} overlaps ${describe(shown[j])}`);
        }
      }
    }
    if (dialog) {
      const r = dialog.getBoundingClientRect();
      if (r.top < 0 || r.bottom > innerHeight + 1) found.push("a dialog does not fit the window");
    }
    return found;
  });

const report = [];
try {
  for (const simple of [true, false]) {
    for (const [width, height] of [
      [800, 600],
      [1280, 800],
    ]) {
      const tag = `${simple ? "simple" : "advanced"}-${width}`;
      await page.setViewportSize({ width, height });
      await page.evaluate(
        ([r, s]) => {
          localStorage.setItem("yaoplm.settings", JSON.stringify({ ...JSON.parse(localStorage.getItem("yaoplm.settings") ?? "{}"), root: r, simpleMode: s }));
          localStorage.setItem("yaoplm.language", "ru");
        },
        [root, simple],
      );
      const visit = async (name, url, ready) => {
        await page.goto(`${origin}${url}`, { waitUntil: "networkidle" });
        await page.waitForSelector(ready, { timeout: 60_000 });
        await page.waitForTimeout(500);
        await page.screenshot({ path: path.join(outDir, `desktop-${tag}-${name}.png`) });
        for (const problem of await problems()) report.push(`${tag} ${name}: ${problem}`);
      };
      await visit("devices", "/devices", '[data-testid="device-card"]');
      await visit("library", "/library", '[data-testid="game-grid"] [role="listitem"]');
      await page.locator('[data-testid="game-grid"] [role="listitem"]').first().click();
      await page.waitForSelector('[data-testid="game-title"]');
      await page.waitForTimeout(500);
      await page.screenshot({ path: path.join(outDir, `desktop-${tag}-game.png`) });
      for (const problem of await problems()) report.push(`${tag} game: ${problem}`);
      await visit("add", "/library/add", '[data-testid="drop-zone"]');
      await visit("settings", "/settings", '[data-testid="about"]');
    }
  }
} finally {
  await page.evaluate((s) => {
    for (const [key, value] of Object.entries({ "yaoplm.settings": s.settings, "yaoplm.language": s.language, "yaoplm.theme": s.theme })) {
      if (value === null) localStorage.removeItem(key);
      else localStorage.setItem(key, value);
    }
  }, saved);
  await page.goto(`${origin}/library`);
}
console.log(report.length === 0 ? "no layout problems" : report.join("\n"));
console.log(errors.length === 0 ? "no console errors" : `console errors:\n${errors.join("\n")}`);
if (report.length > 0 || errors.length > 0) process.exitCode = 1;
await browser.close();
