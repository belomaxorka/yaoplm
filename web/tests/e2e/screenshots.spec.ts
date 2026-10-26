import { expect, test } from "@playwright/test";

import { libraryTool, openLibrary } from "./helpers";

// Screenshots for PR review (TESTING.md): both themes, both languages, two widths.
// Saved to test-results/screenshots/ and uploaded by CI.

const headings = {
  en: { devices: "Devices", library: "Library", game: "Game", add: "Add games", jobs: "Jobs", settings: "Settings" },
  ru: { devices: "Носители", library: "Библиотека", game: "Игра", add: "Добавить игры", jobs: "Задания", settings: "Настройки" },
} as const;

for (const language of ["en", "ru"] as const) {
  for (const theme of ["light", "dark"] as const) {
    for (const width of [1280, 800]) {
      test(`screenshots ${language} ${theme} ${width}`, async ({ page, request }) => {
        await page.setViewportSize({ width, height: 800 });
        await openLibrary(page, request, { language, theme });
        const shot = (name: string) =>
          page.screenshot({ path: `test-results/screenshots/${name}-${language}-${theme}-${width}.png` });
        const heading = (name: keyof (typeof headings)["en"]) =>
          expect(page.getByRole("heading", { level: 1, name: headings[language][name] })).toBeVisible();

        await page.goto("/devices");
        await heading("devices");
        await expect(page.getByTestId("device-card").first()).toBeVisible();
        await shot("devices");

        await page.goto("/library");
        await heading("library");
        await expect(page.getByTestId("library-count")).toBeVisible();
        // Covers written by art.spec.ts show in the grid; the images must have loaded.
        await expect(page.locator('[data-testid="game-cover"][data-has-cover="true"] img').first()).toBeVisible();
        await shot("library");

        // The bulk cover dialog with its scope and sources (ART-4).
        await page.getByTestId("download-covers").click();
        await expect(page.getByTestId("art-fetch-dialog")).toBeVisible();
        // Let the dialog's fade-in finish before the shot.
        await page.waitForTimeout(400);
        await shot("art-fetch");
        await page.keyboard.press("Escape");
        await expect(page.getByTestId("art-fetch-dialog")).toBeHidden();

        // The "More" menu keeps each tool on one line.
        await page.getByTestId("library-more").click();
        await expect(page.getByTestId("convert-shown")).toBeVisible();
        await page.waitForTimeout(200);
        await shot("library-more");
        await page.keyboard.press("Escape");
        await expect(page.getByTestId("convert-shown")).toBeHidden();

        // Picture sets with the two fixture sets picked (ART-5).
        await libraryTool(page, "import-pictures");
        const importDialog = page.getByTestId("art-import-dialog");
        await importDialog.getByTestId("art-import-browse").click();
        const setBrowser = page.getByTestId("folder-browser");
        await setBrowser.getByRole("button", { name: "mirror" }).click();
        await setBrowser.getByRole("button", { name: "sets", exact: true }).click();
        await setBrowser.getByRole("checkbox", { name: /OPLM art backup\.zip/ }).check();
        await setBrowser.getByRole("checkbox", { name: /art\.tar/ }).check();
        await setBrowser.getByTestId("browser-add").click();
        await expect(importDialog.getByTestId("art-import-sets").getByRole("listitem")).toHaveCount(2);
        await page.waitForTimeout(400);
        await shot("art-import");
        await page.keyboard.press("Escape");
        await expect(importDialog).toBeHidden();

        await page.getByRole("listitem", { name: /Retail Game/ }).click();
        await heading("game");
        await expect(page.getByTestId("game-title")).toBeVisible();
        await shot("game");
        // Every picture type with its actions (ART-2, ART-3).
        await page.getByTestId("art-slots").scrollIntoViewIfNeeded();
        await expect(page.locator('[data-testid="art-slot-COV"] img')).toBeVisible();
        await shot("art-slots");
        await page.getByTestId("game-title").scrollIntoViewIfNeeded();

        await page.getByRole("button", { name: language === "en" ? "Move to DVD" : "Перенести в DVD" }).click();
        const plan = page.getByTestId("plan-dialog");
        await expect(plan.getByTestId("plan-summary")).toBeVisible();
        await shot("plan");
        await page.keyboard.press("Escape");
        await expect(plan).toBeHidden();

        // A deletion plan: the user's file highlighted and the extra checkbox.
        await page.getByRole("button", { name: language === "en" ? "Delete" : "Удалить", exact: true }).click();
        await page.getByTestId("delete-preview").click();
        await expect(plan.getByTestId("plan-summary")).toBeVisible();
        await shot("delete-plan");
        await page.keyboard.press("Escape");
        await expect(plan).toBeHidden();

        await page.goto("/library/add");
        await heading("add");
        await shot("add");
        // The import plan with its per-game overrides; cancelling discards it.
        await page.getByTestId("browse-server").click();
        const browser = page.getByTestId("folder-browser");
        await browser.getByRole("button", { name: "sources" }).click();
        await browser.getByRole("checkbox", { name: /Lf Game\.iso/ }).check();
        await browser.getByRole("checkbox", { name: /Disc Image\.cue/ }).check();
        await browser.getByRole("checkbox", { name: /New Game\.cue/ }).check();
        await browser.getByTestId("browser-add").click();
        await page.getByTestId("preview-plan").click();
        // The cover search runs under the plan, which comes back with the cover (ART-8).
        await expect(page.getByTestId("cover-search")).toHaveAttribute("data-status", "done");
        await expect(page.getByTestId("import-plan").getByTestId("plan-transaction")).toHaveCount(4);
        await expect(page.getByTestId("plan-confirm")).toBeEnabled();
        await page.getByTestId("import-plan").scrollIntoViewIfNeeded();
        await shot("import-plan");
        await page.getByTestId("import-plan").getByRole("button", { name: language === "en" ? "Cancel" : "Отмена" }).click();

        await page.goto("/jobs");
        await heading("jobs");
        await expect(page.getByTestId("job-card").first()).toBeVisible();
        await shot("jobs");

        await page.goto("/settings");
        await heading("settings");
        await expect(page.getByTestId("app-version")).toHaveText(/^\d+\.\d+\.\d+/);
        await shot("settings");
      });
    }
  }
}

