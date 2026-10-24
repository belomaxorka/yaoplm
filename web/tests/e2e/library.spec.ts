import { expect, test } from "@playwright/test";

import { openLibrary, roots } from "./helpers";

// The whole U1 scenario against one fixture library (TESTING.md): open a
// device, see its games, add one, confirm the plan, watch the job, find the
// game in the list; then rename and delete it with confirmations.
test.describe.configure({ mode: "serial" });

test("a device is opened from the device list", async ({ page, request }) => {
  const { library } = await roots(request);
  await page.goto("/devices");
  const card = page.getByTestId("device-card").filter({ hasText: library }).first();
  await expect(card).toBeVisible();
  await card.getByTestId("device-open").click();
  await expect(page).toHaveURL(/\/library$/);
  await expect(page.getByTestId("library-count")).toHaveText("5 of 5 games");
  await expect(page.getByRole("listitem", { name: /Retail Game/ })).toBeVisible();
  await expect(page.getByTestId("current-root")).toContainText("library");
});

test("search, filters and the list view", async ({ page, request }) => {
  await openLibrary(page, request);
  await page.goto("/library");
  await expect(page.getByTestId("library-count")).toHaveText("5 of 5 games");

  await page.getByRole("searchbox", { name: "Search" }).fill("dual");
  await expect(page.getByTestId("library-count")).toHaveText("1 of 5 games");
  await expect(page).toHaveURL(/q=dual/);
  await page.getByRole("searchbox", { name: "Search" }).fill("slus20062");
  await expect(page.getByRole("listitem", { name: /Retail Game/ })).toBeVisible();
  await page.getByRole("searchbox", { name: "Search" }).fill("");

  await page.getByRole("radio", { name: "UL" }).click();
  await expect(page.getByTestId("library-count")).toHaveText("1 of 5 games");
  await expect(page.getByRole("listitem", { name: /Split Game/ })).toBeVisible();
  await page.getByRole("radio", { name: "All" }).last().click();

  await page.getByRole("button", { name: /warnings$/ }).click();
  await expect(page.getByTestId("library-count")).toHaveText("3 of 5 games");
  await page.getByRole("button", { name: /warnings$/ }).click();

  await page.getByRole("radio", { name: "List" }).click();
  await expect(page.getByTestId("game-list")).toBeVisible();
  await expect(page.getByTestId("game-list").getByRole("listitem")).toHaveCount(5);
  await page.getByRole("radio", { name: "Grid" }).click();
  await expect(page.getByTestId("game-grid")).toBeVisible();

  await page.getByText("2 files OPL ignores").click();
  await expect(page.getByRole("dialog")).toContainText("No Boot2.iso");
  await page.keyboard.press("Escape");
});

test("the game card shows warnings, CFG and keyboard navigation works", async ({ page, request }) => {
  await openLibrary(page, request);
  await page.goto("/library");
  const first = page.getByTestId("game-grid").getByRole("listitem").first();
  await first.focus();
  await page.keyboard.press("ArrowRight");
  await expect(page.getByTestId("game-grid").getByRole("listitem").nth(1)).toBeFocused();
  await page.keyboard.press("ArrowLeft");
  await expect(first).toBeFocused();

  await page.getByRole("listitem", { name: /Retail Game/ }).click();
  await expect(page.getByTestId("game-title")).toHaveText("Retail Game");
  await expect(page.getByTestId("game-warnings")).toContainText(
    "VMC slot 1: VMC/SLUS_200.62_0.bin is unusable: not a PS2 memory card image (no superblock magic)",
  );
  await expect(page.getByText("Mode 1: accurate reads")).toBeVisible();
  await expect(page.getByText("Mode 3: unhook syscalls")).toBeVisible();
  await expect(page.getByText("SLUS_200.62_0", { exact: true })).toBeVisible();

  await page.goBack();
  await page.getByRole("listitem", { name: /Split Game/ }).click();
  await expect(page.getByTestId("game-parts")).toContainText("ul.D4250D72.SCES_500.03.00");
});

