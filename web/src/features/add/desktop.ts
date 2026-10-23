import { invoke, isTauri } from "@tauri-apps/api/core";

/**
 * Native pickers and drag-and-drop exist only in the desktop app. The
 * modules are loaded lazily so the browser build never touches them.
 */

/** What the importer takes (IMP-1, IMP-1b); zip and 7z open like folders. */
export const IMAGE_EXTENSIONS = ["iso", "zso", "bin", "cue", "chd", "zip", "7z"];

export async function pickFiles(): Promise<string[]> {
  if (!isTauri()) return [];
  const { open } = await import("@tauri-apps/plugin-dialog");
  const picked = await open({
    multiple: true,
    filters: [{ name: "PS2 images and archives", extensions: IMAGE_EXTENSIONS }],
  });
  if (!picked) return [];
  return Array.isArray(picked) ? picked : [picked];
}

export const DAT_EXTENSIONS = ["dat", "xml", "zip"];

/** A Redump DAT to import (META-2): the file redump.info serves, or its zip. */
export async function pickDat(): Promise<string | null> {
  if (!isTauri()) return null;
  const { open } = await import("@tauri-apps/plugin-dialog");
  const picked = await open({
    multiple: false,
    filters: [{ name: "Redump DAT", extensions: DAT_EXTENSIONS }],
  });
  return typeof picked === "string" ? picked : null;
}

export async function pickFolder(): Promise<string | null> {
  if (!isTauri()) return null;
  const { open } = await import("@tauri-apps/plugin-dialog");
  const picked = await open({ directory: true, multiple: false });
  return typeof picked === "string" ? picked : null;
}

export const IMAGE_PICK_EXTENSIONS = ["png", "jpg", "jpeg", "bmp", "gif", "webp"];

/** One image file for a cover (ART-3). */
export async function pickImage(): Promise<string | null> {
  if (!isTauri()) return null;
  const { open } = await import("@tauri-apps/plugin-dialog");
  const picked = await open({
    multiple: false,
    filters: [{ name: "Images", extensions: IMAGE_PICK_EXTENSIONS }],
  });
  return typeof picked === "string" ? picked : null;
}

/** Archives a picture set comes in (ART-5); folders are picked with `pickFolder`. */
export const PICTURE_SET_EXTENSIONS = ["zip", "7z", "tar"];

/** Picture sets: OPL Manager's art backups, `art.tar`, or single pictures. */
export async function pickPictureSets(): Promise<string[]> {
  if (!isTauri()) return [];
  const { open } = await import("@tauri-apps/plugin-dialog");
  const picked = await open({
    multiple: true,
    filters: [{ name: "Picture sets", extensions: [...PICTURE_SET_EXTENSIONS, ...IMAGE_PICK_EXTENSIONS] }],
  });
  if (!picked) return [];
  return Array.isArray(picked) ? picked : [picked];
}

/**
 * Lets the WebView load `<root>/ART` through the asset protocol (ART-2).
 * Called after every scan, because the folder may be new.
 */
export async function allowArt(root: string): Promise<void> {
  if (!isTauri()) return;
  try {
    await invoke("allow_art", { root });
  } catch {
    // The folder does not exist yet; covers will show up after the next scan.
  }
}

/** Where a drop landed, in CSS pixels of the page. */
export interface DropPoint {
  x: number;
  y: number;
}

/**
 * Calls `onDrop` with the paths dropped onto the window and where they
 * landed; returns an unlisten function.
 */
export function listenForDrops(
  onDrop: (paths: string[], at: DropPoint) => void,
  onHover?: (hovering: boolean) => void,
): () => void {
  if (!isTauri()) return () => undefined;
  let unlisten: (() => void) | null = null;
  let cancelled = false;
  void import("@tauri-apps/api/webview").then(async ({ getCurrentWebview }) => {
    const stop = await getCurrentWebview().onDragDropEvent((event) => {
      if (event.payload.type === "enter" || event.payload.type === "over") onHover?.(true);
      else if (event.payload.type === "leave") onHover?.(false);
      else if (event.payload.type === "drop") {
        onHover?.(false);
        // The webview reports physical pixels.
        const scale = window.devicePixelRatio || 1;
        onDrop(event.payload.paths, { x: event.payload.position.x / scale, y: event.payload.position.y / scale });
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
