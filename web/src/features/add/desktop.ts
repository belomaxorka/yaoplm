import { isTauri } from "@tauri-apps/api/core";

/**
 * Native pickers and drag-and-drop exist only in the desktop app. The
 * modules are loaded lazily so the browser build never touches them.
 */

export const IMAGE_EXTENSIONS = ["iso", "zso", "bin", "cue"];

export async function pickFiles(): Promise<string[]> {
  if (!isTauri()) return [];
  const { open } = await import("@tauri-apps/plugin-dialog");
  const picked = await open({
    multiple: true,
    filters: [{ name: "PS2 images", extensions: IMAGE_EXTENSIONS }],
  });
  if (!picked) return [];
  return Array.isArray(picked) ? picked : [picked];
}

export async function pickFolder(): Promise<string | null> {
  if (!isTauri()) return null;
  const { open } = await import("@tauri-apps/plugin-dialog");
  const picked = await open({ directory: true, multiple: false });
  return typeof picked === "string" ? picked : null;
}

/** Calls `onDrop` with the paths dropped onto the window; returns an unlisten function. */
export function listenForDrops(onDrop: (paths: string[]) => void, onHover?: (hovering: boolean) => void): () => void {
  if (!isTauri()) return () => undefined;
  let unlisten: (() => void) | null = null;
  let cancelled = false;
  void import("@tauri-apps/api/webview").then(async ({ getCurrentWebview }) => {
    const stop = await getCurrentWebview().onDragDropEvent((event) => {
      if (event.payload.type === "enter" || event.payload.type === "over") onHover?.(true);
      else if (event.payload.type === "leave") onHover?.(false);
      else if (event.payload.type === "drop") {
        onHover?.(false);
        onDrop(event.payload.paths);
      }
    });
    if (cancelled) stop();
    else unlisten = stop;
  });
  return () => {
    cancelled = true;
    unlisten?.();
  };
}
