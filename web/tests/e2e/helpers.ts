import { readdirSync, rmSync } from "node:fs";
import path from "node:path";

import { type APIRequestContext, expect, type Locator, type Page } from "@playwright/test";

interface Device {
  root: string;
}

/** The fixture roots the e2e server was started with (`server.mjs`). */
export async function roots(
  request: APIRequestContext,
): Promise<{ library: string; sources: string; mirror: string; stick: string; emulator: string }> {
  const response = await request.post("/api/rpc", { data: { type: "ListDevices" } });
  const body = (await response.json()) as { devices: Device[] };
  const find = (name: string) => {
    const device = body.devices.find((d) => d.root.replace(/[\\/]+$/, "").endsWith(name));
    if (!device) throw new Error(`no fixture root ending in ${name}: ${JSON.stringify(body)}`);
    return device.root;
  };
  return {
    library: find("library"),
    sources: find("sources"),
    mirror: find("mirror"),
    stick: find("stick"),
    emulator: find("emulator"),
  };
}

/** `<root>/<name>` with the separator the server uses. */
export function joinPath(root: string, ...names: string[]): string {
  const sep = root.includes("\\") ? "\\" : "/";
  return [root.replace(/[\\/]+$/, ""), ...names].join(sep);
}

/** Opens the fixture library before the page loads, like a user who picked it earlier. */
/** Unfolds one section of a game's settings; a section with saved values is open already. */
export async function openSection(settings: Locator, section: string): Promise<void> {
  const details = settings.getByTestId(`cfg-section-${section}`);
  if (!(await details.evaluate((element) => (element as HTMLDetailsElement).open))) {
    await details.locator("summary").click();
  }
  await expect(details).toHaveAttribute("open");
}

export async function openLibrary(
  page: Page,
  request: APIRequestContext,
  extra: { language?: "en" | "ru"; theme?: "light" | "dark"; view?: "grid" | "list"; simple?: boolean } = {},
): Promise<string> {
  const { library, mirror } = await roots(request);
  // The picture sources read the fixture mirrors instead of the network;
  // ScreenScraper has none and stays off.
  const artSources = {
    xlenore: { enabled: true, mirror: joinPath(mirror, "xlenore") },
    oplm: { enabled: true, mirror: joinPath(mirror, "oplm") },
    libretro: { enabled: true, mirror: joinPath(mirror, "libretro") },
    screenscraper: { enabled: false, mirror: "" },
  };
  // Loader releases come from the fixture mirror too (LDR-2).
  const loaderMirror = joinPath(mirror, "loaders");
  await page.addInitScript(
    ([root, language, theme, view, sources, simpleMode, loaders]) => {
      // Runs on every load, so settings a test changed survive a reload.
      const current: unknown = JSON.parse(localStorage.getItem("yaoplm.settings") ?? "{}");
      const kept = typeof current === "object" && current !== null ? current : {};
      localStorage.setItem(
        "yaoplm.settings",
        JSON.stringify({ simpleMode, ...kept, root, view, artSources: sources, loaderMirror: loaders }),
      );
      if (language) localStorage.setItem("yaoplm.language", language);
      if (theme) localStorage.setItem("yaoplm.theme", theme);
    },
    // Most specs test the advanced tools; simple mode is the app's default.
    [library, extra.language ?? "", extra.theme ?? "", extra.view ?? "grid", artSources, extra.simple ?? false, loaderMirror] as const,
  );
  return library;
}

/** Runs one of the library's advanced tools from its "More" menu. */
export async function libraryTool(page: Page, testId: "import-pictures" | "verify-dumps" | "convert-shown"): Promise<void> {
  await page.getByTestId("library-more").click();
  await page.getByTestId(testId).click();
}

export function stickCard(page: Page) {
  return page
    .getByTestId("device-card")
    .filter({ has: page.locator("span.font-mono", { hasText: /[\\/]stick$/ }) })
    .first();
}

const isStick = (root: unknown) => typeof root === "string" && /[\\/]stick$/.test(root);

