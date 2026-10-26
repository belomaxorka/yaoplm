// Drives the running dev desktop over WebView2's CDP port (TESTING.md):
//   node tests/release/desktop.mjs <command> [args]
// Commands: text | shot <file> [height] | goto <path> | click <selector> |
// eval <js> | tabs <n> | storage <json>. Each call connects, acts, leaves.
import { chromium } from "@playwright/test";

const [command, ...args] = process.argv.slice(2);
const browser = await chromium.connectOverCDP("http://127.0.0.1:9222");
const context = browser.contexts()[0];
const page = context.pages().find((p) => p.url().includes("localhost:5173")) ?? context.pages()[0];
if (!page) throw new Error("no page");

switch (command) {
  case "text": {
    console.log(await page.locator("main").innerText());
    break;
  }
  case "shot": {
    const [file, height] = args;
    if (height) await page.setViewportSize({ width: 1024, height: Number(height) });
    await page.waitForTimeout(400);
    await page.screenshot({ path: file, fullPage: false });
    console.log(`saved ${file}`);
    break;
  }
  case "goto": {
    const url = new URL(args[0], "http://localhost:5173/").toString();
    await page.goto(url);
    await page.waitForTimeout(800);
    console.log(page.url());
    break;
  }
  case "click": {
    await page.locator(args[0]).first().click();
    await page.waitForTimeout(800);
    console.log("clicked");
    break;
  }
  case "key": {
    await page.keyboard.press(args[0]);
    await page.waitForTimeout(500);
    console.log("pressed");
    break;
  }
  case "clicktext": {
    await page.getByRole("button", { name: args[0] }).first().click();
    await page.waitForTimeout(1200);
    console.log("clicked");
    break;
  }
  case "eval": {
    console.log(JSON.stringify(await page.evaluate(args[0]), null, 2));
    break;
  }
  case "storage": {
    await page.evaluate((json) => {
      const current = JSON.parse(localStorage.getItem("yaoplm.settings") ?? "{}");
      localStorage.setItem("yaoplm.settings", JSON.stringify({ ...current, ...JSON.parse(json) }));
    }, args[0]);
    await page.reload();
    await page.waitForTimeout(800);
    console.log("settings updated");
    break;
  }
  case "tabs": {
    const names = [];
    await page.locator("body").click({ position: { x: 1, y: 1 } });
    for (let i = 0; i < Number(args[0] ?? 30); i++) {
      await page.keyboard.press("Tab");
      names.push(
        await page.evaluate(() => {
          const el = document.activeElement;
          if (!el || el === document.body) return "(body)";
          const name = el.getAttribute("aria-label") ?? el.innerText?.trim().split("\n")[0] ?? "";
          return `<${el.tagName.toLowerCase()}> ${name.slice(0, 50)}`;
        }),
      );
    }
    console.log(names.join("\n"));
    break;
  }
  default:
    throw new Error(`unknown command ${command}`);
}
await browser.close();