test("a game is added from the server folders through the plan", async ({ page, request }) => {
  await openLibrary(page, request);
  await page.goto("/library/add");
  // Covers on add have their own test (verify.spec): without them this plan
  // does not depend on what the cache holds from other tests.
  await page.getByTestId("covers-on-add").uncheck();
  await expect(page.getByTestId("covers-on-add")).not.toBeChecked();
  await page.getByTestId("browse-server").click();
  const browser = page.getByTestId("folder-browser");
  await browser.getByRole("button", { name: "sources" }).click();
  await browser.getByRole("checkbox", { name: "Select Lf Game.iso" }).check();
  // Files OPL cannot import cannot be picked.
  await expect(browser.getByRole("checkbox", { name: "Select notes.txt" })).toBeDisabled();
  await browser.getByTestId("browser-add").click();
  await expect(page.getByTestId("source-list")).toContainText("Lf Game.iso");

  await page.getByTestId("preview-plan").click();
  const plan = page.getByTestId("import-plan");
  await expect(plan).toBeVisible();
  await expect(plan.getByTestId("plan-transaction")).toHaveCount(1);
  await expect(plan).toContainText("SCES_500.03");
  // The bundled database knows the game: its title replaces the file's.
  await expect(page.getByTestId("titles-from-db")).toBeChecked();
  await expect(plan).toContainText("CD/SCES_500.03.Dead or Alive 2 - Hardcore.iso");
  await expect(plan).toContainText("The bundled database calls SCES_500.03 “Dead or Alive 2 - Hardcore”");

  // A title override re-plans; the file name follows.
  await plan.getByLabel("Title").fill("Lf Renamed");
  await expect(plan).toContainText("CD/SCES_500.03.Lf Renamed.iso");

  await plan.getByTestId("plan-confirm").click();
  await expect(page).toHaveURL(/\/jobs$/);
  const job = page.getByTestId("job-card").filter({ hasText: "Import" }).first();
  await expect(job).toHaveAttribute("data-status", "finished");
  await expect(job.getByTestId("job-outcomes")).toContainText("1 done");
  await job.getByRole("button", { name: /^Log/ }).click();
  await expect(job.getByTestId("job-log")).toContainText("Write CD/SCES_500.03.Lf Renamed.iso: done");

  await page.getByRole("link", { name: "Library", exact: true }).click();
  await expect(page.getByRole("listitem", { name: /Lf Renamed/ })).toBeVisible();
  await expect(page.getByTestId("library-count")).toHaveText("6 of 6 games");
});

test("renaming goes through the plan dialog", async ({ page, request }) => {
  await openLibrary(page, request);
  await page.goto("/library");
  await page.getByRole("listitem", { name: /Lf Renamed/ }).click();
  await page.getByRole("button", { name: "Rename", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "Rename game" });
  await dialog.getByLabel("Title", { exact: true }).fill("Lf Final");
  await dialog.getByRole("button", { name: "Preview changes" }).click();
  const plan = page.getByTestId("plan-dialog");
  await expect(plan).toContainText("CD/SCES_500.03.Lf Final.iso");
  await plan.getByTestId("plan-confirm").click();
  // The card stays open, found by the game's ID now that its file changed.
  await expect(page).toHaveURL(/\/library\/game\/id%3ASCES_500\.03$/);
  await expect(page.getByRole("heading", { name: "Lf Final" })).toBeVisible();
  await page.goto("/library");
  await expect(page.getByRole("listitem", { name: /Lf Final/ })).toBeVisible();
  await expect(page.getByRole("listitem", { name: /Lf Renamed/ })).toHaveCount(0);
});

test("deleting needs the plan and an extra confirmation", async ({ page, request }) => {
  await openLibrary(page, request);
  await page.goto("/library");
  await page.getByRole("listitem", { name: /Lf Final/ }).click();
  await page.getByRole("button", { name: "Delete", exact: true }).click();
  await page.getByTestId("delete-preview").click();
  const plan = page.getByTestId("plan-dialog");
  await expect(plan).toContainText("Delete CD/SCES_500.03.Lf Final.iso");
  await expect(plan.getByTestId("plan-confirm")).toBeDisabled();
  await plan.getByLabel("I understand that files will be deleted").check();
  await plan.getByTestId("plan-confirm").click();
  await expect(page).toHaveURL(/\/library$/);
  await expect(page.getByRole("listitem", { name: /Lf Final/ })).toHaveCount(0);
  await expect(page.getByTestId("library-count")).toHaveText("5 of 5 games");
});

test("the jobs page lists finished jobs and clears them", async ({ page, request }) => {
  await openLibrary(page, request);
  await page.goto("/jobs");
  const cards = page.getByTestId("job-card");
  await expect(cards.first()).toBeVisible();
  const plans = page.locator('[data-testid="job-card"][data-kind="executePlan"]');
  await expect(plans).not.toHaveCount(0);
  // A finished plan shows its outcomes even after a reload (GetJob).
  await expect(plans.first().getByTestId("job-outcomes")).toContainText("done");
  await page.getByRole("button", { name: "Clear finished" }).click();
  await expect(page.getByText("No jobs")).toBeVisible();
});

test("settings change the profile and name style", async ({ page, request }) => {
  await openLibrary(page, request);
  await page.goto("/settings");
  await page.getByRole("combobox", { name: "Loader profile" }).click();
  await page.getByRole("option", { name: "OPL 1.1.0" }).click();
  await page.getByRole("radio", { name: /^Title\.iso/ }).click();
  await page.reload();
  await expect(page.getByRole("combobox", { name: "Loader profile" })).toHaveText("OPL 1.1.0");
  await expect(page.getByRole("radio", { name: /^Title\.iso/ })).toBeChecked();
  // Back to the defaults for the tests that follow.
  await page.getByRole("combobox", { name: "Loader profile" }).click();
  await page.getByRole("option", { name: "OPL 1.2.0 beta" }).click();
  await page.getByRole("radio", { name: /^ID\.Title\.iso/ }).click();
});
