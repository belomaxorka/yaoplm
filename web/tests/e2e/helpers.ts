import type { APIRequestContext, Page } from "@playwright/test";

interface Device {
  root: string;
}

/** The fixture roots the e2e server was started with (`server.mjs`). */
export async function roots(request: APIRequestContext): Promise<{ library: string; sources: string }> {
  const response = await request.post("/api/rpc", { data: { type: "ListDevices" } });
  const body = (await response.json()) as { devices: Device[] };
  const find = (name: string) => {
    const device = body.devices.find((d) => d.root.replace(/[\\/]+$/, "").endsWith(name));
    if (!device) throw new Error(`no fixture root ending in ${name}: ${JSON.stringify(body)}`);
    return device.root;
  };
  return { library: find("library"), sources: find("sources") };
}

/** Opens the fixture library before the page loads, like a user who picked it earlier. */
export async function openLibrary(
  page: Page,
  request: APIRequestContext,
  extra: { language?: "en" | "ru"; theme?: "light" | "dark"; view?: "grid" | "list" } = {},
): Promise<string> {
  const { library } = await roots(request);
  await page.addInitScript(
    ([root, language, theme, view]) => {
      // Runs on every load, so settings a test changed survive a reload.
      const current: unknown = JSON.parse(localStorage.getItem("yaoplm.settings") ?? "{}");
      const kept = typeof current === "object" && current !== null ? current : {};
      localStorage.setItem("yaoplm.settings", JSON.stringify({ ...kept, root, view }));
      if (language) localStorage.setItem("yaoplm.language", language);
      if (theme) localStorage.setItem("yaoplm.theme", theme);
    },
    [library, extra.language ?? "", extra.theme ?? "", extra.view ?? "grid"] as const,
  );
  return library;
}
