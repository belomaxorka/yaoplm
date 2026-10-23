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
