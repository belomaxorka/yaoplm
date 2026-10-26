import { cpSync, existsSync, readFileSync } from "node:fs";
import path from "node:path";

import { type APIRequestContext, expect, type Page, test } from "@playwright/test";

import { emptyStick, job, joinPath, openLibrary, roots, rpc, stickAsUsb, stickCard } from "./helpers";

// Replacing the loader (LDR-6, LDR-7, ADR-0046) on the fixture's stick:
// "What changes for the games" for both directions between OPL 1.1.0 and
// the beta, and a replacement that re-encodes a JPG cover before it puts
// the beta last. OPL 1.1.0 comes in as the user's own file: the catalogue
// pins the real 1.1.0 archive, which the fixture mirror does not have.

/** Copies the fixture's `extras/<folder>` onto the stick. */
function copyExtras(stick: string, ...folders: string[]) {
  for (const folder of folders) {
    cpSync(path.join(path.dirname(stick), "extras", folder), path.join(stick, folder), { recursive: true });
  }
}

async function install(request: APIRequestContext, stick: string, build: string, sha256: string) {
  const plan = await rpc(request, {
    type: "PlanInstallLoader",
    root: stick,
    build,
    sha256,
    presets: ["fmcb"],
    device: "usb",
  });
  await job(request, { type: "ExecutePlan", plan: plan.id, confirmDelete: false });
}

/** OPL 1.1.0 at FreeMcBoot's path, from the user's own file. */
async function installStable(request: APIRequestContext, stick: string) {
  const { mirror } = await roots(request);
  const done = await job(request, { type: "ImportLoader", file: joinPath(mirror, "own", "OPNPS2LD-v1.1.0.ELF") });
  await install(request, stick, "opl-stable", (done.result as { sha256: string }).sha256);
}

/** The beta at FreeMcBoot's path, from the fixture mirror. */
async function installBeta(request: APIRequestContext, stick: string) {
  const { mirror } = await roots(request);
  const done = await job(request, { type: "DownloadLoader", build: "opl-beta", mirror: joinPath(mirror, "loaders") });
  await install(request, stick, "opl-beta", (done.result as { sha256: string }).sha256);
}

async function openReplace(page: Page) {
  await page.goto("/devices");
  // Replacing waits in the card's menu: "Open library" is its one main button.
  await stickCard(page).getByTestId("device-more").click();
  await page.getByTestId("device-replace-loader").click();
  return page.getByTestId("loader-install-dialog");
}

test("leaving OPL 1.1.0 re-encodes a JPG cover and puts the beta last", async ({ page, request }) => {
  const stick = await emptyStick(request);
  copyExtras(stick, "CD", "ART");
  await installStable(request, stick);
  await openLibrary(page, request, { language: "en" });
  await stickAsUsb(page);
  const dialog = await openReplace(page);
  await expect(dialog.getByRole("heading", { name: "Replace the loader" })).toBeVisible();
  // The installed build is marked; the recommended beta is chosen.
  await expect(dialog.getByTestId("loader-build-installed")).toBeVisible();
  await expect(dialog.getByTestId("loader-build-opl-beta")).toHaveAttribute("data-state", "checked");
  const impact = dialog.getByTestId("loader-impact");
  await expect(impact).toContainText("Re-encode JPG and BMP pictures to PNG: 1");
  await expect(impact.getByTestId("loader-impact-art")).toHaveAttribute("data-state", "checked");

  await dialog.getByTestId("loader-install-start").click();
  const plan = page.getByTestId("plan-dialog");
  await expect(plan.getByRole("heading", { name: "Replace OPL 1.1.0 with OPL 1.2.0 beta" })).toBeVisible();
  await plan.getByTestId("plan-confirm").click();
  await expect(page.getByTestId("loader-boot-hints")).toBeVisible();

  expect(readFileSync(path.join(stick, "ART", "SLUS_212.18_COV.png")).subarray(1, 4).toString("latin1")).toBe("PNG");
  expect(existsSync(path.join(stick, "ART", "SLUS_212.18_COV.jpg"))).toBe(false);
  expect(readFileSync(path.join(stick, "UPDT", "OPL.ELF")).includes("v1.2.0-Beta-2271-b3a2199")).toBe(true);
  await page.getByTestId("loader-install-done").click();
  await expect(stickCard(page).getByTestId("installed-loader-version")).toHaveText("v1.2.0-Beta-2271 (b3a2199)");
});

