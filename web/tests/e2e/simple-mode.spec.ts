import { readFileSync } from "node:fs";
import path from "node:path";

import { type APIRequestContext, expect, type Page, test } from "@playwright/test";

import { emulatorFlow } from "./emulator-flow";
import { emptyStick, job, joinPath, openLibrary, roots, rpc, stickAsUsb, stickCard } from "./helpers";

// Simple mode end to end (UI-4, ADR-0034), the app's default. There is no
// jobs page, so every write must say in its plan what it does, keep Cancel in
// reach while it runs and end in a notification. The plan is the same check
// as in advanced mode; only its steps and housekeeping fold away.

const OLD = "v1.2.0-Beta-2271-b3a2199";
const NEW = "v1.2.0-Beta-2280-0123abc";

test("simple mode: a game is added and deleted through plans that name it, and notifications say how it went", async ({
  page,
  request,
}) => {
  await openLibrary(page, request, { language: "en", simple: true });
  await page.goto("/library/add");
  await expect(page.getByRole("link", { name: "Jobs" })).toHaveCount(0);
  // Covers on add depend on the cache other tests leave: verify.spec has them.
  await page.getByTestId("covers-on-add").uncheck();
  await page.getByTestId("browse-server").click();
  const browser = page.getByTestId("folder-browser");
  await browser.getByRole("button", { name: "sources" }).click();
  await browser.getByRole("checkbox", { name: "Select Lf Game.iso" }).check();
  await browser.getByTestId("browser-add").click();
  await page.getByTestId("preview-plan").click();

  const plan = page.getByTestId("import-plan");
  await expect(plan.getByTestId("plan-transaction")).toHaveCount(1);
  await expect(plan).toContainText("1 operation");
  await expect(plan).toContainText("SCES_500.03");
  // The steps of each operation fold away in simple mode.
  await expect(plan.getByTestId("plan-steps")).toHaveCount(0);
  await plan.getByTestId("plan-confirm").click();

  // No jobs page: back to the library, and a notification.
  await expect(page).toHaveURL(/\/library$/);
  await expect(page.getByText("Done: Import 1 game")).toBeVisible();
  const added = page.getByRole("listitem", { name: /Dead or Alive 2/ });
  await expect(added).toBeVisible();
  await expect(page.getByTestId("library-count")).toHaveText("6 of 6 games");

  await added.click();
  await page.getByRole("button", { name: "Delete", exact: true }).click();
  await page.getByTestId("delete-preview").click();
  const removal = page.getByTestId("plan-dialog");
  await expect(removal.getByTestId("plan-transaction")).toHaveCount(1);
  await expect(removal.getByTestId("plan-confirm")).toBeDisabled();
  await removal.getByLabel("I understand that files will be deleted").check();
  await removal.getByTestId("plan-confirm").click();
  await expect(page).toHaveURL(/\/library$/);
  await expect(page.getByText(/^Done: Delete /)).toBeVisible();
  await expect(page.getByTestId("library-count")).toHaveText("5 of 5 games");
});

test("simple mode: a plan of housekeeping alone lists what it does", async ({ page, request }) => {
  await openLibrary(page, request, { language: "en", simple: true });
  await page.goto("/devices");
  // The empty sources folder is read over SMB: OPL's folders come first.
  const sources = page
    .getByTestId("device-card")
    .filter({ has: page.locator("span.font-mono", { hasText: /[\\/]sources$/ }) })
    .first();
  await sources.getByTestId("smb-init-button").click();
  const plan = page.getByTestId("plan-dialog");
  await expect(plan.getByTestId("plan-transaction")).toHaveCount(1);
  await expect(plan.getByTestId("plan-transaction")).toContainText("Create the OPL folders");
  await page.keyboard.press("Escape");
  await expect(plan).toHaveCount(0);
});

test("simple mode: a memory card emulator's SD names its plan, keeps its files and gets no loader", async ({
  page,
  request,
}) => {
  // ADR-0048: the boot cards answer the version, then "Create OPL folders"
  // through a plan that lists its transaction; the emulator's files stay
  // byte for byte.
  await emulatorFlow(page, request, true);
});

/** Build 2271 at FreeMcBoot's path, through the server, as the install screen would. */
async function installOld(request: APIRequestContext, stick: string, mirror: string): Promise<void> {
  const done = await job(request, { type: "DownloadLoader", build: "opl-beta", mirror });
  const { sha256 } = done.result as { sha256: string };
  const plan = await rpc(request, {
    type: "PlanInstallLoader",
    root: stick,
    build: "opl-beta",
    sha256,
    presets: ["fmcb"],
    device: "usb",
  });
  await job(request, { type: "ExecutePlan", plan: plan.id, confirmDelete: false });
}

async function releasesFrom(page: Page, mirror: string): Promise<void> {
  await page.addInitScript((folder) => {
    const settings = JSON.parse(localStorage.getItem("yaoplm.settings") ?? "{}") as Record<string, unknown>;
    localStorage.setItem("yaoplm.settings", JSON.stringify({ ...settings, loaderMirror: folder }));
  }, mirror);
}

test("simple mode: update and rollback plans list the loader they put on the stick", async ({ page, request }) => {
  const stick = await emptyStick(request);
  const { mirror } = await roots(request);
  await installOld(request, stick, joinPath(mirror, "loaders"));
  await openLibrary(page, request, { language: "en", simple: true });
  await releasesFrom(page, joinPath(mirror, "loaders-next"));
  await stickAsUsb(page);
  await page.goto("/devices");
  const row = stickCard(page).getByTestId("installed-loader");
  await row.getByTestId("installed-loader-check").click();

  const dialog = page.getByTestId("loader-update-dialog");
  await dialog.getByTestId("loader-update-start").click();
  const update = page.getByTestId("plan-dialog");
  // The loader is what the plan changes: listed, the manifest folded away.
  await expect(update.getByTestId("plan-transaction").first()).toContainText("Put OPL 1.2.0 beta in UPDT/OPL.ELF");
  await expect(update.getByTestId("plan-transaction").filter({ hasText: "Remember the installed loader" })).toHaveCount(0);
  await update.getByTestId("plan-confirm").click();
  await expect(dialog).toBeHidden();
  await expect(row.getByTestId("installed-loader-version")).toHaveText("v1.2.0-Beta-2280 (0123abc)");
  expect(readFileSync(path.join(stick, "UPDT", "OPL.ELF")).includes(NEW)).toBe(true);

  await row.getByRole("button", { name: "Put back v1.2.0-Beta-2271" }).click();
  const rollback = page.getByTestId("plan-dialog");
  await expect(rollback.getByTestId("plan-transaction")).toHaveCount(1);
  await expect(rollback.getByTestId("plan-transaction")).toContainText("back as UPDT/OPL.ELF");
  await rollback.getByTestId("plan-confirm").click();
  await expect(row.getByTestId("installed-loader-version")).toHaveText("v1.2.0-Beta-2271 (b3a2199)");
  expect(readFileSync(path.join(stick, "UPDT", "OPL.ELF")).includes(OLD)).toBe(true);
  // The card refreshes after the write: a device list still on its way
  // must not outlive the test inside the stand-in's route.
  await page.unrouteAll({ behavior: "ignoreErrors" });
});
