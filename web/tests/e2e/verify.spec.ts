import { expect, type Page, test } from "@playwright/test";

import { libraryTool, openLibrary } from "./helpers";

// Dump checks and conversions end to end (GAME-4, GAME-5, META-2): the
// Redump DAT is imported from a server file, every game is checked against
// it, the verdicts reach the library and the game card, and one game goes
// to ZSO and back through plans. Runs after the other files (they expect
// the fixture as it was built) and leaves every game in its format.
test.describe.configure({ mode: "serial" });

const DAT_GAMES = 4;

async function importDat(page: Page) {
  const panel = page.getByTestId("redump-panel").first();
  await panel.getByTestId("redump-import").click();
  const browser = page.getByTestId("folder-browser");
  await browser.getByRole("button", { name: "mirror" }).click();
  await browser.getByRole("button", { name: "redump" }).click();
  await browser.getByRole("checkbox", { name: /Datfile/ }).check();
  await browser.getByTestId("browser-add").click();
}

test("the Redump DAT is imported from a server file", async ({ page, request }) => {
  await openLibrary(page, request);
  await page.goto("/settings");
  const panel = page.getByTestId("redump-panel");
  await expect(panel.getByTestId("redump-none")).toHaveText("No Redump DAT on this computer yet.");
  await importDat(page);
  await expect(panel.getByTestId("redump-message")).toHaveText(`Imported, entries: ${DAT_GAMES}.`);
  await expect(panel.getByTestId("redump-state")).toContainText(`entries: ${DAT_GAMES}`);
  // Imported, never asked: Redump may still be asked.
  await expect(panel.getByTestId("redump-update")).toBeEnabled();
});

test("every game is checked and the verdicts reach the library", async ({ page, request }) => {
  await openLibrary(page, request, { view: "list" });
  await page.goto("/library");
  await libraryTool(page, "verify-dumps");
  const dialog = page.getByTestId("verify-dialog");
  await expect(dialog.getByTestId("redump-state")).toContainText(`entries: ${DAT_GAMES}`);
  await dialog.getByTestId("verify-scope-all").click();
  await dialog.getByTestId("verify-start").click();
  const results = dialog.getByTestId("verify-results");
  // Wrong Name holds Retail Game's bytes under another ID: verified, with a note.
  await expect(results.getByTestId("verify-count-verified")).toHaveText("Verified: 3");
  await expect(results.getByTestId("verify-count-badDump")).toHaveText("Bad dumps: 1");
  await expect(results.getByTestId("verify-count-unknown")).toHaveText("Unknown: 1");
  await expect(results.getByTestId("verify-count-failed")).toHaveCount(0);
  await expect(results).toContainText("the game ID differs from Redump’s");
  await expect(results.getByTestId("verify-row").first()).toHaveAttribute("data-status", "badDump");
  await expect(results).toContainText("Retail Game (USA)");
  await dialog.getByTestId("verify-close").click();

  // The library is scanned again once the check is over.
  const retail = page.getByRole("listitem", { name: /Retail Game/ });
  await expect(retail.getByTestId("dump-mark")).toHaveAttribute("data-status", "verified");
  const split = page.getByRole("listitem", { name: /Split Game/ });
  await expect(split.getByTestId("dump-mark")).toHaveAttribute("data-status", "badDump");
});

test("the game card shows the verdict and checks one game from the cache", async ({ page, request }) => {
  await openLibrary(page, request);
  await page.goto("/library");
  await page.getByRole("listitem", { name: /Split Game/ }).click();
  await expect(page.getByTestId("game-dump").getByTestId("dump-badge")).toHaveAttribute("data-status", "badDump");
  await page.getByTestId("game-verify").click();
  const dialog = page.getByTestId("verify-dialog");
  await dialog.getByTestId("verify-start").click();
  const row = dialog.getByTestId("verify-row");
  await expect(row).toHaveAttribute("data-status", "badDump");
  await expect(row).toContainText("Hashes from the cache");
  await expect(row).toContainText("Redump entries with this game ID:");
  await row.getByRole("button", { name: "Hashes" }).click();
  await expect(row).toContainText("SHA-1");
});

async function convert(page: Page, title: RegExp, target: "iso" | "zso", step: string) {
  await page.goto("/library");
  await page.getByRole("listitem", { name: title }).click();
  await page.getByTestId("game-convert").click();
  const dialog = page.getByTestId("convert-dialog");
  await dialog.getByTestId(`convert-target-${target}`).click();
  await dialog.getByTestId("convert-preview").click();
  const plan = page.getByTestId("plan-dialog");
  await expect(plan).toContainText(step);
  // The old image goes: the plan needs the extra confirmation.
  await expect(plan.getByTestId("plan-confirm")).toBeDisabled();
  await plan.getByRole("checkbox", { name: /deleted/ }).check();
  await plan.getByTestId("plan-confirm").click();
  await expect(page).toHaveURL(/\/library$/);
}

test("a game goes to ZSO and back through plans", async ({ page, request }) => {
  await openLibrary(page, request, { view: "list" });
  await convert(page, /Retail Game/, "zso", "Compress into CD/SLUS_200.62.Retail Game.zso");
  const row = page.getByRole("listitem", { name: /Retail Game/ });
  await expect(row).toContainText("ZSO");
  await row.click();
  // The image is new, but its hashes are those of the ISO: no cached verdict yet.
  await expect(page.getByTestId("game-dump")).toHaveText("Not checked yet");

  await convert(page, /Retail Game/, "iso", "Write CD/SLUS_200.62.Retail Game.iso");
  await expect(page.getByRole("listitem", { name: /Retail Game/ })).toContainText("ISO");
});

