import { readdirSync, readFileSync, rmSync, statSync } from "node:fs";
import path from "node:path";

import { type APIRequestContext, expect, type Page } from "@playwright/test";

import { openLibrary, roots } from "./helpers";

// A memory card emulator's SD (DEV-2, ADR-0048). The fixture root `emulator`
// has the factory card's layout: boot card 1 holds FreeMcBoot's folders and
// an OPL build with MMCE, boot card 2 PS2BBL's. The emulator's files make it
// the emulator's SD wherever it lies, its card shows what each boot card
// holds, it gets no loader, and the MMCE build answers which OPL reads it,
// so OPL's folders come at once.

export function emulatorCard(page: Page) {
  return page
    .getByTestId("device-card")
    .filter({ has: page.locator("span.font-mono", { hasText: /[\\/]emulator$/ }) })
    .first();
}

/** Every file under `root`, relative, with its bytes. */
function snapshot(root: string): Map<string, Buffer> {
  const files = new Map<string, Buffer>();
  const walk = (dir: string) => {
    for (const name of readdirSync(dir)) {
      const full = path.join(dir, name);
      if (statSync(full).isDirectory()) walk(full);
      else files.set(path.relative(root, full).split(path.sep).join("/"), readFileSync(full));
    }
  };
  walk(root);
  return files;
}

/** Back to the fixture: OPL's folders and YAOPLM's own go, the SD's files stay. */
function reset(root: string) {
  for (const name of readdirSync(root)) {
    if (name !== ".sd2psx" && name !== "MemoryCards") rmSync(path.join(root, name), { recursive: true, force: true });
  }
}

/** The whole flow on the emulator's SD, in either mode of the interface. */
export async function emulatorFlow(page: Page, request: APIRequestContext, simple: boolean): Promise<void> {
  const { emulator } = await roots(request);
  reset(emulator);
  const before = snapshot(emulator);
  await openLibrary(page, request, { language: "en", simple });
  await page.goto("/devices");
  const card = emulatorCard(page);
  const mode = card.getByTestId("device-mode");
  await expect(mode).toHaveAttribute("data-mode", "emulator");
  await expect(mode).toHaveAttribute("data-source", "detected");
  await expect(mode.getByTestId("device-mode-reason")).toHaveText(
    "— the files of a memory card emulator: .sd2psx, MemoryCards",
  );
  const panel = card.getByTestId("emulator-panel");
  await expect(panel).toContainText("Memory card emulator SD");
  await expect(panel).toContainText("left alone");
  await expect(panel.getByTestId("emulator-boot-channel").first()).toContainText("FMCB 1.966");
  await expect(panel.getByTestId("emulator-settings")).toHaveCount(simple ? 0 : 1);
  await expect(card.getByTestId("device-install-loader")).toHaveCount(0);
  await expect(card.getByTestId("device-replace-loader")).toHaveCount(0);
  // The factory card makes 64 MiB cards: a note for the open device in the
  // advanced mode (the owner cannot change it in YAOPLM; release pass 1.8, 5.5).
  await expect(card).not.toContainText("The emulator makes 64 MiB cards");
  await card.getByTestId("device-open").click();
  await page.goto("/devices");
  await expect(card.getByText("Open", { exact: true })).toBeVisible();
  if (simple) await expect(card).not.toContainText("The emulator makes 64 MiB cards");
  else await expect(card).toContainText("The emulator makes 64 MiB cards");

  // What the boot cards hold, read from their own file systems: in both
  // modes the launcher, in advanced mode the OPL builds and MMCE.
  const first = panel.getByTestId("emulator-boot-channel").first();
  await expect(first).toContainText("FreeMcBoot");
  await expect(first.getByTestId("emulator-boot-loader")).toHaveCount(simple ? 0 : 1);
  if (!simple) await expect(first.getByTestId("emulator-boot-loader")).toContainText("v1.2.0-Beta-2216-bb415d2");
  // The MMCE build inside boot card 1 answers which OPL reads the SD, so
  // nothing is asked and OPL's folders come at once.
  await expect(card.getByTestId("loader-question")).toHaveCount(0);
  await expect(card.getByTestId("loader-profile")).toContainText(
    "OPL 1.2.0 beta — v1.2.0-Beta-2216-bb415d2 with MMCE in APPS/OPL-MMCE-BETA2.ELF inside boot card 1 (PS2)",
  );

  // OPL's folders through a plan that names them, in either mode.
  await card.getByTestId("emulator-init").getByTestId("smb-init-button").click();
  const plan = page.getByTestId("plan-dialog");
  await expect(plan.getByTestId("plan-transaction")).toHaveCount(1);
  await expect(plan.getByTestId("plan-transaction")).toContainText("Create the OPL folders");
  await plan.getByTestId("plan-confirm").click();
  await expect(plan).toHaveCount(0);
  await expect(card.getByTestId("emulator-init")).toHaveCount(0, { timeout: 15_000 });
  await expect.poll(() => readdirSync(emulator).includes("DVD")).toBe(true);
  // Not a byte of the emulator's files changed.
  const after = snapshot(emulator);
  for (const [file, bytes] of before) expect(after.get(file)?.equals(bytes), file).toBe(true);
  reset(emulator);
}
