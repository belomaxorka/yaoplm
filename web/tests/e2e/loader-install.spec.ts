import { existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import path from "node:path";

import { expect, type Page, test } from "@playwright/test";

import { openLibrary, roots } from "./helpers";

// Installing a loader (LDR-3, BOOT-1, ADR-0044) on the fixture's empty
// stick, from the fixture's release mirror: the wizard, the plan, the
// launcher's lines, and what lands on the device.

function stickCard(page: Page) {
  return page
    .getByTestId("device-card")
    .filter({ has: page.locator("span.font-mono", { hasText: /[\\/]stick$/ }) })
    .first();
}

const isStick = (root: unknown) => typeof root === "string" && /[\\/]stick$/.test(root);

/**
 * The stick is a temp folder on this machine's system disk, where no loader
 * may go (§5.10.1); the device list shows it as the FAT32 USB stick it
 * stands for. The plan and everything after it are the server's own.
 */
async function stickAsUsb(page: Page): Promise<void> {
  await page.route("**/api/rpc", async (route) => {
    const body = route.request().postDataJSON() as { type?: string } | null;
    if (body?.type !== "ListDevices") return route.fallback();
    const response = await route.fetch();
    const json = (await response.json()) as { devices: Record<string, unknown>[] };
    for (const device of json.devices.filter((d) => isStick(d.root))) {
      Object.assign(device, {
        fileSystem: "FAT32",
        fsName: "FAT32",
        kind: "removable",
        bus: "usb",
        external: true,
        system: false,
        readOnly: false,
        model: null,
        serial: null,
        partition: null,
        advice: [],
      });
    }
    await route.fulfill({ response, json });
  });
}

/** Every test starts from an empty stick. */
async function emptyStick(request: Parameters<typeof roots>[0]): Promise<string> {
  const { stick } = await roots(request);
  for (const name of readdirSync(stick)) rmSync(path.join(stick, name), { recursive: true, force: true });
  return stick;
}

test("the recommended build goes onto a fresh stick, and the card names it", async ({ page, request }) => {
  const stick = await emptyStick(request);
  await openLibrary(page, request, { language: "en" });
  await stickAsUsb(page);
  await page.goto("/devices");
  await stickCard(page).getByTestId("device-install-loader").click();

  const dialog = page.getByTestId("loader-install-dialog");
  await expect(dialog.getByTestId("loader-offer")).toContainText("v1.2.0-Beta-2271-b3a2199");
  await expect(dialog.getByTestId("loader-offer")).toContainText("AFL-3.0");
  await expect(dialog.getByTestId("loader-build-opl-beta")).toHaveAttribute("data-state", "checked");
  await expect(dialog.getByTestId("loader-method-fmcb")).toHaveAttribute("data-state", "checked");
  await dialog.getByTestId("loader-install-start").click();

  const plan = page.getByTestId("plan-dialog");
  await expect(plan.getByRole("heading", { name: "Install OPL 1.2.0 beta" })).toBeVisible();
  // Nothing is written before the plan is confirmed.
  expect(existsSync(path.join(stick, "UPDT"))).toBe(false);
  await plan.getByTestId("plan-confirm").click();

  const hints = page.getByTestId("loader-boot-hints");
  await expect(hints.getByTestId("loader-boot-hint")).toHaveAttribute("data-launcher", "fmcb");
  await expect(hints).toContainText("mass:/UPDT/OPL.ELF");
  await expect(hints).toContainText("USB device start mode to Auto");

  expect(readFileSync(path.join(stick, "UPDT", "OPL.ELF")).subarray(0, 4).toString("latin1")).toBe("\x7fELF");
  for (const folder of ["CFG", "THM", "LNG", "ART", "VMC", "CHT", "APPS", "CD", "DVD"]) {
    expect(existsSync(path.join(stick, folder)), folder).toBe(true);
  }
  const manifest = JSON.parse(readFileSync(path.join(stick, ".yaoplm", "manifest.json"), "utf8")) as {
    loaders: { build: string; path: string; version: string; device: string }[];
  };
  expect(manifest.loaders).toEqual([
    expect.objectContaining({ build: "opl-beta", path: "UPDT/OPL.ELF", version: "v1.2.0-Beta-2271-b3a2199", device: "usb" }),
  ]);

  await dialog.getByTestId("loader-install-done").click();
  await expect(dialog).toBeHidden();
  await expect(stickCard(page).getByTestId("loader-profile-reason")).toContainText("UPDT/OPL.ELF");
});

test("a program that is not OPL is replaced only when ticked, and kept as a backup", async ({ page, request }) => {
  const stick = await emptyStick(request);
  mkdirSync(path.join(stick, "APPS"), { recursive: true });
  // wLaunchELF where PS2BBL looks first.
  const foreign = Buffer.concat([Buffer.from("\x7fELF", "latin1"), Buffer.from("wLaunchELF 4.43a")]);
  writeFileSync(path.join(stick, "APPS", "OPNPS2LD.ELF"), foreign);

  await openLibrary(page, request, { language: "ru" });
  await stickAsUsb(page);
  await page.goto("/devices");
  await stickCard(page).getByTestId("device-install-loader").click();
  const dialog = page.getByTestId("loader-install-dialog");
  await dialog.getByTestId("loader-method-ps2bbl").click();
  await expect(dialog.getByTestId("loader-offer")).toBeVisible();
  await dialog.getByTestId("loader-install-start").click();

  const plan = page.getByTestId("plan-dialog");
  await expect(plan).toContainText("APPS/OPNPS2LD.ELF");
  await expect(plan.getByTestId("plan-confirm")).toBeDisabled();
  await plan.getByTestId("loader-replace-foreign").click();
  await expect(plan.getByTestId("plan-confirm")).toBeEnabled();
  await plan.getByTestId("plan-confirm").click();

  const hints = page.getByTestId("loader-boot-hints");
  await expect(hints.getByTestId("loader-boot-hint")).toHaveAttribute("data-launcher", "ps2bbl");
  await expect(hints).toContainText("стандартный конфиг запускает mass:/APPS/OPNPS2LD.ELF");

  const backups = path.join(stick, ".yaoplm", "backup", "loader");
  const stamps = readdirSync(backups);
  expect(stamps).toHaveLength(1);
  expect(readFileSync(path.join(backups, stamps[0] ?? "", "APPS_OPNPS2LD.ELF"))).toEqual(foreign);
  expect(readFileSync(path.join(stick, "APPS", "OPNPS2LD.ELF"))).not.toEqual(foreign);
});

// The wizard, its plan and the launcher's lines for PR review (TESTING.md).
for (const language of ["en", "ru"] as const) {
  for (const theme of ["light", "dark"] as const) {
    test(`loader install screenshots ${language} ${theme}`, async ({ page, request }) => {
      const stick = await emptyStick(request);
      // Something else where PS2BBL looks: the plan stops at it.
      mkdirSync(path.join(stick, "APPS"), { recursive: true });
      writeFileSync(path.join(stick, "APPS", "OPNPS2LD.ELF"), "\x7fELF wLaunchELF");
      await page.setViewportSize({ width: 1280, height: 900 });
      await openLibrary(page, request, { language, theme });
      await stickAsUsb(page);
      const shot = async (name: string) => {
        // Let fades and scrolling settle.
        await page.waitForTimeout(400);
        await page.screenshot({ path: `test-results/screenshots/${name}-${language}-${theme}-1280.png` });
      };
      await page.goto("/devices");
      await stickCard(page).getByTestId("device-install-loader").click();
      const dialog = page.getByTestId("loader-install-dialog");
      await expect(dialog.getByTestId("loader-offer")).toBeVisible();
      await shot("loader-install");

      await dialog.getByTestId("loader-method-unknown").click();
      await dialog.locator("details summary").click();
      await dialog.getByTestId("loader-offer").scrollIntoViewIfNeeded();
      await shot("loader-install-release");

      await dialog.getByTestId("loader-install-start").click();
      const plan = page.getByTestId("plan-dialog");
      await expect(plan.getByTestId("loader-replace-foreign")).toBeVisible();
      await expect(plan.getByTestId("plan-confirm")).toBeDisabled();
      await shot("loader-install-foreign");

      await plan.getByTestId("loader-replace-foreign").click();
      await expect(plan.getByTestId("plan-confirm")).toBeEnabled();
      await shot("loader-install-plan");

      await plan.getByTestId("plan-confirm").click();
      await expect(page.getByTestId("loader-boot-hint")).toHaveCount(2);
      await shot("loader-boot-hints");
    });
  }
}
