import { expect, type Page, test } from "@playwright/test";

import { openLibrary } from "./helpers";

// Dump checks and conversions end to end (GAME-4, GAME-5, META-2): the
// Redump DAT is imported from a server file, every game is checked against
// it, the verdicts reach the library and the game card, and one game goes
// to ZSO and back through plans. Runs after the other files (they expect
// the fixture as it was built) and leaves every game in its format.
test.describe.configure({ mode: "serial" });

const DAT_GAMES = 3;

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
  await page.getByTestId("verify-dumps").click();
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
  await page.getByTestId("convert-shown").click();
  const dialog = page.getByTestId("convert-dialog");
  await dialog.getByTestId("convert-target-zso").click();
  await expect(dialog.getByTestId("convert-count")).toHaveText(/^Change format: \d of 5\.$/);
  await page.keyboard.press("Escape");
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

      await page.getByTestId("verify-dumps").click();
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
