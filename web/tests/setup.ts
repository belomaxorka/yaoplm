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

afterEach(() => {
  cleanup();
  window.localStorage.clear();
});
