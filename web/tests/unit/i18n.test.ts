import { describe, expect, it } from "vitest";

import en from "@/locales/en/translation.json";
import ru from "@/locales/ru/translation.json";

const PLURAL_SUFFIX = /_(zero|one|two|few|many|other)$/;

function keys(value: unknown, prefix = ""): string[] {
  if (typeof value !== "object" || value === null) return [prefix];
  return Object.entries(value).flatMap(([key, child]) => keys(child, prefix ? `${prefix}.${key}` : key));
}

/** Plural forms differ per language (ru has few/many); compare the base keys. */
function baseKeys(value: unknown): string[] {
  return [...new Set(keys(value).map((key) => key.replace(PLURAL_SUFFIX, "")))].sort();
}

function leaves(value: unknown): unknown[] {
  if (typeof value !== "object" || value === null) return [value];
  return Object.values(value).flatMap(leaves);
}

function placeholders(text: string): string[] {
  return [...text.matchAll(/\{\{(\w+)\}\}/g)].map((m) => m[1] ?? "").sort();
}

function flat(value: unknown, prefix = ""): Record<string, string> {
  if (typeof value === "string") return { [prefix]: value };
  if (typeof value !== "object" || value === null) return {};
  const out: Record<string, string> = {};
  for (const [k, v] of Object.entries(value)) Object.assign(out, flat(v, prefix ? `${prefix}.${k}` : k));
  return out;
}

describe("locales", () => {
  it("en and ru have the same keys", () => {
    expect(baseKeys(ru)).toEqual(baseKeys(en));
  });

  it("has no empty strings", () => {
    for (const catalogue of [en, ru]) {
      for (const leaf of leaves(catalogue)) {
        expect(typeof leaf === "string" && leaf.trim().length > 0).toBe(true);
      }
    }
  });

  it("uses the same placeholders in both languages", () => {
    const enFlat = flat(en);
    const ruFlat = flat(ru);
    for (const [key, text] of Object.entries(enFlat)) {
      const other = ruFlat[key] ?? ruFlat[key.replace(PLURAL_SUFFIX, "_other")];
      if (other === undefined) continue;
      expect(placeholders(other), key).toEqual(placeholders(text));
    }
  });
});
