import { afterEach, describe, expect, it, vi } from "vitest";

import { installDesktopChrome } from "@/desktop";

// `isTauri` is swapped per test: the same code runs in the desktop window and
// in a browser tab of yaoplm-server.
const tauri = vi.hoisted(() => ({ on: false }));
vi.mock("@tauri-apps/api/core", () => ({ isTauri: () => tauri.on }));

function rightClick(target: Element): boolean {
  const event = new MouseEvent("contextmenu", { bubbles: true, cancelable: true, button: 2 });
  target.dispatchEvent(event);
  return event.defaultPrevented;
}

function field<K extends keyof HTMLElementTagNameMap>(tag: K, attributes: Record<string, string> = {}): HTMLElementTagNameMap[K] {
  const element = document.createElement(tag);
  for (const [name, value] of Object.entries(attributes)) element.setAttribute(name, value);
  document.body.append(element);
  return element;
}

describe("the desktop window's chrome (UI-5)", () => {
  let uninstall: () => void = () => undefined;

  afterEach(() => {
    uninstall();
    document.body.innerHTML = "";
  });

  it("marks the document and blocks the WebView2 menu outside editable fields", () => {
    tauri.on = true;
    uninstall = installDesktopChrome();
    expect(document.documentElement).toHaveAttribute("data-desktop");
    expect(rightClick(field("button"))).toBe(true);
    expect(rightClick(field("h2"))).toBe(true);
    expect(rightClick(document.body)).toBe(true);
    // Cut, copy and paste are wanted where one types.
    expect(rightClick(field("input"))).toBe(false);
    expect(rightClick(field("input", { type: "search" }))).toBe(false);
    expect(rightClick(field("textarea"))).toBe(false);
    expect(rightClick(field("div", { contenteditable: "true" }))).toBe(false);
    // A checkbox, a disabled or read-only field have no caret.
    expect(rightClick(field("input", { type: "checkbox" }))).toBe(true);
    expect(rightClick(field("input", { disabled: "" }))).toBe(true);
    expect(rightClick(field("input", { readonly: "" }))).toBe(true);
  });

  it("leaves a browser tab alone", () => {
    tauri.on = false;
    uninstall = installDesktopChrome();
    expect(document.documentElement).not.toHaveAttribute("data-desktop");
    expect(rightClick(field("button"))).toBe(false);
  });

  it("takes everything back when uninstalled", () => {
    tauri.on = true;
    installDesktopChrome()();
    expect(document.documentElement).not.toHaveAttribute("data-desktop");
    expect(rightClick(field("button"))).toBe(false);
  });
});