test("moving to OPL 1.1.0 lists the ZSO games, ticked to convert", async ({ page, request }) => {
  const stick = await emptyStick(request);
  copyExtras(stick, "DVD");
  await installBeta(request, stick);
  await openLibrary(page, request, { language: "ru" });
  await stickAsUsb(page);
  const dialog = await openReplace(page);
  await expect(dialog.getByTestId("loader-build-opl-stable")).toHaveAttribute("data-state", "checked");
  const impact = dialog.getByTestId("loader-impact");
  await expect(impact).toContainText("ZSO-игры, которые новая сборка не покажет: 1");
  await expect(impact.getByTestId("loader-impact-zso")).toHaveAttribute("data-state", "checked");
  // Between the beta and YAOPL nothing changes.
  await dialog.getByTestId("loader-build-yaopl").click();
  await expect(dialog.getByTestId("loader-impact-nothing")).toContainText("Для игр ничего не меняется");
});

// "Restore v1.1.0" crosses builds as a replacement does: the plan says what
// the games lose and points to "Replace…", in both modes (LDR-4, LDR-7).
for (const simple of [false, true]) {
  test(`a rollback to OPL 1.1.0 warns about the ZSO game, ${simple ? "simple" : "advanced"} mode`, async ({ page, request }) => {
    const stick = await emptyStick(request);
    copyExtras(stick, "DVD");
    await installStable(request, stick);
    await installBeta(request, stick);
    await openLibrary(page, request, { language: "en", simple });
    await stickAsUsb(page);
    await page.goto("/devices");
    await stickCard(page).getByTestId("installed-loader-rollback").click();
    const plan = page.getByTestId("plan-dialog");
    await expect(plan).toContainText("ZSO games OPL 1.1.0 cannot show: 1. “Replace…” converts them to ISO in the same plan");
    await expect(plan.getByTestId("plan-confirm")).toBeEnabled();
  });
}

for (const language of ["en", "ru"] as const) {
  for (const theme of ["light", "dark"] as const) {
    test(`loader rollback screenshots ${language} ${theme}`, async ({ page, request }) => {
      const stick = await emptyStick(request);
      copyExtras(stick, "DVD");
      await installStable(request, stick);
      await installBeta(request, stick);
      await page.setViewportSize({ width: 1280, height: 900 });
      await openLibrary(page, request, { language, theme });
      await stickAsUsb(page);
      await page.goto("/devices");
      await stickCard(page).getByTestId("installed-loader-rollback").click();
      await expect(page.getByTestId("plan-dialog")).toContainText(language === "en" ? "ZSO games" : "ZSO-игры");
      await page.waitForTimeout(400);
      await page.screenshot({ path: `test-results/screenshots/loader-rollback-${language}-${theme}-1280.png` });
    });
  }
}

// The replace wizard for PR review (TESTING.md).
for (const language of ["en", "ru"] as const) {
  for (const theme of ["light", "dark"] as const) {
    test(`loader replace screenshots ${language} ${theme}`, async ({ page, request }) => {
      const stick = await emptyStick(request);
      copyExtras(stick, "DVD", "CD", "ART");
      await installBeta(request, stick);
      await page.setViewportSize({ width: 1280, height: 900 });
      await openLibrary(page, request, { language, theme });
      await stickAsUsb(page);
      const shot = async (name: string) => {
        await page.waitForTimeout(400);
        await page.screenshot({ path: `test-results/screenshots/${name}-${language}-${theme}-1280.png` });
      };
      const dialog = await openReplace(page);
      await expect(dialog.getByTestId("loader-impact")).toBeVisible();
      await shot("loader-replace");
      await dialog.getByTestId("loader-impact").scrollIntoViewIfNeeded();
      await shot("loader-replace-impact");
    });
  }
}
