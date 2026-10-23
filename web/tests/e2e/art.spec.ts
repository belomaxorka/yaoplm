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
  await page.getByTestId("cover-download").click();
  const dialog = page.getByTestId("art-fetch-dialog");
  await expect(dialog).toContainText("xlenore/ps2-covers");
  await dialog.getByTestId("art-fetch-start").click();
  // The job ends with a plan; the plan dialog takes over.
  const plan = page.getByTestId("plan-dialog");
  await expect(plan).toContainText("Write 1 cover");
  await expect(plan).toContainText("ART/SLUS_200.62_COV.png");
  await plan.getByTestId("plan-confirm").click();
  await expect(plan).toBeHidden();
  const cover = page.getByTestId("cover-actions").getByTestId("game-cover");
  await expect(cover).toHaveAttribute("data-has-cover", "true");
  await expect(page.getByTestId("cover-actions")).toContainText("140×200");
  // The image really loads from /media/art.
  const src = await cover.locator("img").getAttribute("src");
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
  await expect(plan).toContainText("Games that already have a cover were left alone");
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
  await page.getByTestId("cover-replace").click();
  await page.getByTestId("cover-from-file").click();
  const browser = page.getByTestId("folder-browser");
  await browser.getByRole("button", { name: "mirror" }).click();
  await browser.getByRole("checkbox", { name: "Select custom-cover.jpg" }).check();
  await browser.getByRole("button", { name: "Use this image" }).click();
  const plan = page.getByTestId("plan-dialog");
  await expect(plan).toContainText("ART/SLPM_123.45_COV.png");
  await expect(plan).toContainText(joinPath(mirror, "custom-cover.jpg"));
  await plan.getByTestId("plan-confirm").click();
  await expect(plan).toBeHidden();
  await expect(page.getByTestId("cover-actions").getByTestId("game-cover")).toHaveAttribute("data-has-cover", "true");

  await page.getByTestId("cover-delete").click();
  await expect(plan).toContainText("Delete ART/SLPM_123.45_COV.png");
  await expect(plan.getByTestId("plan-confirm")).toBeDisabled();
  await plan.getByLabel("I understand that files will be deleted").check();
  await plan.getByTestId("plan-confirm").click();
  await expect(plan).toBeHidden();
  await expect(page.getByTestId("cover-actions").getByTestId("game-cover")).toHaveAttribute("data-has-cover", "false");
});

test("the About screen credits the data and cover sources", async ({ page, request }) => {
  await openLibrary(page, request);
  await page.goto("/settings");
  const about = page.getByTestId("about");
  await expect(about.getByTestId("source-pcsx2")).toContainText("GPL-3.0");
  await expect(about.getByTestId("source-libretro").first()).toContainText("CC BY-SA 4.0");
  await expect(about.getByTestId("source-xlenore")).toContainText("no licence stated");
  await expect(about).toContainText(/Game IDs: \d{2,}, built 20\d\d-\d\d-\d\d/);
  // The art sources and the cache are configurable.
  await expect(page.getByTestId("art-source-xlenore")).toContainText("xlenore/ps2-covers");
  await expect(page.getByText(/\d+ objects?, .* in /)).toBeVisible();
});
