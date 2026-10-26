import { defineConfig, devices } from "@playwright/test";

// The release UI pass (docs/plans/release-ui-consistency.md) against a server
// already running on the dev port with the fixture library:
//   preview `yaoplm-server-fixture`, then
//   pnpm -C web exec playwright test --config tests/release/playwright.config.ts
// Screenshots and the report go to web/test-results/release/.
const baseURL = process.env.YAOPLM_RELEASE_URL ?? "http://127.0.0.1:7878";

export default defineConfig({
  testDir: ".",
  fullyParallel: false,
  workers: 1,
  timeout: 600_000,
  // A step that cannot act (a menu item that closed) fails and is recorded
  // instead of waiting forever.
  expect: { timeout: 8_000 },
  reporter: "list",
  use: {
    baseURL,
    actionTimeout: 10_000,
    navigationTimeout: 20_000,
    locale: "en-US",
    colorScheme: "light",
    trace: "off",
  },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
});
