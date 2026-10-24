import { expect, test } from "@playwright/test";

import { joinPath, openLibrary, roots } from "./helpers";

// Covers end to end against the fixture mirrors (ART-1 to ART-4, META-1):
// one game downloads its cover from the game card, the bulk run skips the
// games that have one, a manual replacement comes from a server file, a
// cover is deleted with confirmation, and the About screen credits the
// sources. The tests share one library, so they run in order.
test.describe.configure({ mode: "serial" });

test("the fixture starts with one cover and the database titles", async ({ page, request }) => {
  await openLibrary(page, request);
  await page.goto("/library");
  await expect(page.getByTestId("library-count")).toHaveText("5 of 5 games");
  await expect(page.getByTestId("filter-no-cover")).toHaveText("4 without cover");
  const dual = page.getByRole("listitem", { name: /Dual Layer Quest/ });
  await expect(dual.getByTestId("game-cover")).toHaveAttribute("data-has-cover", "true");
  const retail = page.getByRole("listitem", { name: /Retail Game/ });
  await expect(retail.getByTestId("game-cover")).toHaveAttribute("data-has-cover", "false");

  await page.getByTestId("filter-no-cover").click();
  await expect(page.getByTestId("library-count")).toHaveText("4 of 5 games");
  await expect(page).toHaveURL(/nocover=1/);
  await page.getByTestId("filter-no-cover").click();

  // META-1: the bundled database knows SLUS_200.62.
  await retail.click();
  await expect(page.getByTestId("known-title")).toHaveText("Grand Theft Auto III");
});

test("a cover is downloaded for one game from its card", async ({ page, request }) => {
  await openLibrary(page, request);
  await page.goto("/library");
  await page.getByRole("listitem", { name: /Retail Game/ }).click();
  // Every type has a source now (ADR-0034): the icon slot can download.
  await expect(page.getByTestId("art-download-ICO")).toBeEnabled();
  await page.getByTestId("art-download-COV").click();
  // The sources' pictures to pick from, the first one chosen.
  const variants = page.getByTestId("art-variants-dialog");
  await expect(variants.getByTestId("art-variant")).toHaveCount(1);
  await expect(variants.getByTestId("art-variant")).toHaveAttribute("aria-checked", "true");
  await expect(variants).toContainText("xlenore");
  await variants.getByTestId("art-variant-use").click();
  const plan = page.getByTestId("plan-dialog");
  await expect(plan).toContainText("Write 1 cover");
  await expect(plan).toContainText("ART/SLUS_200.62_COV.png");
  await expect(plan).toContainText("Retail Game: cover");
  await plan.getByTestId("plan-confirm").click();
  await expect(plan).toBeHidden();
  const slot = page.getByTestId("art-slot-COV");
  await expect(slot).toHaveAttribute("data-has-art", "true");
  await expect(slot).toContainText("140×200");
  await expect(page.getByTestId("game-hero").getByTestId("game-cover")).toHaveAttribute("data-has-cover", "true");
  // The image really loads from /media/art.
  const src = await slot.locator("img").getAttribute("src");
  expect(src).toContain("/media/art?path=");
  const image = await request.get(src!);
  expect(image.ok()).toBe(true);
  expect(image.headers()["content-type"]).toBe("image/png");
});

test("the bulk run takes only the games without a cover", async ({ page, request }) => {
  await openLibrary(page, request);
  await page.goto("/library");
  await page.getByTestId("download-covers").click();
  const dialog = page.getByTestId("art-fetch-dialog");
  await expect(dialog).toContainText("Only the 3 games without a cover");
  await dialog.getByTestId("art-fetch-start").click();
  const plan = page.getByTestId("plan-dialog");
  // Split Game (SCES_500.03) comes from the libretro mirror by its Redump name.
  await expect(plan).toContainText("Write 1 cover");
  await expect(plan).toContainText("ART/SCES_500.03_COV.png");
  await expect(plan).toContainText("2 games already have a cover and were left alone");
  // The two games no mirror knows are listed as not found, not as errors.
  await expect(plan.getByTestId("plan-skipped")).toContainText("SLPM_123.45");
  await expect(plan.getByTestId("plan-skipped")).toContainText("SLES_999.99");
  await plan.getByTestId("plan-confirm").click();
  await expect(plan).toBeHidden();
  await expect(page.getByTestId("filter-no-cover")).toHaveText("2 without cover");

  // The jobs page keeps the search with its per-game log.
  await page.goto("/jobs");
  // Cover searches carry a translated title, not the core's English one.
  const job = page.getByTestId("job-card").filter({ hasText: "Cover search" }).first();
  await expect(job).toHaveAttribute("data-status", "finished");
  await job.getByRole("button", { name: /^Log/ }).click();
  await expect(job.getByTestId("job-log")).toContainText("SCES_500.03 Split Game: found at libretro");
  await expect(job.getByTestId("job-log")).toContainText("SLPM_123.45 Uppercase Ext: not found");
});

