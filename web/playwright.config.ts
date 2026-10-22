import { defineConfig, devices } from "@playwright/test";

// A port apart from the dev default (7878) so a running server is not reused by accident.
const port = 7879;
const baseURL = `http://127.0.0.1:${port}`;

export default defineConfig({
  testDir: "tests/e2e",
  fullyParallel: true,
  forbidOnly: Boolean(process.env.CI),
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? [["github"], ["html", { open: "never" }]] : "list",
  use: {
    baseURL,
    locale: "en-US",
    colorScheme: "light",
    trace: "retain-on-failure",
  },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
  webServer: {
    // Serves web/dist, so run `pnpm build` first.
    command: `cargo run -q -p yaoplm-server -- --bind 127.0.0.1:${port}`,
    cwd: "..",
    url: `${baseURL}/health`,
    reuseExistingServer: !process.env.CI,
    timeout: 300_000,
  },
});
