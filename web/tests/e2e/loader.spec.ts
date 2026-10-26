import { mkdirSync, rmSync, writeFileSync } from "node:fs";
import path from "node:path";

import { expect, type Page, test } from "@playwright/test";

import { openLibrary, roots } from "./helpers";

// The loader profile per device (LIB-1, ADR-0023, ADR-0040) and the device's
// mode (ADR-0047). The fixture library has no loader and no ZSO, so auto
// falls back to OPL 1.2.0 beta; it is a server folder read over SMB, so the
// version is asked, and the question stands for the "OPL version" line.

function libraryCard(page: Page) {
  return page
    .getByTestId("device-card")
    .filter({ has: page.locator("span.font-mono", { hasText: /[\\/]library$/ }) })
    .first();
}

test("auto reads the device, an override wins, auto comes back", async ({ page, request }) => {
  await openLibrary(page, request, { language: "en" });
  await page.goto("/devices");
  const card = libraryCard(page);
  // No sign of a loader: asked, and no version line meanwhile.
  await expect(card.getByTestId("loader-question")).toBeVisible();
  await expect(card.getByTestId("loader-profile")).toHaveCount(0);

  // Choose OPL 1.1.0 for this device only, in its one menu; another empty
  // root keeps auto and is still asked.
  const sources = page
    .getByTestId("device-card")
    .filter({ has: page.locator("span.font-mono", { hasText: /[\\/]sources$/ }) })
    .first();
  await card.getByTestId("device-choice-change").click();
  await page.getByTestId("loader-profile-Opl110").click();
  const line = card.getByTestId("loader-profile");
  await expect(line).toHaveAttribute("data-source", "override");
  await expect(line).toContainText("OPL: OPL 1.1.0");
  await expect(line.getByTestId("loader-profile-reason")).toHaveText("— your choice");
  await expect(card.getByTestId("loader-question")).toHaveCount(0);
  await expect(sources.getByTestId("loader-question")).toBeVisible();

  // The library scans with it and says so in its header.
  await page.goto("/library");
  const header = page.getByTestId("loader-profile");
  await expect(header.getByTestId("loader-profile-name")).toHaveText("OPL 1.1.0");
  await expect(page.getByTestId("library-count")).toBeVisible();

  // Back to auto from the library header: the question comes back.
  await page.getByTestId("device-choice-change").click();
  await page.getByTestId("loader-profile-auto").click();
  await expect(page.getByTestId("loader-question")).toBeVisible();
  await expect(page.getByTestId("loader-profile")).toHaveCount(0);
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
    await expect(header).toContainText("OPL: RiptOPL");
    // The fixture is a folder read over SMB: the loader's files there are a
    // copy of what the console runs (ADR-0047).
    await expect(header.getByTestId("loader-profile-reason")).toHaveText(
      "— APPS/APP_RIPTOPL/settings_riptopl.cfg, который пишет только RiptOPL (копия: по SMB консоль запускает OPL с карты памяти или флешки)",
    );
    await expect(page.getByTestId("loader-question")).toHaveCount(0);
  } finally {
    // Other specs expect a library without a loader.
    rmSync(app, { recursive: true, force: true });
  }
});

test("there is no global profile: an old saved one is ignored (ADR-0040)", async ({ page, request }) => {
  // Settings saved before ADR-0040 still hold a profile for every device.
  await page.addInitScript(() => {
    const current: unknown = JSON.parse(localStorage.getItem("yaoplm.settings") ?? "{}");
    const kept = typeof current === "object" && current !== null ? current : {};
    localStorage.setItem("yaoplm.settings", JSON.stringify({ ...kept, loaderProfile: "RiptOpl" }));
  });
  await openLibrary(page, request, { language: "ru" });
  await page.goto("/settings");
  await expect(page.getByText("Имена файлов")).toBeVisible();
  await expect(page.getByRole("combobox", { name: "Профиль загрузчика" })).toHaveCount(0);
  await page.goto("/library");
  // Still read from the device: no signs there, so it is asked.
  await expect(page.getByTestId("loader-question")).toBeVisible();
  await page.getByTestId("device-choice-change").click();
  await expect(page.getByTestId("loader-profile-auto")).toHaveText("По носителю");
  await expect(page.getByTestId("loader-profile-auto")).toHaveAttribute("data-state", "checked");
});