test("the add page offers ZSO and the bulk dialog counts the games", async ({ page, request }) => {
  await openLibrary(page, request);
  await page.goto("/library/add");
  await expect(page.getByTestId("compress-zso")).toBeEnabled();
  await page.goto("/library");
  await libraryTool(page, "convert-shown");
  const dialog = page.getByTestId("convert-dialog");
  await dialog.getByTestId("convert-target-zso").click();
  await expect(dialog.getByTestId("convert-count")).toHaveText(/^Change format: \d of 5\.$/);
  await page.keyboard.press("Escape");
});

test("games are added with their covers and checked as they are written", async ({ page, request }) => {
  await openLibrary(page, request);
  await page.goto("/library/add");
  // Both are on by default (ART-8, GAME-5).
  await expect(page.getByTestId("covers-on-add")).toBeChecked();
  await expect(page.getByTestId("check-on-add")).toBeChecked();
  await page.getByTestId("browse-server").click();
  const browser = page.getByTestId("folder-browser");
  await browser.getByRole("button", { name: "sources" }).click();
  await browser.getByRole("checkbox", { name: "Select New Game.cue" }).check();
  await browser.getByRole("checkbox", { name: "Select Disc Image.cue" }).check();
  await browser.getByTestId("browser-add").click();
  await page.getByTestId("preview-plan").click();

  const plan = page.getByTestId("import-plan");
  const search = plan.getByTestId("cover-search");
  await expect(search).toHaveAttribute("data-status", "done");
  await expect(search.getByTestId("cover-search-result")).toHaveText(
    // All eight types are looked for (OPL Manager's backup has every type,
    // ADR-0035); the mirrors have one of them for these two games.
    "Pictures found: 1, not found: 13, on the device already: 2.",
  );
  await expect(search.getByTestId("job-log")).toContainText("SLUS_299.99 New Game: found at xlenore");
  // A long plan scrolls inside the page, never the window as well (the
  // second scrollbar of the 2026-10-24 walkthrough).
  expect(await page.evaluate(() => document.scrollingElement!.scrollHeight <= window.innerHeight)).toBe(true);
  // The plan comes back with the cover right after its game.
  const steps = plan.getByTestId("plan-transaction");
  await expect(steps).toHaveCount(3);
  const texts = await steps.allInnerTexts();
  const game = texts.findIndex((t) => t.includes("CD/SLUS_299.99.New Game.iso"));
  expect(game).toBeGreaterThanOrEqual(0);
  expect(texts[game + 1]).toContain("ART/SLUS_299.99_COV.png");

  await plan.getByTestId("plan-confirm").click();
  await expect(page).toHaveURL(/\/jobs$/);
  const job = page.getByTestId("job-card").filter({ hasText: "Import 2 games" }).first();
  await expect(job).toHaveAttribute("data-status", "finished");
  const checks = job.getByTestId("job-checks");
  await expect(checks).toContainText("Verified: 2");
  await checks.getByRole("button", { name: /Dump checks/ }).click();
  const results = checks.getByTestId("verify-results");
  // The CD is matched through the data track of its BIN, the DVD by its ISO.
  await expect(results).toContainText("New Game (USA)");
  await expect(results).toContainText("Dual Layer Quest (USA)");
  await expect(results.getByText("Checked by the data track of the BIN or CHD the game was added from.")).toHaveCount(1);

  // The library has the cover, and the verdict comes from the cache.
  await page.goto("/library");
  await page.getByRole("listitem", { name: /New Game/ }).click();
  await expect(page.getByTestId("art-slot-COV")).toHaveAttribute("data-has-art", "true");
  await expect(page.getByTestId("game-dump").getByTestId("dump-badge")).toHaveAttribute("data-status", "verified");
});

// Screenshots for the PR (TESTING.md), saved with the others.
for (const language of ["en", "ru"] as const) {
  for (const theme of ["light", "dark"] as const) {
    test(`dump screenshots ${language} ${theme}`, async ({ page, request }) => {
      await page.setViewportSize({ width: 1280, height: 800 });
      await openLibrary(page, request, { language, theme });
      const shot = (name: string) =>
        page.screenshot({ path: `test-results/screenshots/${name}-${language}-${theme}-1280.png` });

      await page.goto("/library");
      await expect(page.getByTestId("dump-mark").first()).toBeVisible();
      await shot("library-dumps");

      await libraryTool(page, "verify-dumps");
      const dialog = page.getByTestId("verify-dialog");
      await dialog.getByTestId("verify-scope-all").click();
      await shot("verify-form");
      await dialog.getByTestId("verify-start").click();
      await expect(dialog.getByTestId("verify-results")).toBeVisible();
      await page.waitForTimeout(400);
      await shot("verify-results");
      await dialog.getByTestId("verify-close").click();

      await page.getByRole("listitem", { name: /Retail Game/ }).click();
      await expect(page.getByTestId("game-dump")).toBeVisible();
      await shot("game-dump");
      await page.getByTestId("game-convert").click();
      await expect(page.getByTestId("convert-dialog")).toBeVisible();
      await page.waitForTimeout(400);
      await shot("convert");
      await page.keyboard.press("Escape");

      await page.goto("/settings");
      await expect(page.getByTestId("redump-state")).toBeVisible();
      await page.getByTestId("redump-panel").scrollIntoViewIfNeeded();
      await expect(page.getByTestId("redump-update")).toBeEnabled();
      await page.waitForTimeout(400);
      await shot("settings-redump");
    });
  }
}
