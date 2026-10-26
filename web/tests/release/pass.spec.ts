import AxeBuilder from "@axe-core/playwright";
import { expect, type Locator, type Page, test } from "@playwright/test";
import { cpSync, mkdirSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import path from "node:path";

import { emptyStick, job, joinPath, openLibrary, roots, rpc, stickAsUsb, stickCard } from "../e2e/helpers";

// The release UI pass (docs/plans/release-ui-consistency.md): every screen
// and state in both interface modes, both languages and both themes at
// 1024 px, with a screenshot, axe (WCAG 2.2 AA) and the layout checks of
// walkthrough.spec.ts for each. The report lists what axe and the layout
// checks found, state by state, and the Tab order of the device and
// library screens. Runs against the fixture server (config next to it).

const OUT = path.resolve("test-results/release");
const WIDTH = 1024;
const HEIGHT = 900;

interface Run {
  language: "en" | "ru";
  theme: "light" | "dark";
  simple: boolean;
}

const runs: Run[] = [];
for (const simple of [true, false]) {
  for (const language of ["en", "ru"] as const) {
    for (const theme of ["light", "dark"] as const) runs.push({ language, theme, simple });
  }
}

interface StateReport {
  state: string;
  axe: string[];
  layout: string[];
  error?: string;
  tabs?: string[];
}

/** The layout checks of walkthrough.spec.ts. */
async function layoutProblems(page: Page): Promise<string[]> {
  await page.evaluate(() => new Promise((resolve) => setTimeout(resolve, 250)));
  return page.evaluate(() => {
    const problems: string[] = [];
    const describe = (el: Element) => {
      const name = el.getAttribute("aria-label") ?? el.textContent?.trim().slice(0, 40) ?? "";
      const id = el.getAttribute("data-testid");
      return `<${el.tagName.toLowerCase()}${id ? ` data-testid=${id}` : ""}> "${name}"`;
    };
    const doc = document.documentElement;
    if (doc.scrollWidth > doc.clientWidth + 1) problems.push(`the page scrolls sideways by ${doc.scrollWidth - doc.clientWidth} px`);
    for (const el of document.querySelectorAll<HTMLElement>("body *")) {
      const style = getComputedStyle(el);
      if ((style.overflowX === "auto" || style.overflowX === "scroll") && el.scrollWidth > el.clientWidth + 1) {
        problems.push(`${describe(el)} scrolls sideways by ${el.scrollWidth - el.clientWidth} px`);
      }
    }
    const roots = [document.getElementById("main"), ...document.querySelectorAll('[role="dialog"]')];
    for (const root of roots) {
      if (!root) continue;
      const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
      for (let node = walker.nextNode(); node; node = walker.nextNode()) {
        if (!node.textContent?.trim()) continue;
        const range = document.createRange();
        range.selectNodeContents(node);
        const text = range.getBoundingClientRect();
        if (text.width === 0) continue;
        for (let el: HTMLElement | null = node.parentElement; el && el !== root.parentElement; el = el.parentElement) {
          const style = getComputedStyle(el);
          if (style.overflowX !== "visible") break;
          if (style.display === "contents") continue;
          const outer = el.getBoundingClientRect();
          const over = Math.max(text.right - outer.right, outer.left - text.left);
          if (over > 1) {
            problems.push(`${describe(el)} text spills ${Math.round(over)} px out of its box`);
            break;
          }
        }
      }
    }
    const selector =
      'a[href], button, input, select, textarea, [role="button"], [role="radio"], [role="checkbox"], [role="switch"], [role="tab"], [role="combobox"], [role="menuitem"]';
    const visible = [...document.querySelectorAll<HTMLElement>(selector)].filter((el) => {
      const r = el.getBoundingClientRect();
      const style = getComputedStyle(el);
      return r.width > 0 && r.height > 0 && style.visibility !== "hidden" && !el.closest("[aria-hidden=true], [inert]");
    });
    // Behind an open dialog or menu, only it counts (menus are not modal: the
    // page behind them stays in the accessibility tree).
    const dialog = document.querySelector('[role="dialog"], [role="alertdialog"], [role="menu"]');
    const shown = dialog ? visible.filter((el) => dialog.contains(el)) : visible;
    for (let i = 0; i < shown.length; i++) {
      const a = shown[i]!;
      const ra = a.getBoundingClientRect();
      for (let j = i + 1; j < shown.length; j++) {
        const b = shown[j]!;
        if (a.contains(b) || b.contains(a)) continue;
        const rb = b.getBoundingClientRect();
        const x = Math.min(ra.right, rb.right) - Math.max(ra.left, rb.left);
        const y = Math.min(ra.bottom, rb.bottom) - Math.max(ra.top, rb.top);
        if (x > 1 && y > 1) problems.push(`${describe(a)} overlaps ${describe(b)}`);
      }
    }
    if (dialog) {
      const r = dialog.getBoundingClientRect();
      if (r.top < 0 || r.bottom > window.innerHeight + 1 || r.left < 0 || r.right > window.innerWidth + 1) {
        problems.push(`the dialog ${describe(dialog)} does not fit the window`);
      }
    }
    return problems;
  });
}

async function axeViolations(page: Page): Promise<string[]> {
  const result = await new AxeBuilder({ page }).withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa"]).analyze();
  return result.violations.flatMap((violation) =>
    violation.nodes.map((node) => `${violation.id} (${violation.impact ?? "?"}) at ${node.target.join(" ")}`),
  );
}

/** The focus order under Tab, as the names of what gets focus. */
async function tabOrder(page: Page, presses: number): Promise<string[]> {
  const names: string[] = [];
  await page.locator("body").click({ position: { x: 1, y: 1 } });
  for (let i = 0; i < presses; i++) {
    await page.keyboard.press("Tab");
    names.push(
      await page.evaluate(() => {
        const el = document.activeElement;
        if (!el || el === document.body) return "(body)";
        const name = el.getAttribute("aria-label") ?? (el as HTMLElement).innerText?.trim().split("\n")[0] ?? "";
        return `<${el.tagName.toLowerCase()}${el.getAttribute("role") ? ` role=${el.getAttribute("role")}` : ""}> ${name.slice(0, 50)}`;
      }),
    );
  }
  return names;
}

class Pass {
  readonly report: StateReport[] = [];
  readonly page: Page;
  readonly run: Run;
  constructor(page: Page, run: Run) {
    this.page = page;
    this.run = run;
  }

  get suffix(): string {
    return `${this.run.language}-${this.run.theme}-${this.run.simple ? "simple" : "advanced"}`;
  }

  /** A screenshot tall enough for everything the screen or dialog scrolls. */
  async shot(name: string): Promise<void> {
    await this.page.waitForTimeout(400);
    const extra = await this.page.evaluate(() => {
      let extra = 0;
      for (const el of document.querySelectorAll<HTMLElement>("body *")) {
        const style = getComputedStyle(el);
        if (style.overflowY === "auto" || style.overflowY === "scroll") extra = Math.max(extra, el.scrollHeight - el.clientHeight);
      }
      return extra;
    });
    const height = Math.min(HEIGHT + extra, 3000);
    if (height !== HEIGHT) {
      await this.page.setViewportSize({ width: WIDTH, height });
      await this.page.waitForTimeout(250);
    }
    await this.page.screenshot({ path: path.join(OUT, `${name}--${this.suffix}.png`) });
    if (height !== HEIGHT) await this.page.setViewportSize({ width: WIDTH, height: HEIGHT });
  }

  /** One state: screenshot, axe and layout; a failure is recorded, not fatal. */
  async state(name: string, body: () => Promise<void>, extra: { tabs?: number } = {}): Promise<void> {
    const entry: StateReport = { state: name, axe: [], layout: [] };
    this.report.push(entry);
    try {
      await body();
      await this.shot(name);
      entry.layout = await layoutProblems(this.page);
      entry.axe = await axeViolations(this.page);
      if (extra.tabs) entry.tabs = await tabOrder(this.page, extra.tabs);
    } catch (error) {
      entry.error = error instanceof Error ? error.message.split("\n").slice(0, 3).join(" ") : String(error);
      await this.page.screenshot({ path: path.join(OUT, `${name}--${this.suffix}--error.png`) }).catch(() => {});
    }
  }

  /** The device's one menu, opened unless it is open already (a click would close it). */
  async openMenu(card: Locator): Promise<void> {
    // A menu left open (or closing after a viewport change) would swallow the click.
    await this.page.keyboard.press("Escape");
    await expect(this.page.getByRole("menu")).toHaveCount(0);
    await card.getByTestId("device-choice-change").click();
    await expect(this.page.getByRole("menu")).toBeVisible();
  }

  async escape(): Promise<void> {
    await this.page.keyboard.press("Escape");
    await expect(this.page.getByRole("dialog")).toHaveCount(0, { timeout: 5000 }).catch(() => {});
  }

  /**
   * `localStorage` settings merged on this and every later load (after
   * `openLibrary`'s own init script, which would put its values back), then
   * a reload so the providers read them.
   */
  async settings(patch: Record<string, unknown>): Promise<void> {
    await this.page.addInitScript((p) => {
      const current = JSON.parse(localStorage.getItem("yaoplm.settings") ?? "{}") as Record<string, unknown>;
      localStorage.setItem("yaoplm.settings", JSON.stringify({ ...current, ...p }));
    }, patch);
    await this.page.reload();
  }
}

const words = {
  en: { actions: "Actions", rename: "Rename", delete: "Delete", moveToDvd: "Move to DVD", deleteConfirm: "I understand that files will be deleted", jobs: "Jobs", logs: /^Log/, mode2: /Mode 2/ },
  ru: { actions: "Действия", rename: "Переименовать", delete: "Удалить", moveToDvd: "Перенести в DVD", deleteConfirm: "Я понимаю, что файлы будут удалены", jobs: "Задания", logs: /^Журнал/, mode2: /Режим 2/ },
} as const;

/** Copies the fixture's `extras/<folder>` onto the stick. */
function copyExtras(stick: string, ...folders: string[]) {
  for (const folder of folders) {
    cpSync(path.join(path.dirname(stick), "extras", folder), path.join(stick, folder), { recursive: true });
  }
}

function emulatorCard(page: Page): Locator {
  return page
    .getByTestId("device-card")
    .filter({ has: page.locator("span.font-mono", { hasText: /[\\/]emulator$/ }) })
    .first();
}

function resetEmulator(root: string) {
  for (const name of readdirSync(root)) {
    if (name !== ".sd2psx" && name !== "MemoryCards") rmSync(path.join(root, name), { recursive: true, force: true });
  }
}

mkdirSync(OUT, { recursive: true });

for (const run of runs) {
  const label = `${run.language} ${run.theme} ${run.simple ? "simple" : "advanced"}`;
  test(`release pass: ${label}`, async ({ page, request }) => {
    const w = words[run.language];
    const pass = new Pass(page, run);
    const fixture = await roots(request);
    const stick = await emptyStick(request);
    resetEmulator(fixture.emulator);
    await page.setViewportSize({ width: WIDTH, height: HEIGHT });
    await openLibrary(page, request, run);
    const consoleErrors: string[] = [];
    page.on("pageerror", (error) => consoleErrors.push(error.message));
    page.on("console", (message) => {
      if (message.type() === "error") consoleErrors.push(message.text());
    });

    // ---- Devices: folders read over SMB and the emulator's SD ----
    await pass.state(
      "devices",
      async () => {
        await page.goto("/devices");
        await expect(page.getByTestId("device-card").first()).toBeVisible();
        await expect(emulatorCard(page).getByTestId("emulator-panel")).toBeVisible();
      },
      { tabs: 40 },
    );
    await pass.state("devices-stick-answered", async () => {
      await stickCard(page).getByTestId("loader-answer-Opl110").click();
      await expect(stickCard(page).getByTestId("loader-profile")).toBeVisible();
    });
    await pass.state("devices-choice-menu", async () => {
      await pass.openMenu(stickCard(page));
    });
    await pass.state("devices-stick-back-to-auto", async () => {
      await pass.openMenu(stickCard(page));
      await page.getByTestId("loader-profile-auto").click();
      await expect(stickCard(page).getByTestId("loader-question")).toBeVisible();
    });
    await pass.state("devices-stick-as-ps2-by-hand", async () => {
      await pass.openMenu(stickCard(page));
      await page.getByTestId("device-mode-ps2").click();
      await expect(stickCard(page).getByTestId("device-mode")).toHaveAttribute("data-mode", "ps2");
      await page.waitForTimeout(800);
    });
    await pass.state("devices-stick-mode-auto", async () => {
      await pass.openMenu(stickCard(page));
      await page.getByTestId("device-mode-auto").click();
      await expect(stickCard(page).getByTestId("device-mode")).toHaveAttribute("data-source", "detected");
    });
    await pass.escape();
    await pass.state("devices-smb-init-plan", async () => {
      await expect(page.getByRole("menu")).toHaveCount(0);
      await stickCard(page).getByTestId("smb-init-button").click();
      await expect(page.getByTestId("plan-dialog").getByTestId("plan-transaction")).toHaveCount(1);
    });
    await pass.escape();
    await pass.state("devices-emulator-init-plan", async () => {
      await emulatorCard(page).getByTestId("emulator-init").getByTestId("smb-init-button").click();
      await expect(page.getByTestId("plan-dialog").getByTestId("plan-transaction")).toHaveCount(1);
    });
    await pass.escape();

    // ---- Devices: the stick as a FAT32 USB stick plugged into the console ----
    await stickAsUsb(page);
    await pass.state("stick-empty", async () => {
      await page.goto("/devices");
      await expect(stickCard(page).getByTestId("device-install-loader")).toBeVisible();
    });
    const dialog = page.getByTestId("loader-install-dialog");
    await pass.state("install-1-wizard", async () => {
      await stickCard(page).getByTestId("device-install-loader").click();
      await expect(dialog.getByTestId("loader-offer")).toBeVisible();
    });
    await pass.state("install-2-unknown-method-details", async () => {
      await dialog.getByTestId("loader-method-unknown").click();
      await dialog.locator("details summary").first().click();
      await expect(dialog.getByTestId("loader-offer")).toBeVisible();
    });
    if (!run.simple) await pass.state("install-3-other-version", async () => {
      await dialog.getByTestId("loader-other-version").click();
      await expect(dialog.getByTestId("loader-versions")).toBeVisible();
    });
    await pass.state("install-4-stable-build", async () => {
      await dialog.getByTestId("loader-build-opl-stable").click();
      await expect(dialog.getByTestId("loader-offer")).toBeVisible();
      await page.waitForTimeout(500);
    });
    await pass.state("install-5-plan", async () => {
      await dialog.getByTestId("loader-build-opl-beta").click();
      await dialog.getByTestId("loader-method-fmcb").click();
      await expect(dialog.getByTestId("loader-offer")).toBeVisible();
      await dialog.getByTestId("loader-install-start").click();
      await expect(page.getByTestId("plan-dialog").getByTestId("plan-confirm")).toBeEnabled();
    });
    await pass.state("install-6-boot-hints", async () => {
      await page.getByTestId("plan-dialog").getByTestId("plan-confirm").click();
      await expect(page.getByTestId("loader-boot-hints")).toBeVisible({ timeout: 30_000 });
    });
    await pass.state("stick-installed-current", async () => {
      await dialog.getByTestId("loader-install-done").click();
      await expect(dialog).toBeHidden();
      await expect(stickCard(page).getByTestId("installed-loader")).toHaveAttribute("data-freshness", "current");
    });
    await pass.state("reinstall-wizard", async () => {
      await stickCard(page).getByTestId("device-more").click();
      await page.getByTestId("device-install-loader").click();
      await expect(dialog.getByTestId("loader-offer")).toBeVisible();
    });
    await pass.escape();
    // A newer build is published: "Check for updates" reads it.
    await pass.settings({ loaderMirror: joinPath(fixture.mirror, "loaders-next") });
    await stickAsUsb(page);
    await page.goto("/devices");
    const row = stickCard(page).getByTestId("installed-loader");
    const update = page.getByTestId("loader-update-dialog");
    await pass.state("update-1-offer", async () => {
      await row.getByTestId("installed-loader-check").click();
      await expect(update.getByTestId("loader-update-versions")).toBeVisible({ timeout: 20_000 });
    });
    await pass.state("update-2-not-now", async () => {
      await update.getByTestId("loader-update-later").click();
      await expect(update).toBeHidden();
      await expect(row.getByTestId("installed-loader-update-to")).toBeVisible();
    });
    await pass.state("update-3-plan", async () => {
      await row.getByTestId("installed-loader-update-to").click();
      await update.getByTestId("loader-update-start").click();
      await expect(page.getByTestId("plan-dialog").getByTestId("plan-confirm")).toBeEnabled();
    });
    await pass.state("update-4-done", async () => {
      await page.getByTestId("plan-dialog").getByTestId("plan-confirm").click();
      await expect(update).toBeHidden({ timeout: 30_000 });
      await expect(row.getByTestId("installed-loader-version")).toContainText("2280");
    });
    await pass.state("rollback-plan", async () => {
      await row.getByTestId("installed-loader-rollback").click();
      await expect(page.getByTestId("plan-dialog").getByTestId("plan-confirm")).toBeEnabled();
    });
    await pass.state("rollback-done-update-available", async () => {
      await page.getByTestId("plan-dialog").getByTestId("plan-confirm").click();
      await expect(row.getByTestId("installed-loader-version")).toContainText("2271", { timeout: 30_000 });
      await expect(row).toHaveAttribute("data-freshness", "update");
    });
    await pass.state("replace-1-wizard", async () => {
      copyExtras(stick, "DVD", "CD", "ART");
      await stickCard(page).getByTestId("device-more").click();
      await page.getByTestId("device-replace-loader").click();
      await expect(dialog.getByTestId("loader-impact")).toBeVisible({ timeout: 20_000 });
    });
    await pass.state("replace-2-to-stable-impact", async () => {
      await dialog.getByTestId("loader-build-opl-stable").click();
      await expect(dialog.getByTestId("loader-impact-zso")).toBeVisible({ timeout: 20_000 });
    });
    await pass.escape();
    // Rollback across builds: OPL 1.1.0 was here before the beta.
    await pass.state("rollback-across-builds-plan", async () => {
      const own = await job(request, { type: "ImportLoader", file: joinPath(fixture.mirror, "own", "OPNPS2LD-v1.1.0.ELF") });
      const plan = await rpc(request, {
        type: "PlanInstallLoader",
        root: stick,
        build: "opl-stable",
        sha256: (own.result as { sha256: string }).sha256,
        presets: ["fmcb"],
        device: "usb",
      });
      await job(request, { type: "ExecutePlan", plan: plan.id, confirmDelete: false });
      const beta = await job(request, { type: "DownloadLoader", build: "opl-beta", mirror: joinPath(fixture.mirror, "loaders") });
      const plan2 = await rpc(request, {
        type: "PlanInstallLoader",
        root: stick,
        build: "opl-beta",
        sha256: (beta.result as { sha256: string }).sha256,
        presets: ["fmcb"],
        device: "usb",
      });
      await job(request, { type: "ExecutePlan", plan: plan2.id, confirmDelete: false });
      await page.goto("/devices");
      await stickCard(page).getByTestId("installed-loader-rollback").click();
      await expect(page.getByTestId("plan-dialog").getByTestId("plan-confirm")).toBeVisible({ timeout: 20_000 });
    });
    await pass.escape();
    // OPL 1.1.0 on an exFAT stick: the conflict on the card and in the wizard.
    await pass.state("stick-exfat-with-opl110", async () => {
      const stable = await job(request, { type: "ImportLoader", file: joinPath(fixture.mirror, "own", "OPNPS2LD-v1.1.0.ELF") });
      const plan = await rpc(request, {
        type: "PlanInstallLoader",
        root: stick,
        build: "opl-stable",
        sha256: (stable.result as { sha256: string }).sha256,
        presets: ["fmcb"],
        device: "usb",
      });
      await job(request, { type: "ExecutePlan", plan: plan.id, confirmDelete: false });
      await stickAsUsb(page, { fileSystem: "exFAT" });
      await page.goto("/devices");
      await expect(stickCard(page).getByTestId("installed-loader")).toBeVisible();
      await page.waitForTimeout(800);
    });
    await pass.state("install-exfat-wizard", async () => {
      await stickCard(page).getByTestId("device-more").click();
      await page.getByTestId("device-install-loader").click();
      await expect(dialog.getByTestId("loader-build-opl-stable")).toBeDisabled();
    });
    await pass.escape();
    await pass.state("replace-exfat-wizard", async () => {
      await stickCard(page).getByTestId("device-more").click();
      await page.getByTestId("device-replace-loader").click();
      await expect(dialog.getByTestId("loader-offer")).toBeVisible({ timeout: 20_000 });
    });
    await pass.escape();
    await page.unrouteAll({ behavior: "ignoreErrors" });

    // ---- Library ----
    await pass.state(
      "library",
      async () => {
        await page.goto("/library");
        await expect(page.getByTestId("game-grid").getByRole("listitem").first()).toBeVisible();
      },
      { tabs: 30 },
    );
    await pass.state("library-search-nomatch", async () => {
      await page.getByRole("searchbox").fill("zzz");
      await expect(page.getByTestId("game-grid").getByRole("listitem")).toHaveCount(0);
    });
    await page.getByRole("searchbox").fill("");
    if (!run.simple) {
      await pass.state("library-list-view", async () => {
        await page.getByTestId("library-count").waitFor();
        await page.getByRole("radio", { name: run.language === "en" ? "List" : "Список" }).click();
        await expect(page.getByTestId("game-list")).toBeVisible();
      });
      await page.getByRole("radio", { name: run.language === "en" ? "Grid" : "Сетка" }).click();
      await pass.state("library-more-menu", async () => {
        await page.getByTestId("library-more").click();
        await expect(page.getByRole("menu")).toBeVisible();
      });
      await pass.escape();
      await pass.state("library-skipped-files", async () => {
        await page.getByRole("button", { name: /files OPL will not show|OPL не покажет/ }).click();
        await expect(page.getByRole("dialog")).toBeVisible();
      });
      await pass.escape();
      await pass.state("art-import", async () => {
        await page.getByTestId("library-more").click();
        await page.getByTestId("import-pictures").click();
        await expect(page.getByTestId("art-import-dialog")).toBeVisible();
      });
      await pass.state("art-import-browser", async () => {
        await page.getByTestId("art-import-dialog").getByTestId("art-import-browse").click();
        await expect(page.getByTestId("folder-browser")).toBeVisible();
      });
      await pass.escape();
      await pass.escape();
      await pass.state("verify-dialog", async () => {
        await page.getByTestId("library-more").click();
        await page.getByTestId("verify-dumps").click();
        await expect(page.getByTestId("verify-dialog")).toBeVisible();
      });
      await pass.escape();
      await pass.state("convert-dialog", async () => {
        await page.getByTestId("library-more").click();
        await page.getByTestId("convert-shown").click();
        await expect(page.getByTestId("convert-dialog")).toBeVisible();
      });
      await pass.state("convert-dialog-ul", async () => {
        await page.getByTestId("convert-dialog").getByTestId("convert-target-ul").click();
        await page.waitForTimeout(300);
      });
      await pass.escape();
    }
    await pass.state("art-fetch", async () => {
      await page.getByTestId("download-covers").click();
      await expect(page.getByTestId("art-fetch-dialog")).toBeVisible();
      await page.waitForTimeout(1500);
    });
    await pass.escape();
    await pass.state("library-empty-folder", async () => {
      await emptyStick(request);
      await page.goto("/devices");
      await stickCard(page).getByTestId("device-open").click();
      await expect(page).toHaveURL(/\/library$/);
      await page.waitForTimeout(1500);
    });
    await pass.state("library-stale-device", async () => {
      await pass.settings({ root: "Z:\\gone" });
      await page.goto("/library");
      await page.waitForTimeout(1500);
    });
    await pass.settings({ root: fixture.library });

    // ---- Game card ----
    await pass.state("game", async () => {
      await page.goto("/library");
      await page.getByRole("listitem", { name: /Retail Game/ }).click();
      await expect(page.getByTestId("game-title")).toBeVisible();
      await expect(page.getByTestId("game-settings")).toBeVisible();
      await page.waitForTimeout(800);
    });
    await pass.state("game-rename", async () => {
      await page.getByRole("group", { name: w.actions }).getByRole("button", { name: w.rename, exact: true }).click();
      await expect(page.getByRole("dialog")).toBeVisible();
    });
    await pass.escape();
    await pass.state("game-delete", async () => {
      await page.getByRole("button", { name: w.delete, exact: true }).click();
      await expect(page.getByRole("dialog")).toBeVisible();
    });
    await pass.state("game-delete-plan", async () => {
      await page.getByTestId("delete-preview").click();
      await expect(page.getByTestId("plan-dialog").getByTestId("plan-confirm")).toBeDisabled();
    });
    await pass.escape();
    await pass.escape();
    await pass.state("game-move-plan", async () => {
      await page.getByTestId("game-more").click();
      await page.getByTestId("game-move").click();
      await expect(page.getByTestId("plan-dialog").getByTestId("plan-confirm")).toBeVisible();
    });
    await pass.escape();
    if (!run.simple) await pass.state("game-convert", async () => {
      await page.getByTestId("game-more").click();
      await page.getByTestId("game-convert").click();
      await expect(page.getByTestId("convert-dialog")).toBeVisible();
    });
    await pass.escape();
    if (!run.simple) await pass.state("game-verify", async () => {
      await page.getByTestId("game-verify").click();
      await expect(page.getByTestId("verify-dialog")).toBeVisible();
    });
    await pass.escape();
    await pass.state("art-variants", async () => {
      await page.getByTestId("art-download-COV").click();
      await expect(page.getByTestId("art-variants-dialog")).toBeVisible();
      await page.waitForTimeout(1500);
    });
    await pass.escape();
    await pass.state("art-slot-menu", async () => {
      await page.getByTestId("art-slot-COV").scrollIntoViewIfNeeded();
      await page.getByTestId("art-replace-COV").click();
      await expect(page.getByRole("menu")).toBeVisible();
    });
    await pass.escape();
    if (!run.simple) await pass.state("vmc-new", async () => {
      await page.getByTestId("cfg-vmc-create-0").click();
      await expect(page.getByRole("dialog")).toBeVisible();
    });
    await pass.escape();
    await pass.state("cfg-plan", async () => {
      const settings = page.getByTestId("game-settings");
      await settings.getByRole("checkbox", { name: w.mode2 }).check();
      await page.getByTestId("cfg-save").click();
      await expect(page.getByTestId("plan-dialog").getByTestId("plan-cfg-changes")).toBeVisible();
    });
    await pass.escape();
    if (!run.simple) await pass.state("game-ul", async () => {
      await page.goto("/library");
      await page.getByRole("listitem", { name: /Split Game/ }).click();
      await expect(page.getByTestId("game-parts")).toBeVisible();
    });
    await pass.state("game-no-cover-warning", async () => {
      await page.goto("/library");
      await page.getByRole("listitem", { name: /Wrong Name/ }).click();
      await expect(page.getByTestId("game-warnings")).toBeVisible();
    });

    // ---- Add games ----
    await pass.state("add", async () => {
      await page.goto("/library/add");
      await expect(page.getByTestId("drop-zone")).toBeVisible();
    });
    await pass.state("add-browser", async () => {
      await page.getByTestId("browse-server").click();
      await expect(page.getByTestId("folder-browser")).toBeVisible();
    });
    await pass.state("add-sources", async () => {
      const browser = page.getByTestId("folder-browser");
      await browser.getByRole("button", { name: "sources" }).click();
      await browser.getByRole("checkbox", { name: /Lf Game\.iso/ }).check();
      await browser.getByTestId("browser-add").click();
      await expect(page.getByTestId("source-list")).toContainText("Lf Game.iso");
    });
    await pass.state("add-plan", async () => {
      await page.getByTestId("preview-plan").click();
      await expect(page.getByTestId("import-plan").getByTestId("plan-confirm")).toBeEnabled({ timeout: 60_000 });
      await page.getByTestId("import-plan").scrollIntoViewIfNeeded();
    });
    await pass.state("add-done", async () => {
      await page.getByTestId("import-plan").getByTestId("plan-confirm").click();
      // Simple mode comes back to the library; advanced mode opens the jobs page.
      await expect(page).toHaveURL(run.simple ? /\/library$/ : /\/jobs$/, { timeout: 60_000 });
      if (run.simple) await expect(page.getByRole("listitem", { name: /Dead or Alive 2|Lf Game/ })).toBeVisible({ timeout: 60_000 });
      else await expect(page.getByTestId("job-card").first().getByTestId("job-outcomes")).toBeVisible({ timeout: 60_000 });
      await page.waitForTimeout(600);
    });
    if (!run.simple) {
      await pass.state("jobs", async () => {
        await page.goto("/jobs");
        await expect(page.getByTestId("job-card").first()).toBeVisible();
      });
      await pass.state("jobs-log", async () => {
        await page.getByTestId("job-card").first().getByRole("button", { name: w.logs }).click();
        await expect(page.getByTestId("job-log").first()).toBeVisible();
      });
    }
    // Back to five games.
    await pass.state("delete-added-game", async () => {
      await page.goto("/library");
      await page.getByRole("listitem", { name: /Dead or Alive 2|Lf Game/ }).click();
      await page.getByRole("button", { name: w.delete, exact: true }).click();
      await page.getByTestId("delete-preview").click();
      const plan = page.getByTestId("plan-dialog");
      await plan.getByLabel(w.deleteConfirm).check();
      await plan.getByTestId("plan-confirm").click();
      await expect(page).toHaveURL(/\/library$/);
      await page.waitForTimeout(800);
    });

    // ---- Settings ----
    await pass.state("settings", async () => {
      await page.goto("/settings");
      await expect(page.getByTestId("about")).toBeVisible();
    });
    if (!run.simple) {
      await pass.state("settings-redump-import-browser", async () => {
        await page.getByTestId("redump-panel").first().getByTestId("redump-import").click();
        await expect(page.getByTestId("folder-browser")).toBeVisible();
      });
      await pass.escape();
    }

    // ---- Connection lost (server mode) ----
    await pass.state("connection-lost", async () => {
      await page.route("**/api/**", (route) => route.abort());
      await page.goto("/library").catch(() => {});
      await page.waitForTimeout(3000);
    });
    await page.unrouteAll({ behavior: "ignoreErrors" });

    writeFileSync(
      path.join(OUT, `report--${pass.suffix}.json`),
      JSON.stringify({ run, consoleErrors, states: pass.report }, null, 2),
    );
    await emptyStick(request);
    resetEmulator(fixture.emulator);
  });
}