test("a cover is replaced from a server file and deleted with confirmation", async ({ page, request }) => {
  const { mirror } = await roots(request);
  await openLibrary(page, request);
  await page.goto("/library");
  await page.getByRole("listitem", { name: /Uppercase Ext/ }).click();
  await page.getByTestId("art-replace-COV").click();
  await page.getByTestId("art-from-file-COV").click();
  const browser = page.getByTestId("folder-browser");
  await browser.getByRole("button", { name: "mirror" }).click();
  await browser.getByRole("checkbox", { name: "Select custom-cover.jpg" }).check();
  await browser.getByRole("button", { name: "Use this image" }).click();
  const plan = page.getByTestId("plan-dialog");
  await expect(plan).toContainText("ART/SLPM_123.45_COV.png");
  await expect(plan).toContainText(joinPath(mirror, "custom-cover.jpg"));
  await plan.getByTestId("plan-confirm").click();
  await expect(plan).toBeHidden();
  await expect(page.getByTestId("art-slot-COV")).toHaveAttribute("data-has-art", "true");

  await page.getByTestId("art-delete-COV").click();
  await expect(plan).toContainText("Delete ART/SLPM_123.45_COV.png");
  await expect(plan.getByTestId("plan-confirm")).toBeDisabled();
  await plan.getByLabel("I understand that files will be deleted").check();
  await plan.getByTestId("plan-confirm").click();
  await expect(plan).toBeHidden();
  await expect(page.getByTestId("art-slot-COV")).toHaveAttribute("data-has-art", "false");

  // Any other type the same way: a background from the same file.
  await page.getByTestId("art-replace-BG").click();
  await page.getByTestId("art-from-file-BG").click();
  await browser.getByRole("button", { name: "mirror" }).click();
  await browser.getByRole("checkbox", { name: "Select custom-cover.jpg" }).check();
  await browser.getByRole("button", { name: "Use this image" }).click();
  await expect(plan).toContainText("Write 1 picture");
  await expect(plan).toContainText("ART/SLPM_123.45_BG.png");
  await expect(plan).toContainText("Uppercase Ext: background");
  await plan.getByTestId("plan-confirm").click();
  await expect(plan).toBeHidden();
  const bg = page.getByTestId("art-slot-BG");
  await expect(bg).toHaveAttribute("data-has-art", "true");
  await expect(bg).toContainText("640×480");
  // Deleting it asks again, even right after the cover's deletion.
  await page.getByTestId("art-delete-BG").click();
  await expect(plan).toContainText("Delete from SLPM_123.45: background");
  await expect(plan.getByTestId("plan-confirm")).toBeDisabled();
  await plan.getByLabel("I understand that files will be deleted").check();
  await plan.getByTestId("plan-confirm").click();
  await expect(bg).toHaveAttribute("data-has-art", "false");
});

test("the bulk run takes the picture types it is given", async ({ page, request }) => {
  await openLibrary(page, request);
  await page.goto("/library");
  await page.getByTestId("download-covers").click();
  const dialog = page.getByTestId("art-fetch-dialog");
  // Every type has a source; the front cover is preselected.
  await expect(dialog.getByTestId("art-kinds").getByRole("checkbox")).toHaveCount(8);
  await expect(dialog.getByTestId("art-kind-COV")).toBeChecked();
  await dialog.getByTestId("art-kind-COV").click();
  await dialog.getByTestId("art-kind-LGO").click();
  await expect(dialog).toContainText("Only the missing pictures (5)");
  await dialog.getByTestId("art-fetch-start").click();
  // The fixture's mirrors have no logos: nothing to write, every game listed.
  const plan = page.getByTestId("plan-dialog");
  await expect(plan).toContainText("No picture to write");
  await expect(plan.getByTestId("plan-skipped")).toContainText("SLUS_200.62 Retail Game (LGO)");
  await page.keyboard.press("Escape");
  await page.goto("/jobs");
  // Not covers only: the job is a picture search.
  const job = page.getByTestId("job-card").filter({ hasText: "Picture search" }).first();
  await job.getByRole("button", { name: /^Log/ }).click();
  await expect(job.getByTestId("job-log")).toContainText("Logo");
});

