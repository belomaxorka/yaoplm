import { expect, test } from "@playwright/test";

// Screenshots for PR review (TESTING.md): both themes, both languages, two widths.
// Saved to test-results/screenshots/ and uploaded by CI.

const pages = [
  { path: "/", name: "library", heading: { en: "Library", ru: "Библиотека" } },
  { path: "/settings", name: "settings", heading: { en: "Settings", ru: "Настройки" } },
] as const;

for (const language of ["en", "ru"] as const) {
  for (const theme of ["light", "dark"] as const) {
    for (const width of [1280, 800]) {
      test(`screenshots ${language} ${theme} ${width}`, async ({ page }) => {
        await page.setViewportSize({ width, height: 800 });
        await page.addInitScript(
          ([lang, mode]) => {
            localStorage.setItem("yaoplm.language", lang);
            localStorage.setItem("yaoplm.theme", mode);
          },
          [language, theme] as const,
        );

        for (const target of pages) {
          await page.goto(target.path);
          await expect(page.getByRole("heading", { level: 1, name: target.heading[language] })).toBeVisible();
          await expect(page.getByRole("status")).toHaveAttribute("data-state", "open");
          if (target.name === "settings") {
            await expect(page.getByTestId("app-version")).toHaveText(/^\d+\.\d+\.\d+/);
          }
          await page.screenshot({
            path: `test-results/screenshots/${target.name}-${language}-${theme}-${width}.png`,
          });
        }
      });
    }
  }
}
