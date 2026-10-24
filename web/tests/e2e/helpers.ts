import type { APIRequestContext, Page } from "@playwright/test";

interface Device {
  root: string;
}

/** The fixture roots the e2e server was started with (`server.mjs`). */
export async function roots(
  request: APIRequestContext,
): Promise<{ library: string; sources: string; mirror: string }> {
  const response = await request.post("/api/rpc", { data: { type: "ListDevices" } });
  const body = (await response.json()) as { devices: Device[] };
  const find = (name: string) => {
    const device = body.devices.find((d) => d.root.replace(/[\\/]+$/, "").endsWith(name));
    if (!device) throw new Error(`no fixture root ending in ${name}: ${JSON.stringify(body)}`);
    return device.root;
  };
  return { library: find("library"), sources: find("sources"), mirror: find("mirror") };
}

/** `<root>/<name>` with the separator the server uses. */
export function joinPath(root: string, ...names: string[]): string {
  const sep = root.includes("\\") ? "\\" : "/";
  return [root.replace(/[\\/]+$/, ""), ...names].join(sep);
}

/** Opens the fixture library before the page loads, like a user who picked it earlier. */
export async function openLibrary(
  page: Page,
  request: APIRequestContext,
  extra: { language?: "en" | "ru"; theme?: "light" | "dark"; view?: "grid" | "list"; simple?: boolean } = {},
): Promise<string> {
  const { library, mirror } = await roots(request);
  // The cover sources read the fixture mirrors instead of the network.
  const artSources = {
    xlenore: { enabled: true, mirror: joinPath(mirror, "xlenore") },
    libretro: { enabled: true, mirror: joinPath(mirror, "libretro") },
  };
  await page.addInitScript(
    ([root, language, theme, view, sources, simpleMode]) => {
      // Runs on every load, so settings a test changed survive a reload.
      const current: unknown = JSON.parse(localStorage.getItem("yaoplm.settings") ?? "{}");
      const kept = typeof current === "object" && current !== null ? current : {};
      localStorage.setItem("yaoplm.settings", JSON.stringify({ simpleMode, ...kept, root, view, artSources: sources }));
      if (language) localStorage.setItem("yaoplm.language", language);
      if (theme) localStorage.setItem("yaoplm.theme", theme);
    },
    // Most specs test the advanced tools; simple mode is the app's default.
    [library, extra.language ?? "", extra.theme ?? "", extra.view ?? "grid", artSources, extra.simple ?? false] as const,
  );
  return library;
}

/** Runs one of the library's advanced tools from its "More" menu. */
export async function libraryTool(page: Page, testId: "import-pictures" | "verify-dumps" | "convert-shown"): Promise<void> {
  await page.getByTestId("library-more").click();
  await page.getByTestId(testId).click();
}
