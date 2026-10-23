import { expect, test } from "@playwright/test";

import { openLibrary } from "./helpers";

// Screenshots for PR review (TESTING.md): both themes, both languages, two widths.
// Saved to test-results/screenshots/ and uploaded by CI.

const headings = {
  en: { devices: "Devices", library: "Library", game: "Game", add: "Add games", jobs: "Jobs", settings: "Settings" },
  ru: { devices: "Устройства", library: "Библиотека", game: "Игра", add: "Добавить игры", jobs: "Задания", settings: "Настройки" },
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
        await expect(page.getByRole("status").filter({ hasText: /./ }).first()).toBeVisible();
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

        await page.getByRole("listitem", { name: /Retail Game/ }).click();
        await heading("game");
        await expect(page.getByTestId("game-title")).toBeVisible();
        await shot("game");

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
        await browser.getByTestId("browser-add").click();
        await page.getByTestId("preview-plan").click();
        await expect(page.getByTestId("import-plan").getByTestId("plan-transaction")).toHaveCount(2);
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
