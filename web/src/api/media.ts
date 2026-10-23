import { convertFileSrc, isTauri } from "@tauri-apps/api/core";

import type { Art } from "@/api/generated/Art";
import type { Game } from "@/api/generated/Game";
import { readStorage } from "@/lib/storage";

const TOKEN_KEY = "yaoplm.token";
const LOADABLE = ["png", "jpg", "jpeg", "bmp"];

function extensionOf(path: string): string {
  const dot = path.lastIndexOf(".");
  return dot >= 0 ? path.slice(dot + 1).toLowerCase() : "";
}

/**
 * The front cover the UI shows: the PNG first, because that is what OPL
 * 1.2 loads; another format only when there is nothing else (the scan
 * warns about it).
 */
export function coverOf(game: Game): Art | undefined {
  const covers = game.art.filter((art) => art.kind.toUpperCase() === "COV" && LOADABLE.includes(extensionOf(art.path)));
  return covers.find((art) => extensionOf(art.path) === "png") ?? covers[0];
}

export function hasCover(game: Game): boolean {
  return coverOf(game) !== undefined;
}

/**
 * Where the browser loads a cover from: the asset protocol on the desktop
 * (the open root's ART folder is allowed at runtime), `/media/art` on the
 * server. The mtime in the query keeps a replaced cover from being served
 * from the browser cache.
 */
export function artUrl(art: Art): string {
  const version = art.modified === null ? String(art.size) : `${art.modified}-${art.size}`;
  if (isTauri()) {
    const base = convertFileSrc(art.path);
    return `${base}${base.includes("?") ? "&" : "?"}v=${encodeURIComponent(version)}`;
  }
  const url = new URL("/media/art", window.location.origin);
  url.searchParams.set("path", art.path);
  url.searchParams.set("v", version);
  // <img> cannot send Authorization; the server accepts ?token= here (ADR-0012).
  const token = readStorage(TOKEN_KEY);
  if (token) url.searchParams.set("token", token);
  return url.toString();
}
