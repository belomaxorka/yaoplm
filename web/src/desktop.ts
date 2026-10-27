import { isTauri } from "@tauri-apps/api/core";

/**
 * The desktop window is an application, not a web page (UI-5; the owner's
 * remarks of 2026-10-26): WebView2's own context menu ("Back", "Reload",
 * "Save as", "Print") opens nowhere but in editable fields, where cut, copy
 * and paste are wanted, and the interface cannot be swept into a selection.
 * Both live in the web layer so the desktop and the server share one code:
 * `data-desktop` on `<html>` switches the rules in `index.css` on, and the
 * elements people copy from (paths, IDs, titles, launcher lines, errors,
 * logs) carry the `selectable` class. In a browser nothing changes.
 */
export function installDesktopChrome(doc: Document = document): () => void {
  if (!isTauri()) return () => undefined;
  doc.documentElement.setAttribute("data-desktop", "");
  const onContextMenu = (event: Event) => {
    if (!isEditable(event.target)) event.preventDefault();
  };
  doc.addEventListener("contextmenu", onContextMenu);
  return () => {
    doc.removeEventListener("contextmenu", onContextMenu);
    doc.documentElement.removeAttribute("data-desktop");
  };
}

function isEditable(target: EventTarget | null): boolean {
  if (!(target instanceof Element)) return false;
  const editable = target.closest("input, textarea, [contenteditable]");
  if (!editable) return false;
  if (editable instanceof HTMLInputElement) return !editable.disabled && !editable.readOnly && !NON_TEXT_INPUTS.has(editable.type);
  if (editable instanceof HTMLTextAreaElement) return !editable.disabled && !editable.readOnly;
  return editable.getAttribute("contenteditable") !== "false";
}

/** Inputs without a caret: nothing to cut or paste. */
const NON_TEXT_INPUTS = new Set(["button", "checkbox", "color", "file", "image", "radio", "range", "reset", "submit"]);
