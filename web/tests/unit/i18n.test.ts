import { describe, expect, it } from "vitest";

import en from "@/locales/en/translation.json";
import ru from "@/locales/ru/translation.json";

function keys(value: unknown, prefix = ""): string[] {
  if (typeof value !== "object" || value === null) return [prefix];
  return Object.entries(value).flatMap(([key, child]) => keys(child, prefix ? `${prefix}.${key}` : key));
}

function leaves(value: unknown): unknown[] {
  if (typeof value !== "object" || value === null) return [value];
  return Object.values(value).flatMap(leaves);
}

describe("locales", () => {
  it("en and ru have the same keys", () => {
    expect(keys(ru).sort()).toEqual(keys(en).sort());
  });

  it("has no empty strings", () => {
    for (const catalogue of [en, ru]) {
      for (const leaf of leaves(catalogue)) {
        expect(typeof leaf === "string" && leaf.trim().length > 0).toBe(true);
      }
    }
  });
});
