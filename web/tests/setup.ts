import "@testing-library/jest-dom/vitest";

import { cleanup } from "@testing-library/react";
import { afterEach } from "vitest";

// jsdom has no matchMedia; the theme code asks for prefers-color-scheme.
Object.defineProperty(window, "matchMedia", {
  writable: true,
  value: (query: string): MediaQueryList => ({
    matches: false,
    media: query,
    onchange: null,
    addEventListener: () => undefined,
    removeEventListener: () => undefined,
    addListener: () => undefined,
    removeListener: () => undefined,
    dispatchEvent: () => false,
  }),
});

// Nor ResizeObserver; jsdom lays nothing out, so there is nothing to report.
globalThis.ResizeObserver ??= class {
  observe(): void {}
  unobserve(): void {}
  disconnect(): void {}
};

// Nor scrollIntoView; jsdom lays nothing out, so there is nowhere to scroll.
if (!("scrollIntoView" in Element.prototype)) {
  Object.defineProperty(Element.prototype, "scrollIntoView", { value: () => undefined, writable: true, configurable: true });
}

afterEach(() => {
  cleanup();
  window.localStorage.clear();
});
