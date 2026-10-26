import AxeBuilder from "@axe-core/playwright";
import { expect, type Page, test } from "@playwright/test";

import { openLibrary } from "./helpers";

// The UI at every window size (UI-5, docs/plans/m5-ui-simple-mode.md).

interface Box {
  x: number;
  y: number;
  width: number;
  height: number;
}

function overlap(a: Box, b: Box): boolean {
  // Sub-pixel layout rounding is not an overlap.
  const e = 0.5;
  return a.x + a.width - e > b.x && b.x + b.width - e > a.x && a.y + a.height - e > b.y && b.y + b.height - e > a.y;
}

/** Every card of the grid with the boxes of its cover and text, in page coordinates. */
async function cardBoxes(page: Page): Promise<{ card: Box; parts: Box[] }[]> {
  return page.getByTestId("game-grid").getByRole("listitem").evaluateAll((cards) =>
    cards.map((card) => {
      const box = (el: Element) => {
        const r = el.getBoundingClientRect();
        return { x: r.x, y: r.y, width: r.width, height: r.height };
      };
      const parts = [...card.children].flatMap((child) => [child, ...child.querySelectorAll("*")]);
      return { card: box(card), parts: parts.map(box).filter((b) => b.width > 0 && b.height > 0) };
    }),
  );
}

test("library grid: cards never overlap and their text stays inside, from 800 to 2560 px", async ({ page, request }) => {
  await openLibrary(page, request);
  await page.setViewportSize({ width: 800, height: 600 });
  await page.goto("/library");
  await expect(page.getByTestId("game-grid").getByRole("listitem").first()).toBeVisible();

  // An odd step lands on many different column counts and leftover widths.
  for (let width = 800; width <= 2560; width += 37) {
    await page.setViewportSize({ width, height: Math.round(width * 0.6) });
    // The grid re-lays out on the next frame after the resize.
    await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))));
    const cards = await cardBoxes(page);
    expect(cards.length, `cards at ${width} px`).toBeGreaterThan(0);
    for (const [i, { card, parts }] of cards.entries()) {
      for (const part of parts) {
        // The hover zoom of the cover is not in effect: the mouse is elsewhere.
        expect(part.y + part.height, `card ${i} content bottom at ${width} px`).toBeLessThanOrEqual(card.y + card.height + 0.5);
        expect(part.x + part.width, `card ${i} content right edge at ${width} px`).toBeLessThanOrEqual(card.x + card.width + 0.5);
      }
      for (const other of cards.slice(i + 1)) {
        expect(overlap(card, other.card), `cards ${i} and another overlap at ${width} px`).toBe(false);
      }
    }
    const scroll = await page.evaluate(() => ({
      page: document.documentElement.scrollWidth - document.documentElement.clientWidth,
      main: (() => {
        const main = document.getElementById("main");
        return main ? main.scrollWidth - main.clientWidth : 0;
      })(),
    }));
    expect(scroll, `horizontal scroll at ${width} px`).toEqual({ page: 0, main: 0 });
  }
});

const sizes = [
  [800, 600],
  [1024, 768],
  [1280, 800],
  [1366, 768],
  [1920, 1080],
  [2560, 1440],
] as const;

/**
 * What a screen must not have at any size (UI-5): a sideways scrollbar,
 * interactive elements on top of each other, or a dialog taller than the
 * window. Returns one line per problem.
 */
