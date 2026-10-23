import { isTauri } from "@tauri-apps/api/core";

/** OPL rejects art over 1024 px (ADR-0005); nothing larger needs to cross the IPC. */
const MAX_SIDE = 1024;

export interface PastedImage {
  /** PNG bytes, base64. */
  data: string;
  name: string;
}

function toBase64(blob: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onerror = () => reject(reader.error ?? new Error("cannot read the image"));
    reader.onload = () => {
      const text = typeof reader.result === "string" ? reader.result : "";
      resolve(text.slice(text.indexOf(",") + 1));
    };
    reader.readAsDataURL(blob);
  });
}

function fit(width: number, height: number): { width: number; height: number } {
  const scale = Math.min(1, MAX_SIDE / Math.max(width, height));
  return { width: Math.max(1, Math.round(width * scale)), height: Math.max(1, Math.round(height * scale)) };
}

/** Draws `source` into a canvas no larger than 1024 px and encodes a PNG. */
async function encodePng(source: CanvasImageSource, width: number, height: number): Promise<string> {
  const size = fit(width, height);
  const canvas = document.createElement("canvas");
  canvas.width = size.width;
  canvas.height = size.height;
  const context = canvas.getContext("2d");
  if (!context) throw new Error("no 2D canvas");
  context.drawImage(source, 0, 0, size.width, size.height);
  const blob = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, "image/png"));
  if (!blob) throw new Error("cannot encode the image");
  return toBase64(blob);
}

/** A browser File or Blob (a drop, a paste) as PNG base64, downscaled to 1024 px. */
export async function imageBlobToPng(blob: Blob): Promise<string> {
  const bitmap = await createImageBitmap(blob);
  try {
    return await encodePng(bitmap, bitmap.width, bitmap.height);
  } finally {
    bitmap.close();
  }
}

/**
 * The image on the clipboard, if any (ART-3). The desktop reads it through
 * the clipboard plugin (WebView2 does not grant the Clipboard API);
 * browsers use `navigator.clipboard.read()`.
 */
export async function readClipboardImage(): Promise<PastedImage | null> {
  if (isTauri()) {
    const { readImage } = await import("@tauri-apps/plugin-clipboard-manager");
    const image = await readImage();
    try {
      const [{ width, height }, rgba] = await Promise.all([image.size(), image.rgba()]);
      if (width === 0 || height === 0) return null;
      const pixels = new ImageData(new Uint8ClampedArray(rgba.buffer, rgba.byteOffset, rgba.byteLength), width, height);
      const bitmap = await createImageBitmap(pixels);
      try {
        return { data: await encodePng(bitmap, width, height), name: "clipboard" };
      } finally {
        bitmap.close();
      }
    } finally {
      await image.close();
    }
  }
  if (!("clipboard" in navigator) || typeof navigator.clipboard.read !== "function") return null;
  const items = await navigator.clipboard.read();
  for (const item of items) {
    const type = item.types.find((t) => t.startsWith("image/"));
    if (!type) continue;
    const blob = await item.getType(type);
    return { data: await imageBlobToPng(blob), name: "clipboard" };
  }
  return null;
}

export const IMAGE_EXTENSIONS = ["png", "jpg", "jpeg", "bmp", "gif", "webp"];

export function isImageName(name: string): boolean {
  const dot = name.lastIndexOf(".");
  return dot >= 0 && IMAGE_EXTENSIONS.includes(name.slice(dot + 1).toLowerCase());
}
