import { existsSync, readFileSync } from "node:fs";
import path from "node:path";

import { type APIRequestContext, expect, type Page, test } from "@playwright/test";

import { emptyStick, joinPath, openLibrary, roots, stickAsUsb, stickCard } from "./helpers";

// Updates and rollbacks of an installed loader (LDR-4, ADR-0045) on the
// fixture's stick: OPL 1.2.0 beta 2271 from `mirror/loaders`, and 2280
// from `mirror/loaders-next`, which "Check for updates" reads.

const OLD = "v1.2.0-Beta-2271-b3a2199";
const NEW = "v1.2.0-Beta-2280-0123abc";

interface Answer {
  type: string;
  [key: string]: unknown;
}

async function rpc(request: APIRequestContext, body: object): Promise<Answer> {
  const response = await request.post("/api/rpc", { data: body });
  const json = (await response.json()) as Answer;
  if (!response.ok()) throw new Error(`${JSON.stringify(body)}: ${JSON.stringify(json)}`);
  return json;
}

/** Runs a job to its end. */
async function job(request: APIRequestContext, body: object): Promise<Answer> {
  const { job } = await rpc(request, body);
  for (;;) {
    const info = await rpc(request, { type: "GetJob", job });
    if (info.status === "finished") return info;
    if (info.status === "failed" || info.status === "cancelled") throw new Error(JSON.stringify(info));
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
}

/** Build 2271 at FreeMcBoot's path, through the server, as the install screen would. */
async function installOld(request: APIRequestContext, stick: string, mirror: string): Promise<void> {
  const done = await job(request, { type: "DownloadLoader", build: "opl-beta", mirror });
  const { sha256 } = done.result as { sha256: string };
  const plan = await rpc(request, {
    type: "PlanInstallLoader",
    root: stick,
    build: "opl-beta",
    sha256,
    presets: ["fmcb"],
    device: "usb",
  });
  await job(request, { type: "ExecutePlan", plan: plan.id, confirmDelete: false });
}

/** Releases come from `mirror` from now on, as the setting says. */
async function releasesFrom(page: Page, mirror: string): Promise<void> {
  await page.addInitScript((folder) => {
    const settings = JSON.parse(localStorage.getItem("yaoplm.settings") ?? "{}") as Record<string, unknown>;
    localStorage.setItem("yaoplm.settings", JSON.stringify({ ...settings, loaderMirror: folder }));
  }, mirror);
}

async function setup(page: Page, request: APIRequestContext, extra: Parameters<typeof openLibrary>[2]) {
  const stick = await emptyStick(request);
  const { mirror } = await roots(request);
  await installOld(request, stick, joinPath(mirror, "loaders"));
  await openLibrary(page, request, extra);
  await releasesFrom(page, joinPath(mirror, "loaders-next"));
  await stickAsUsb(page);
  return stick;
}

test("Check for updates finds a newer build; it goes on through a plan and rolls back", async ({ page, request }) => {
  const stick = await setup(page, request, { language: "en" });
  await page.goto("/devices");
  const row = stickCard(page).getByTestId("installed-loader");
  await expect(row).toHaveAttribute("data-freshness", "current");
  await expect(row.getByTestId("installed-loader-version")).toHaveText("v1.2.0-Beta-2271 (b3a2199)");

  await row.getByTestId("installed-loader-check").click();
  // The release is read now; the window opens by itself, once.
  const dialog = page.getByTestId("loader-update-dialog");
  await expect(dialog.getByRole("heading", { name: "Update available" })).toBeVisible();
  await expect(dialog.getByTestId("loader-update-versions")).toContainText("v1.2.0-Beta-2280 (0123abc)");
  await expect(dialog.getByRole("link", { name: /What changed since your version/ })).toHaveAttribute(
    "href",
    "https://github.com/ps2homebrew/Open-PS2-Loader/compare/b3a2199...0123abc",
  );
  await dialog.getByTestId("loader-update-start").click();
  const plan = page.getByTestId("plan-dialog");
  await expect(plan.getByRole("heading", { name: `Update OPL 1.2.0 beta to ${NEW}` })).toBeVisible();
  await plan.getByTestId("plan-confirm").click();
  await expect(dialog).toBeHidden();

  await expect(row.getByTestId("installed-loader-version")).toHaveText("v1.2.0-Beta-2280 (0123abc)");
  await expect(row).toHaveAttribute("data-freshness", "current");
  expect(readFileSync(path.join(stick, "UPDT", "OPL.ELF")).includes(NEW)).toBe(true);
  expect(existsSync(path.join(stick, ".yaoplm", "backup", "loader"))).toBe(true);

  await row.getByRole("button", { name: "Put back v1.2.0-Beta-2271" }).click();
  await page.getByTestId("plan-dialog").getByTestId("plan-confirm").click();
  await expect(row.getByTestId("installed-loader-version")).toHaveText("v1.2.0-Beta-2271 (b3a2199)");
  await expect(row).toHaveAttribute("data-freshness", "update");
  expect(readFileSync(path.join(stick, "UPDT", "OPL.ELF")).includes(OLD)).toBe(true);
  // This device and build were offered already.
  await expect(row.getByTestId("installed-loader-update-to")).toBeVisible();
  await expect(dialog).toBeHidden();
});

test("Not now is remembered across visits, and the setting turns the window off", async ({ page, request }) => {
  await setup(page, request, { language: "ru" });
  const { mirror } = await roots(request);
  // The release was read before: the window opens without the network.
  await job(request, { type: "LoaderRelease", build: "opl-beta", mirror: joinPath(mirror, "loaders-next") });
  await page.goto("/devices");
  const dialog = page.getByTestId("loader-update-dialog");
  await expect(dialog.getByRole("heading", { name: "Доступно обновление" })).toBeVisible();
  await dialog.getByTestId("loader-update-later").click();
  await expect(dialog).toBeHidden();
  await page.reload();
  await expect(stickCard(page).getByTestId("installed-loader-update")).toBeVisible();
  await expect(dialog).toBeHidden();

  await page.goto("/settings");
  await expect(page.getByTestId("offer-loader-updates")).toHaveAttribute("data-state", "checked");
});

// The card and the window for PR review (TESTING.md).
for (const language of ["en", "ru"] as const) {
  for (const theme of ["light", "dark"] as const) {
    test(`loader update screenshots ${language} ${theme}`, async ({ page, request }) => {
      await page.setViewportSize({ width: 1280, height: 900 });
      await setup(page, request, { language, theme });
      const { mirror } = await roots(request);
      await job(request, { type: "LoaderRelease", build: "opl-beta", mirror: joinPath(mirror, "loaders-next") });
      const shot = async (name: string) => {
        await page.waitForTimeout(400);
        await page.screenshot({ path: `test-results/screenshots/${name}-${language}-${theme}-1280.png` });
      };
      await page.goto("/devices");
      const dialog = page.getByTestId("loader-update-dialog");
      await expect(dialog.getByTestId("loader-update-versions")).toBeVisible();
      await shot("loader-update-offer");
      await dialog.getByTestId("loader-update-later").click();
      await expect(dialog).toBeHidden();
      const card = stickCard(page);
      await card.scrollIntoViewIfNeeded();
      await shot("loader-update-card");

      await card.getByTestId("installed-loader-update-to").click();
      await dialog.getByTestId("loader-update-start").click();
      await page.getByTestId("plan-dialog").getByTestId("plan-confirm").click();
      await expect(card.getByTestId("installed-loader-current")).toBeVisible();
      // The "done" notice goes by itself; the card's buttons are under it.
      await expect(page.locator("[data-sonner-toast]")).toHaveCount(0, { timeout: 15_000 });
      await card.scrollIntoViewIfNeeded();
      await shot("loader-update-done");
    });
  }
}