test("a server folder is read over SMB and asked about its OPL (ADR-0047)", async ({ page, request }) => {
  // The answers: LoaderProfileHint.test.tsx.
  await openLibrary(page, request, { language: "en" });
  await page.goto("/devices");
  const card = libraryCard(page);
  const mode = card.getByTestId("device-mode");
  await expect(mode).toHaveAttribute("data-mode", "smb");
  await expect(mode).toHaveAttribute("data-source", "detected");
  await expect(mode.getByTestId("device-mode-reason")).toHaveText("— a folder on the server");
  await expect(card).not.toHaveAttribute("data-verdict", "error");
  await expect(card.getByTestId("loader-question")).toContainText("Which OPL will read this folder?");
  // One "Change" for the connection and the version.
  await expect(card.getByTestId("device-choice-change")).toHaveCount(1);

  // The other roots are empty folders: OPL's folders first, no loader.
  const sources = page
    .getByTestId("device-card")
    .filter({ has: page.locator("span.font-mono", { hasText: /[\\/]sources$/ }) })
    .first();
  await expect(sources.getByTestId("smb-init")).toBeVisible();
  await expect(sources.getByTestId("loader-question")).toBeVisible();
  // The console starts OPL from its memory card or stick: no install here.
  await expect(card.getByTestId("device-install-loader")).toHaveCount(0);
  await expect(sources.getByTestId("device-install-loader")).toHaveCount(0);
  await sources.getByTestId("smb-init-button").click();
  const plan = page.getByTestId("plan-dialog");
  await expect(plan).toContainText("CD");
  await page.keyboard.press("Escape");
  await expect(plan).toHaveCount(0);

  // The library says the same in its header, and an answer replaces the
  // question with the version line.
  await page.goto("/library");
  const header = page.getByTestId("device-mode");
  await expect(header).toHaveAttribute("data-mode", "smb");
  await expect(page.getByTestId("loader-question")).toBeVisible();
  await page.getByTestId("loader-answer-RiptOpl").click();
  await expect(page.getByTestId("loader-profile-name")).toHaveText("RiptOPL");
  await expect(page.getByTestId("loader-question")).toHaveCount(0);
  await expect(page.getByTestId("loader-conflict")).toHaveCount(0);
  await page.getByTestId("device-choice-change").click();
  await page.getByTestId("loader-profile-auto").click();
  await expect(page.getByTestId("loader-question")).toBeVisible();
});

test("the mode is switched per device and goes back to auto (ADR-0047)", async ({ page, request }) => {
  await openLibrary(page, request, { language: "ru" });
  await page.goto("/devices");
  const card = libraryCard(page);
  const mode = card.getByTestId("device-mode");
  await card.getByTestId("device-choice-change").click();
  await page.getByTestId("device-mode-ps2").click();
  await expect(mode).toHaveAttribute("data-mode", "ps2");
  await expect(mode).toHaveAttribute("data-source", "override");
  await expect(mode.getByTestId("device-mode-reason")).toHaveText("— выбрано для этого носителя");
  // Plugged into the console, the BDM rules apply: on a system disk (the
  // Windows temp folder) the doctor says it does not suit OPL, and then
  // nothing is asked; the other roots keep their place's mode.
  const unusable = (await card.getAttribute("data-verdict")) === "error";
  await expect(card.getByTestId("loader-question")).toHaveCount(unusable ? 0 : 1);
  await expect(page.locator('[data-testid="device-mode"][data-mode="smb"]')).toHaveCount(3);

  await page.goto("/library");
  const header = page.getByTestId("device-mode");
  await expect(header).toHaveAttribute("data-mode", "ps2");
  await page.getByTestId("device-choice-change").click();
  await expect(page.getByTestId("device-mode-auto")).toHaveText("Определять по носителю");
  await page.getByTestId("device-mode-auto").click();
  await expect(header).toHaveAttribute("data-mode", "smb");
  await expect(header).toHaveAttribute("data-source", "detected");
});

for (const language of ["en", "ru"] as const) {
  for (const theme of ["light", "dark"] as const) {
    test(`loader profile screenshots ${language} ${theme}`, async ({ page, request }) => {
      await page.setViewportSize({ width: 1280, height: 800 });
      await openLibrary(page, request, { language, theme, simple: true });
      await page.goto("/devices");
      await expect(libraryCard(page).getByTestId("device-mode")).toBeVisible();
      await page.screenshot({ path: `test-results/screenshots/loader-devices-${language}-${theme}.png` });
      // How the PS2 reads the folder and which OPL: one menu (ADR-0047).
      await libraryCard(page).getByTestId("device-choice-change").click();
      await expect(page.getByTestId("device-mode-ps2")).toBeVisible();
      await page.waitForTimeout(300);
      await page.screenshot({ path: `test-results/screenshots/mode-menu-${language}-${theme}.png` });
      await page.keyboard.press("Escape");
      // Answered: the version line instead of the question.
      await page.goto("/library");
      await page.getByTestId("loader-answer-Opl110").click();
      await expect(page.getByTestId("loader-profile")).toBeVisible();
      await page.waitForTimeout(300);
      await page.screenshot({ path: `test-results/screenshots/loader-library-${language}-${theme}.png` });
      await page.getByTestId("device-choice-change").click();
      await page.getByTestId("loader-profile-auto").click();
      // The settings have no profile any more (ADR-0040).
      await page.goto("/settings");
      await expect(page.getByTestId("simple-mode")).toBeVisible();
      await page.screenshot({ path: `test-results/screenshots/loader-settings-${language}-${theme}.png` });
    });
  }
}