test("picture sets fill the games on the device", async ({ page, request }) => {
  await openLibrary(page, request);
  await page.goto("/library");
  await page.getByTestId("import-pictures").click();
  const dialog = page.getByTestId("art-import-dialog");
  await expect(dialog.getByTestId("art-import-start")).toBeDisabled();
  await dialog.getByTestId("art-import-browse").click();
  const browser = page.getByTestId("folder-browser");
  await browser.getByRole("button", { name: "mirror" }).click();
  await browser.getByRole("button", { name: "sets", exact: true }).click();
  await browser.getByRole("checkbox", { name: "Select OPLM art backup.zip" }).check();
  await browser.getByRole("checkbox", { name: "Select art.tar" }).check();
  await browser.getByRole("button", { name: "Use 2 sets" }).click();
  await expect(dialog.getByTestId("art-import-sets").getByRole("listitem")).toHaveCount(2);
  await dialog.getByTestId("art-import-start").click();

  // Numbered screenshots become SCR; a cover the device has stays; the
  // picture of a game that is not there is left out.
  const plan = page.getByTestId("plan-dialog");
  await expect(plan).toContainText("ART/SLUS_200.62_ICO.png");
  await expect(plan).toContainText("ART/SLUS_200.62_SCR.png");
  await expect(plan).toContainText("ART/SLUS_212.18_COV2.png");
  await expect(plan).not.toContainText("SLUS_212.18_COV.png");
  await expect(plan).toContainText("Kept pictures the device already has: 1");
  await expect(plan).toContainText("Skipped pictures of games not on this device: 1");
  await plan.getByTestId("plan-confirm").click();
  await expect(plan).toBeHidden();

  await page.getByRole("listitem", { name: /Retail Game/ }).click();
  await expect(page.getByTestId("art-slot-ICO")).toHaveAttribute("data-has-art", "true");
  await expect(page.getByTestId("art-slot-SCR")).toHaveAttribute("data-has-art", "true");
  await expect(page.getByTestId("art-slot-SCR2")).toHaveAttribute("data-has-art", "false");
});

test("the About screen credits the data and cover sources", async ({ page, request }) => {
  await openLibrary(page, request);
  await page.goto("/settings");
  const about = page.getByTestId("about");
  await expect(about.getByTestId("source-pcsx2")).toContainText("GPL-3.0");
  await expect(about.getByTestId("source-libretro").first()).toContainText("CC BY-SA 4.0");
  await expect(about.getByTestId("source-xlenore")).toContainText("no licence stated");
  await expect(about).toContainText(/Game IDs: \d{2,}, built 20\d\d-\d\d-\d\d/);
  await expect(about.getByTestId("source-oplm")).toContainText("no licence stated");
  await expect(about.getByTestId("source-screenscraper")).toContainText("CC BY-NC-SA 4.0");
  // The art sources and the cache are configurable; ScreenScraper needs a
  // build with its developer key, which the test server has not.
  await expect(page.getByTestId("art-source-xlenore")).toContainText("xlenore/ps2-covers");
  await expect(page.getByTestId("art-source-oplm")).toContainText("OPL Manager backup");
  await expect(page.getByTestId("screenscraper-unavailable")).toBeVisible();
  await expect(page.getByText(/\d+ objects?, .* in /)).toBeVisible();
});

test("an icon comes from OPL Manager's backup", async ({ page, request }) => {
  await openLibrary(page, request);
  await page.goto("/library");
  await page.getByRole("listitem", { name: /Split Game/ }).click();
  await page.getByTestId("art-download-ICO").click();
  const variants = page.getByTestId("art-variants-dialog");
  await expect(variants.getByTestId("art-variant")).toHaveCount(1);
  await expect(variants).toContainText("oplm");
  await variants.getByTestId("art-variant-use").click();
  const plan = page.getByTestId("plan-dialog");
  await expect(plan).toContainText("ART/SCES_500.03_ICO.png");
  await plan.getByTestId("plan-confirm").click();
  await expect(plan).toBeHidden();
  const slot = page.getByTestId("art-slot-ICO");
  await expect(slot).toHaveAttribute("data-has-art", "true");
  await expect(slot).toContainText("64×64");
});
