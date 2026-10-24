import { expect, test } from "@playwright/test";

import { openLibrary } from "./helpers";

// A game's settings and memory cards (CFG-2, VMC-1, ADR-0036). The fixture's
// Retail Game has a CFG with modes 1 and 3 and names a zero-filled card in
// slot 1; Dual Layer Quest has no CFG at all.

test("advanced: modes, GSM and a new card in slot 2 through one plan", async ({ page, request }) => {
  const root = await openLibrary(page, request);
  await page.goto("/library");
  await page.getByRole("listitem", { name: /Dual Layer Quest/ }).click();
  const settings = page.getByTestId("game-settings");
  await expect(settings.getByTestId("cfg-modes")).toBeVisible();
  await expect(settings.getByTestId("cfg-save")).toBeDisabled();

  await settings.getByRole("checkbox", { name: /Mode 2/ }).check();
  await settings.getByRole("checkbox", { name: /Mode 5/ }).check();
  const gsm = settings.getByTestId("cfg-gsm");
  await gsm.getByRole("radio", { name: "This game’s own" }).click();
  await gsm.getByRole("switch", { name: "Force a video mode" }).click();
  await gsm.getByRole("combobox", { name: "Video mode" }).click();
  await page.getByRole("option", { name: "HDTV 480p, 60 Hz", exact: true }).click();

  await settings.getByTestId("cfg-vmc-create-1").click();
  const dialog = page.getByRole("dialog", { name: "New memory card" });
  await expect(dialog.getByLabel("Name")).toHaveValue("SLUS_212.18_1");
  await dialog.getByRole("radio", { name: "16 MiB" }).click();
  await dialog.getByTestId("cfg-vmc-new").click();
  await expect(settings.getByTestId("cfg-vmc-1")).toContainText("SLUS_212.18_1 (new, 16 MiB)");

  await settings.getByTestId("cfg-save").click();
  const plan = page.getByTestId("plan-dialog");
  const changes = plan.getByTestId("plan-cfg-changes");
  await expect(changes).toContainText("$Compatibility: (none) → 18");
  await expect(changes).toContainText("$GSMSource: (none) → 1");
  await expect(changes).toContainText("$GSMVMode: (none) → 8");
  await expect(changes).toContainText("$VMC_1: (none) → SLUS_212.18_1");
  await plan.getByTestId("plan-steps").locator("summary").click();
  await expect(plan).toContainText("Format memory card VMC/SLUS_212.18_1.bin");
  await plan.getByTestId("plan-confirm").click();
  await expect(plan).toBeHidden();

  await expect(settings.getByTestId("cfg-vmc-1")).toContainText("SLUS_212.18_1 (16 MiB)");
  await expect(settings.getByTestId("cfg-save")).toBeDisabled();
  await expect(settings.getByTestId("cfg-keys")).toContainText("Keys in the CFG file: 6");
  const listed = await request.post("/api/rpc", { data: { type: "ListVmc", root } });
  const cards = ((await listed.json()) as { cards: { name: string; sizeMb: number | null; saves: number | null }[] })
    .cards;
  expect(cards.find((card) => card.name === "SLUS_212.18_1")).toMatchObject({ sizeMb: 16, saves: 0 });
});

test("simple: the broken card in slot 1 is replaced by a formatted one", async ({ page, request }) => {
  await openLibrary(page, request, { simple: true });
  await page.goto("/library");
  await page.getByRole("listitem", { name: /Retail Game/ }).click();
  await expect(page.getByTestId("game-warnings")).toContainText("VMC slot 1: VMC/SLUS_200.62_0.bin is unusable");
  const settings = page.getByTestId("game-settings");
  await expect(settings.getByRole("checkbox", { name: /Mode 1/ })).toBeChecked();
  await expect(settings.getByTestId("cfg-vmc-1")).toHaveCount(0);
  await expect(settings.getByTestId("cfg-gsm")).toHaveCount(0);
  await expect(settings.getByTestId("cfg-vmc-0")).toContainText("OPL does not take SLUS_200.62_0");

  await settings.getByTestId("cfg-vmc-create-0").click();
  await expect(settings.getByTestId("cfg-vmc-0")).toContainText("SLUS_200.62_0 (a new 8 MiB card in its place)");
  await settings.getByTestId("cfg-save").click();
  const plan = page.getByTestId("plan-dialog");
  await expect(plan).toContainText("VMC/SLUS_200.62_0.bin is replaced by a blank card");
  await expect(plan.getByTestId("plan-confirm")).toBeEnabled();
  await plan.getByTestId("plan-confirm").click();
  await expect(plan).toBeHidden();

  await expect(settings.getByTestId("cfg-vmc-0")).toContainText("SLUS_200.62_0 (8 MiB)");
  await expect(page.getByTestId("game-warnings")).toHaveCount(0);
});
