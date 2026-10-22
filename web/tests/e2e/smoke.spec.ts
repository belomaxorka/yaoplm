import { expect, test } from "@playwright/test";

test("health endpoint answers", async ({ request }) => {
  const response = await request.get("/health");
  expect(response.ok()).toBe(true);
  expect(await response.json()).toMatchObject({ status: "ok" });
});

test("rpc answers Ping", async ({ request }) => {
  const response = await request.post("/api/rpc", { data: { type: "Ping" } });
  expect(response.ok()).toBe(true);
  expect(await response.json()).toEqual({ type: "Pong" });
});

test("library screen loads and the event stream connects", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByRole("heading", { level: 1, name: "Library" })).toBeVisible();
  await expect(page.getByText("No library open")).toBeVisible();
  await expect(page.getByRole("status")).toHaveText("Connected");
});

test("settings show the core version over HTTP", async ({ page }) => {
  await page.goto("/settings");
  await expect(page.getByTestId("app-version")).toHaveText(/^\d+\.\d+\.\d+/);
  await expect(page.getByText("Server", { exact: true })).toBeVisible();
});

test("deep links are served by the SPA", async ({ page }) => {
  await page.goto("/jobs");
  await expect(page.getByRole("heading", { level: 1, name: "Jobs" })).toBeVisible();
});

test("language and theme persist across reloads", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("radio", { name: "Русский" }).click();
  await expect(page.getByRole("heading", { level: 1, name: "Библиотека" })).toBeVisible();
  await page.getByRole("button", { name: "Включить тёмную тему" }).click();
  await expect(page.locator("html")).toHaveClass(/\bdark\b/);

  await page.reload();
  await expect(page.getByRole("heading", { level: 1, name: "Библиотека" })).toBeVisible();
  await expect(page.locator("html")).toHaveClass(/\bdark\b/);
  await expect(page.locator("html")).toHaveAttribute("lang", "ru");
});