// The same screens in simple mode, the app's default (UI-4).
for (const language of ["en", "ru"] as const) {
  for (const theme of ["light", "dark"] as const) {
    for (const width of [1280, 800]) {
      test(`simple mode screenshots ${language} ${theme} ${width}`, async ({ page, request }) => {
        await page.setViewportSize({ width, height: 800 });
        await openLibrary(page, request, { language, theme, simple: true });
        const shot = (name: string) =>
          page.screenshot({ path: `test-results/screenshots/simple-${name}-${language}-${theme}-${width}.png` });

        await page.goto("/devices");
        await expect(page.getByTestId("device-card").first()).toBeVisible();
        await shot("devices");

        await page.goto("/library");
        await expect(page.locator('[data-testid="game-cover"][data-has-cover="true"] img').first()).toBeVisible();
        await shot("library");

        await page.getByRole("listitem", { name: /Retail Game/ }).click();
        await expect(page.getByTestId("game-title")).toBeVisible();
        await shot("game");

        await page.goto("/library/add");
        await expect(page.getByTestId("drop-zone")).toBeVisible();
        await shot("add");

        await page.goto("/settings");
        await expect(page.getByTestId("simple-mode")).toBeChecked();
        await shot("settings");
      });
    }
  }
}

// A memory card emulator's SD on its device card (ADR-0048), in both modes:
// the fixture root `emulator` with the version question still open.
for (const language of ["en", "ru"] as const) {
  for (const theme of ["light", "dark"] as const) {
    for (const simple of [false, true]) {
      test(`emulator card screenshots ${language} ${theme} ${simple ? "simple" : "advanced"}`, async ({ page, request }) => {
        await page.setViewportSize({ width: 1280, height: 800 });
        await openLibrary(page, request, { language, theme, simple });
        await page.goto("/devices");
        const card = page
          .getByTestId("device-card")
          .filter({ has: page.locator("span.font-mono", { hasText: /[\\/]emulator$/ }) })
          .first();
        await expect(card.getByTestId("emulator-panel")).toBeVisible();
        await card.screenshot({
          path: `test-results/screenshots/emulator-card-${simple ? "simple" : "advanced"}-${language}-${theme}.png`,
        });
      });
    }
  }
}