async function layoutProblems(page: Page): Promise<string[]> {
  // Animations (dialogs zooming in) settle first.
  await page.evaluate(() => new Promise((resolve) => setTimeout(resolve, 250)));
  return page.evaluate(() => {
    const problems: string[] = [];
    const describe = (el: Element) => {
      const name = el.getAttribute("aria-label") ?? el.textContent?.trim().slice(0, 40) ?? "";
      const id = el.getAttribute("data-testid");
      return `<${el.tagName.toLowerCase()}${id ? ` data-testid=${id}` : ""}> "${name}"`;
    };
    const doc = document.documentElement;
    if (doc.scrollWidth > doc.clientWidth + 1) problems.push(`the page scrolls sideways by ${doc.scrollWidth - doc.clientWidth} px`);
    for (const el of document.querySelectorAll<HTMLElement>("body *")) {
      const style = getComputedStyle(el);
      if ((style.overflowX === "auto" || style.overflowX === "scroll") && el.scrollWidth > el.clientWidth + 1) {
        problems.push(`${describe(el)} scrolls sideways by ${el.scrollWidth - el.clientWidth} px`);
      }
    }
    // Text wider than its box draws over its neighbours (a long path, a word
    // that cannot break). Clipped text (an ellipsis) and scrolling boxes are fine.
    const roots = [document.getElementById("main"), ...document.querySelectorAll('[role="dialog"]')];
    for (const root of roots) {
      if (!root) continue;
      const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
      for (let node = walker.nextNode(); node; node = walker.nextNode()) {
        if (!node.textContent?.trim()) continue;
        const range = document.createRange();
        range.selectNodeContents(node);
        const text = range.getBoundingClientRect();
        if (text.width === 0) continue;
        // Every box up to the first one that clips must hold the text.
        for (let el: HTMLElement | null = node.parentElement; el && el !== root.parentElement; el = el.parentElement) {
          const style = getComputedStyle(el);
          if (style.overflowX !== "visible") break;
          // A box that is not drawn (display: contents) has no edges.
          if (style.display === "contents") continue;
          const outer = el.getBoundingClientRect();
          const over = Math.max(text.right - outer.right, outer.left - text.left);
          if (over > 1) {
            problems.push(`${describe(el)} text spills ${Math.round(over)} px out of its box`);
            break;
          }
        }
      }
    }
    const selector =
      'a[href], button, input, select, textarea, [role="button"], [role="radio"], [role="checkbox"], [role="switch"], [role="tab"], [role="combobox"], [role="menuitem"]';
    const visible = [...document.querySelectorAll<HTMLElement>(selector)].filter((el) => {
      const r = el.getBoundingClientRect();
      const style = getComputedStyle(el);
      return r.width > 0 && r.height > 0 && style.visibility !== "hidden" && !el.closest("[aria-hidden=true], [inert]");
    });
    // Behind an open dialog or menu, only it counts (menus are not modal: the
    // page behind them stays in the accessibility tree).
    const dialog = document.querySelector('[role="dialog"], [role="alertdialog"], [role="menu"]');
    const shown = dialog ? visible.filter((el) => dialog.contains(el)) : visible;
    for (let i = 0; i < shown.length; i++) {
      const a = shown[i]!;
      const ra = a.getBoundingClientRect();
      for (let j = i + 1; j < shown.length; j++) {
        const b = shown[j]!;
        if (a.contains(b) || b.contains(a)) continue;
        const rb = b.getBoundingClientRect();
        const x = Math.min(ra.right, rb.right) - Math.max(ra.left, rb.left);
        const y = Math.min(ra.bottom, rb.bottom) - Math.max(ra.top, rb.top);
        if (x > 1 && y > 1) problems.push(`${describe(a)} overlaps ${describe(b)}`);
      }
    }
    if (dialog) {
      const r = dialog.getBoundingClientRect();
      if (r.top < 0 || r.bottom > window.innerHeight + 1 || r.left < 0 || r.right > window.innerWidth + 1) {
        problems.push(`the dialog ${describe(dialog)} does not fit the window`);
      }
    }
    return problems;
  });
}