/**
 * The stick is a temp folder on this machine's system disk, where no loader
 * may go (§5.10.1); the device list shows it as the FAT32 USB stick it
 * stands for (FAT32 unless told otherwise). The plan and everything after
 * it are the server's own.
 */
export async function stickAsUsb(page: Page, { fileSystem = "FAT32" }: { fileSystem?: "FAT32" | "exFAT" } = {}): Promise<void> {
  await page.route("**/api/rpc", async (route) => {
    const body = route.request().postDataJSON() as { type?: string } | null;
    if (body?.type !== "ListDevices") return route.fallback();
    let response;
    try {
      response = await route.fetch();
    } catch {
      // The device list refreshes on a timer: a refresh in flight when the
      // test ends throws "Test ended" (seen in loader-update.spec.ts).
      return;
    }
    const json = (await response.json()) as { devices: Record<string, unknown>[] };
    for (const device of json.devices.filter((d) => isStick(d.root))) {
      Object.assign(device, {
        fileSystem,
        fsName: fileSystem,
        kind: "removable",
        bus: "usb",
        external: true,
        system: false,
        readOnly: false,
        model: null,
        serial: null,
        partition: null,
        advice: [],
        // A USB stick is plugged into the console (ADR-0047); empty, it is
        // not asked about its OPL.
        mode: {
          mode: "ps2",
          source: "detected",
          detected: "ps2",
          place: { key: "reasons.placeRemovable", params: {} },
          placeText: "a removable device",
        },
        askProfile: false,
      });
    }
    await route.fulfill({ response, json });
  });
}

/** Every test starts from an empty stick. */
export async function emptyStick(request: APIRequestContext): Promise<string> {
  const { stick } = await roots(request);
  for (const name of readdirSync(stick)) rmSync(path.join(stick, name), { recursive: true, force: true });
  return stick;
}

export interface Answer {
  type: string;
  [key: string]: unknown;
}

/** One request to the server's API. */
export async function rpc(request: APIRequestContext, body: object): Promise<Answer> {
  const response = await request.post("/api/rpc", { data: body });
  const json = (await response.json()) as Answer;
  if (!response.ok()) throw new Error(`${JSON.stringify(body)}: ${JSON.stringify(json)}`);
  return json;
}

/** Runs a job to its end. */
export async function job(request: APIRequestContext, body: object): Promise<Answer> {
  const { job } = await rpc(request, body);
  for (;;) {
    const info = await rpc(request, { type: "GetJob", job });
    if (info.status === "finished") return info;
    if (info.status === "failed" || info.status === "cancelled") throw new Error(JSON.stringify(info));
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
}

/**
 * An open menu's items on one line each and not cut (UI-5): a menu as wide
 * as its trigger broke "Reinstall OPL…" and "Switch OPL…" onto two lines
 * under the icon button and clipped them (the owner, 2026-10-27).
 */
export async function menuItemProblems(page: Page): Promise<string[]> {
  return page.evaluate(() => {
    const problems: string[] = [];
    const items = document.querySelectorAll<HTMLElement>('[role="menuitem"], [role="menuitemradio"], [role="menuitemcheckbox"]');
    for (const item of items) {
      if (!item.checkVisibility()) continue;
      const name = item.textContent?.trim().slice(0, 40) ?? "";
      if (item.scrollWidth > item.clientWidth + 1) {
        problems.push(`the menu item "${name}" is cut by ${item.scrollWidth - item.clientWidth} px`);
      }
      const walker = document.createTreeWalker(item, NodeFilter.SHOW_TEXT);
      for (let node = walker.nextNode(); node; node = walker.nextNode()) {
        if (!node.textContent?.trim()) continue;
        const range = document.createRange();
        range.selectNodeContents(node);
        const lines = range.getClientRects().length;
        if (lines > 1) {
          problems.push(`the menu item "${name}" wraps onto ${lines} lines`);
          break;
        }
      }
    }
    return problems;
  });
}
