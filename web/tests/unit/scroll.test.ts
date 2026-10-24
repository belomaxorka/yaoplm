import { describe, expect, it } from "vitest";

/**
 * Scroll containers and the second scrollbar (walkthrough of 2026-10-24).
 *
 * - A container that scrolls must be `relative`. Screen-reader labels
 *   (`sr-only`) and Radix's hidden inputs are absolutely positioned; without
 *   a positioned ancestor they are placed against the document, and on a long
 *   page the window gets a scrollbar of its own next to the page's.
 * - A job log scrolls by itself, so nothing that wraps it scrolls too.
 */

const sources = import.meta.glob<string>(["../../src/**/*.tsx", "!../../src/**/*.test.tsx", "!../../src/components/ui/**"], {
  query: "?raw",
  import: "default",
  eager: true,
});

/** Every static class string with a scrolling overflow class. */
function scrollingClasses(source: string): string[] {
  return [...source.matchAll(/"([^"]*\boverflow(?:-[xy])?-(?:auto|scroll)\b[^"]*)"/g)].map((m) => m[1] ?? "");
}

describe("scroll containers", () => {
  it("found the sources", () => {
    expect(Object.keys(sources).length).toBeGreaterThan(20);
  });

  it("are relative, so absolutely positioned children stay inside", () => {
    const offenders = Object.entries(sources).flatMap(([file, source]) =>
      scrollingClasses(source)
        .filter((classes) => !/\b(relative|absolute|fixed|sticky)\b/.test(classes))
        .map((classes) => `${file}: "${classes}"`),
    );
    expect(offenders).toEqual([]);
  });

  it("never wrap a job log, which scrolls by itself", () => {
    const offenders = Object.entries(sources).flatMap(([file, source]) =>
      [...source.matchAll(/className="([^"]*)"[^>]*>\s*<JobLog\b/g)]
        .filter((m) => /\boverflow(?:-[xy])?-(?:auto|scroll)\b/.test(m[1] ?? ""))
        .map((m) => `${file}: "${m[1]}"`),
    );
    expect(offenders).toEqual([]);
  });
});