for (const simple of [true, false]) {
  const mode = simple ? "simple" : "advanced";
  for (const [index, [width, height]] of sizes.entries()) {
    // Alternating language and theme covers both of each across the sizes.
    const language = index % 2 === 0 ? "en" : "ru";
    const theme = index % 2 === 0 ? "light" : "dark";
    test(`every screen fits at ${width}×${height}, ${mode} mode, ${language} ${theme}`, async ({ page, request }) => {
      const errors: string[] = [];
      page.on("pageerror", (error) => errors.push(error.message));
      page.on("console", (message) => {
        if (message.type() === "error") errors.push(message.text());
      });
      await page.setViewportSize({ width, height });
      await openLibrary(page, request, { language, theme, simple });
      const problems: string[] = [];
      const check = async (screen: string) => {
        for (const problem of await layoutProblems(page)) problems.push(`${screen}: ${problem}`);
      };

      await page.goto("/devices");
      await expect(page.getByTestId("device-card").first()).toBeVisible();
      await check("devices");

      await page.goto("/library");
      await expect(page.getByTestId("library-count")).toBeVisible();
      await expect(page.getByTestId("game-grid").getByRole("listitem").first()).toBeVisible();
      await check("library");
      if (!simple) {
        await page.getByTestId("library-more").click();
        await expect(page.getByRole("menu")).toBeVisible();
        await check("library, More menu");
        await page.keyboard.press("Escape");
        await expect.soft(page.getByTestId("library-more"), "focus returns to the menu button").toBeFocused();
        // A dialog opened from the menu gives focus back to the menu's button.
        await page.getByTestId("library-more").click();
        await page.getByTestId("verify-dumps").click();
        await expect(page.getByRole("dialog")).toBeVisible();
        await page.keyboard.press("Escape");
        await expect(page.getByRole("dialog")).toBeHidden();
        await expect.soft(page.getByTestId("library-more"), "focus returns after a dialog from the menu").toBeFocused();
      }

      await page.getByTestId("game-grid").getByRole("listitem").first().click();
      await expect(page.getByTestId("game-title")).toBeVisible();
      await check("game card");
      const rename = page.getByRole("group", { name: language === "en" ? "Actions" : "Действия" }).getByRole("button").first();
      await rename.click();
      await expect(page.getByRole("dialog")).toBeVisible();
      await check("rename dialog");
      await page.keyboard.press("Escape");
      await expect(page.getByRole("dialog")).toBeHidden();
      await expect.soft(rename, "focus returns to the button that opened the dialog").toBeFocused();

      await page.goto("/library/add");
      await expect(page.getByTestId("drop-zone")).toBeVisible();
      await check("add games");
      await page.getByTestId("browse-server").click();
      await expect(page.getByTestId("folder-browser")).toBeVisible();
      await check("folder browser");
      await page.keyboard.press("Escape");

      await page.goto("/settings");
      await expect(page.getByTestId("about")).toBeVisible();
      await check("settings");

      if (!simple) {
        await page.goto("/jobs");
        await check("jobs");
      }

      expect.soft(problems).toEqual([]);
      expect(errors).toEqual([]);
    });
  }
}

/** WCAG 2.2 A and AA rules axe can check, by screen (UI-3). */
async function axeViolations(page: Page, screen: string): Promise<string[]> {
  // Colour transitions (a button turning primary once its data loads) settle
  // first: half-way colours are not what the user reads.
  await page.waitForTimeout(400);
  const result = await new AxeBuilder({ page }).withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa"]).analyze();
  return result.violations.flatMap((violation) =>
    violation.nodes.map((node) => `${screen}: ${violation.id} (${violation.impact ?? "?"}) at ${node.target.join(" ")}`),
  );
}

for (const simple of [true, false]) {
  for (const theme of ["light", "dark"] as const) {
    test(`no WCAG 2.2 AA violations axe can find, ${simple ? "simple" : "advanced"} mode, ${theme}`, async ({ page, request }) => {
      await page.setViewportSize({ width: 1280, height: 800 });
      await openLibrary(page, request, { language: theme === "light" ? "en" : "ru", theme, simple });
      const found: string[] = [];

      await page.goto("/devices");
      await expect(page.getByTestId("device-card").first()).toBeVisible();
      found.push(...(await axeViolations(page, "devices")));

      await page.goto("/library");
      await expect(page.getByTestId("game-grid").getByRole("listitem").first()).toBeVisible();
      found.push(...(await axeViolations(page, "library")));

      await page.getByTestId("game-grid").getByRole("listitem").first().click();
      await expect(page.getByTestId("game-title")).toBeVisible();
      found.push(...(await axeViolations(page, "game card")));

      await page.goto("/library/add");
      await expect(page.getByTestId("drop-zone")).toBeVisible();
      found.push(...(await axeViolations(page, "add games")));

      await page.goto("/settings");
      await expect(page.getByTestId("about")).toBeVisible();
      found.push(...(await axeViolations(page, "settings")));

      if (!simple) {
        await page.goto("/jobs");
        found.push(...(await axeViolations(page, "jobs")));
      }
      expect(found).toEqual([]);
    });
  }
}
