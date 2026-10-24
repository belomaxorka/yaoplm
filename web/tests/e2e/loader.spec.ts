import { mkdirSync, rmSync, writeFileSync } from "node:fs";
import path from "node:path";

import { expect, type Page, test } from "@playwright/test";

import { openLibrary, roots } from "./helpers";

// The loader profile per device (LIB-1, ADR-0023). The fixture library has
// no loader and no ZSO, so auto falls back to OPL 1.2.0 beta.

function libraryCard(page: Page) {
  return page
    .getByTestId("device-card")
    .filter({ has: page.locator("span.font-mono", { hasText: /[\\/]library$/ }) })
    .first();
}

test("auto reads the device, an override wins, auto comes back", async ({ page, request }) => {
  await openLibrary(page, request, { language: "en" });
  await page.goto("/devices");
  const line = libraryCard(page).getByTestId("loader-profile");
  await expect(line).toHaveAttribute("data-source", "detected");
  await expect(line.getByTestId("loader-profile-name")).toHaveText("OPL 1.2.0 beta");
  await expect(line.getByTestId("loader-profile-reason")).toHaveText("— no signs of a loader on the device");

  // Choose OPL 1.1.0 for this device only; the other roots keep auto.
  await line.getByTestId("loader-profile-change").click();
  await page.getByTestId("loader-profile-Opl110").click();
  await expect(line).toHaveAttribute("data-source", "override");
  await expect(line.getByTestId("loader-profile-name")).toHaveText("OPL 1.1.0");
  await expect(line.getByTestId("loader-profile-reason")).toHaveText("— chosen for this device");
  await expect(page.locator('[data-testid="loader-profile"][data-source="detected"]')).toHaveCount(2);

  // The library scans with it and says so in its header.
  await page.goto("/library");
  const header = page.getByTestId("loader-profile");
  await expect(header.getByTestId("loader-profile-name")).toHaveText("OPL 1.1.0");
  await expect(page.getByTestId("library-count")).toBeVisible();

  // Back to auto from the library header.
  await header.getByTestId("loader-profile-change").click();
  await page.getByTestId("loader-profile-auto").click();
  await expect(header).toHaveAttribute("data-source", "detected");
  await expect(header.getByTestId("loader-profile-name")).toHaveText("OPL 1.2.0 beta");
});

test("a loader's settings file on the device decides the profile", async ({ page, request }) => {
  const { library } = await roots(request);
  const app = path.join(library, "APPS", "APP_RIPTOPL");
  mkdirSync(app, { recursive: true });
  writeFileSync(path.join(app, "settings_riptopl.cfg"), "enable_usb=1\r\n");
  try {
    await openLibrary(page, request, { language: "ru" });
    await page.goto("/library");
    const header = page.getByTestId("loader-profile");
    await expect(header.getByTestId("loader-profile-name")).toHaveText("RiptOPL");
    await expect(header.getByTestId("loader-profile-reason")).toHaveText(
      "— настройки RiptOPL в APPS/APP_RIPTOPL/settings_riptopl.cfg",
    );
  } finally {
    // Other specs expect a library without a loader.
    rmSync(app, { recursive: true, force: true });
  }
});

test("the settings offer auto first and a profile for every device", async ({ page, request }) => {
  await openLibrary(page, request, { language: "ru" });
  await page.goto("/settings");
  const select = page.getByRole("combobox", { name: "Профиль загрузчика" });
  await expect(select).toHaveText("Авто (по носителю)");
  await select.click();
  await page.getByRole("option", { name: "RiptOPL" }).click();
  await page.goto("/library");
  const header = page.getByTestId("loader-profile");
  await expect(header).toHaveAttribute("data-source", "setting");
  await expect(header.getByTestId("loader-profile-reason")).toHaveText("— задан в настройках");
});

for (const language of ["en", "ru"] as const) {
  for (const theme of ["light", "dark"] as const) {
    test(`loader profile screenshots ${language} ${theme}`, async ({ page, request }) => {
      await page.setViewportSize({ width: 1280, height: 800 });
      await openLibrary(page, request, { language, theme, simple: true });
      await page.goto("/devices");
      await expect(libraryCard(page).getByTestId("loader-profile")).toBeVisible();
      await page.screenshot({ path: `test-results/screenshots/loader-devices-${language}-${theme}.png` });
      await page.goto("/library");
      const header = page.getByTestId("loader-profile");
      await expect(header).toBeVisible();
      await header.getByTestId("loader-profile-change").click();
      await expect(page.getByTestId("loader-profile-Opl110")).toBeVisible();
      await page.waitForTimeout(300);
      await page.screenshot({ path: `test-results/screenshots/loader-library-${language}-${theme}.png` });
    });
  }
}
