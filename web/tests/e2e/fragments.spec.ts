import { expect, type Page, test } from "@playwright/test";

import { openLibrary } from "./helpers";

// Fragment counts (IMP-7b, LIB-4). The fixture library lives in a temp
// folder, not on FAT32 or exFAT, so the server counts nothing there; the
// scan result gets the counts of a fragmented stick injected instead. The
// counting itself is tested in Rust (yaoplm-core tests/fragments.rs).

interface WireGame {
  title: string;
  warnings: { code: string; message: string; phrase?: unknown }[];
  fragments: unknown;
}

/** Makes the first game without warnings lie in 70 pieces, or as `pieces` says; returns its title. */
async function fragmentedStick(page: Page, pieces?: unknown): Promise<() => string> {
  let title = "";
  await page.route("**/api/rpc", async (route) => {
    const request = route.request().postDataJSON() as { type?: string };
    const response = await route.fetch();
    if (request.type !== "GetJob") return route.fulfill({ response });
    const body = (await response.json()) as { result?: { type: string; games: WireGame[] } | null };
    if (body.result?.type === "Library") {
      const games = body.result.games;
      const target = games.find((g) => g.title === title) ?? games.find((g) => g.warnings.length === 0);
      for (const game of games) game.fragments = { kind: "known", total: 1, files: [1], limit: 64 };
      if (target && pieces) {
        title = target.title;
        target.fragments = pieces;
      } else if (target) {
        title = target.title;
        target.fragments = { kind: "known", total: 70, files: [70], limit: 64 };
        target.warnings.push({
          code: "gameFragmented",
          message: "The game lies on the device in 70 pieces",
          phrase: { key: "notices.gameFragmented", params: { fragments: 70, limit: 64 } },
        });
      }
    }
    return route.fulfill({ response, json: body });
  });
  return () => title;
}

async function openGame(page: Page, title: string): Promise<void> {
  await page.getByRole("searchbox", { name: /Search|Поиск/ }).fill(title);
  await page.getByRole("listitem").filter({ hasText: title }).first().click();
  await expect(page.getByTestId("game-title")).toHaveText(title);
}

test("a fragmented game: warnings filter, card and pieces", async ({ page, request }) => {
  const title = await fragmentedStick(page);
  await openLibrary(page, request, { language: "en" });
  await page.goto("/library");
  await expect(page.getByTestId("library-count")).toBeVisible();
  expect(title()).not.toBe("");

  // The warnings filter keeps the game (LIB-4).
  await page.getByTitle("Only games with warnings").click();
  await page.getByRole("searchbox", { name: "Search" }).fill(title());
  await expect(page.getByRole("listitem").filter({ hasText: title() })).toHaveCount(1);
  await page.getByRole("searchbox", { name: "Search" }).fill("");
  await page.getByTitle("Only games with warnings").click();

  await openGame(page, title());
  await expect(page.getByTestId("game-warnings")).toContainText("is in 70 pieces on the device, more than the 64");
  await expect(page.getByTestId("game-fragments")).toHaveText("70 (OPL loads a game from up to 64)");
});

test("simple mode keeps the warning and hides the count", async ({ page, request }) => {
  const title = await fragmentedStick(page);
  await openLibrary(page, request, { language: "ru", simple: true });
  await page.goto("/library");
  await expect(page.getByTestId("library-count")).toBeVisible();
  await openGame(page, title());
  await expect(page.getByTestId("game-warnings")).toContainText("Игра лежит на носителе кусками (70) и не запустится");
  await expect(page.getByTestId("game-fragments")).toHaveCount(0);
});

test("OPL 1.1.0 says whether every file is one piece", async ({ page, request }) => {
  let title = await fragmentedStick(page, { kind: "known", total: 5, files: [1, 1, 1, 1, 1], limit: null });
  await openLibrary(page, request, { language: "en" });
  await page.goto("/library");
  await expect(page.getByTestId("library-count")).toBeVisible();
  await openGame(page, title());
  await expect(page.getByTestId("game-fragments")).toHaveText("5: each file in one piece, as OPL 1.1.0 needs");

  await page.unroute("**/api/rpc");
  title = await fragmentedStick(page, { kind: "known", total: 3, files: [2, 1], limit: null });
  await page.goto("/library");
  await expect(page.getByTestId("library-count")).toBeVisible();
  await openGame(page, title());
  await expect(page.getByTestId("game-fragments")).toHaveText(
    "3: not every file is in one piece, and OPL 1.1.0 needs each to be",
  );
});

for (const language of ["en", "ru"] as const) {
  for (const theme of ["light", "dark"] as const) {
    test(`fragments screenshot ${language} ${theme}`, async ({ page, request }) => {
      await page.setViewportSize({ width: 1280, height: 900 });
      const title = await fragmentedStick(page);
      await openLibrary(page, request, { language, theme });
      await page.goto("/library");
      await expect(page.getByTestId("library-count")).toBeVisible();
      await openGame(page, title());
      await page.getByTestId("game-fragments").scrollIntoViewIfNeeded();
      await page.screenshot({ path: `test-results/screenshots/fragments-${language}-${theme}.png`, fullPage: true });
    });
  }
}
