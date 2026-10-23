import { describe, expect, it } from "vitest";

import en from "@/locales/en/translation.json";
import enRaw from "@/locales/en/translation.json?raw";
import ru from "@/locales/ru/translation.json";
import ruRaw from "@/locales/ru/translation.json?raw";

/**
 * The catalogues must agree (docs/I18N.md): the same keys in every
 * language with the plural forms each language needs, the same
 * placeholders, no duplicate keys, no stray whitespace or ASCII
 * stand-ins for typography, and a translation for every code the core
 * reports and every value the UI turns into a key.
 */

const catalogues = { en, ru } as const;
const raws = { en: enRaw, ru: ruRaw } as const;
type Language = keyof typeof catalogues;
const languages = Object.keys(catalogues) as Language[];

const PLURAL_SUFFIX = /_(zero|one|two|few|many|other)$/;

function flat(value: unknown, prefix = ""): Record<string, string> {
  if (typeof value === "string") return { [prefix]: value };
  if (typeof value !== "object" || value === null) return {};
  const out: Record<string, string> = {};
  for (const [k, v] of Object.entries(value)) Object.assign(out, flat(v, prefix ? `${prefix}.${k}` : k));
  return out;
}

const flats = { en: flat(en), ru: flat(ru) } as const;

/** Keys without plural suffixes. */
function baseKeys(entries: Record<string, string>): string[] {
  return [...new Set(Object.keys(entries).map((key) => key.replace(PLURAL_SUFFIX, "")))].sort();
}

/** Plural forms per base key, for keys that have any. */
function pluralForms(entries: Record<string, string>): Map<string, string[]> {
  const forms = new Map<string, string[]>();
  for (const key of Object.keys(entries)) {
    const match = PLURAL_SUFFIX.exec(key);
    if (!match?.[1]) continue;
    const base = key.slice(0, match.index);
    forms.set(base, [...(forms.get(base) ?? []), match[1]].sort());
  }
  return forms;
}

function placeholders(text: string): string[] {
  return [...new Set([...text.matchAll(/\{\{(\w+)\}\}/g)].map((m) => m[1] ?? ""))].sort();
}

/** Whether `key` is in the catalogue, as a string or as plural forms. */
function has(entries: Record<string, string>, key: string): boolean {
  return key in entries || `${key}_other` in entries;
}

/** Object keys that appear twice in one object of raw JSON (`JSON.parse` keeps the last). */
function duplicateKeys(text: string): string[] {
  const found: string[] = [];
  const stack: { keys: Set<string>; path: string; key: string | null }[] = [];
  let last: string | null = null;
  for (let i = 0; i < text.length; i += 1) {
    const c = text[i];
    if (c === '"') {
      let end = i + 1;
      while (end < text.length && text[end] !== '"') end += text[end] === "\\" ? 2 : 1;
      last = JSON.parse(text.slice(i, end + 1)) as string;
      i = end;
    } else if (c === ":") {
      const top = stack.at(-1);
      if (top && last !== null) {
        if (top.keys.has(last)) found.push(top.path + last);
        top.keys.add(last);
        top.key = last;
      }
      last = null;
    } else if (c === "{") {
      const top = stack.at(-1);
      stack.push({ keys: new Set(), path: top?.key ? `${top.path}${top.key}.` : "", key: null });
    } else if (c === "}") {
      stack.pop();
    } else if (c === ",") {
      last = null;
    }
  }
  return found;
}

/** The string values a TypeScript union of literals allows. */
function unionValues(source: string): string[] {
  const body = /export type \w+ = ([^;]+);/.exec(source)?.[1] ?? "";
  return [...body.matchAll(/"([^"]+)"/g)].map((m) => m[1] ?? "");
}

const generated = import.meta.glob<string>("../../src/api/generated/*.ts", {
  query: "?raw",
  import: "default",
  eager: true,
});

function generatedType(name: string): string {
  const source = generated[`../../src/api/generated/${name}.ts`];
  if (source === undefined) throw new Error(`no generated type ${name}`);
  return source;
}

/** Rust sources whose notice codes the UI shows (`notices.<code>`). */
const rust = import.meta.glob<string>(
  [
    "../../../crates/yaoplm-core/src/**/*.rs",
    "../../../crates/yaoplm-api/src/**/*.rs",
    "../../../crates/yaoplm-device/src/advice.rs",
  ],
  { query: "?raw", import: "default", eager: true },
);

/** Every code a `Notice`, a skip or a `fn code()` match arm names. */
function noticeCodes(): string[] {
  const codes = new Set<string>();
  for (const [path, source] of Object.entries(rust)) {
    const code = String.raw`"([a-z][A-Za-z0-9]+)"`;
    const patterns = [
      new RegExp(String.raw`Notice::(?:info|warning|error)\(\s*${code}`, "g"),
      new RegExp(String.raw`\bskip\(\s*[^,()]+(?:\([^()]*\))?\s*,\s*${code}`, "g"),
    ];
    if (path.endsWith("model.rs") || path.endsWith("advice.rs")) patterns.push(new RegExp(String.raw`=>\s*${code}`, "g"));
    for (const pattern of patterns) for (const match of source.matchAll(pattern)) if (match[1]) codes.add(match[1]);
  }
  return [...codes].sort();
}

describe("locales", () => {
  it("have the same keys in every language", () => {
    for (const language of languages) expect(baseKeys(flats[language]), language).toEqual(baseKeys(flats.en));
  });

  it("have every plural form each language needs, and only for plural keys", () => {
    const plural = new Set(pluralForms(flats.en).keys());
    for (const language of languages) {
      const needed = [...new Intl.PluralRules(language).resolvedOptions().pluralCategories].sort();
      const forms = pluralForms(flats[language]);
      for (const [base, found] of forms) expect(found, `${language}: ${base}`).toEqual(needed);
      expect([...forms.keys()].sort(), language).toEqual([...plural].sort());
    }
  });

  it("use the same placeholders in every language", () => {
    for (const base of baseKeys(flats.en)) {
      const of = (language: Language) =>
        placeholders(
          Object.entries(flats[language])
            .filter(([key]) => key.replace(PLURAL_SUFFIX, "") === base)
            .map(([, text]) => text)
            .join(" "),
        ).filter((name) => name !== "count");
      for (const language of languages) expect(of(language), `${language}: ${base}`).toEqual(of("en"));
    }
  });

  it("have no duplicate keys", () => {
    for (const language of languages) expect(duplicateKeys(raws[language]), language).toEqual([]);
  });

  it("have no empty strings, stray spaces or ASCII stand-ins", () => {
    for (const language of languages) {
      for (const [key, text] of Object.entries(flats[language])) {
        const where = `${language}: ${key} = ${JSON.stringify(text)}`;
        expect(text.trim().length > 0, where).toBe(true);
        expect(text, where).toBe(text.trim());
        expect(text.includes("  "), where).toBe(false);
        expect(text.includes("..."), `${where} (use …)`).toBe(false);
        expect(/\s[,.;:!?](\s|$)/.test(text), `${where} (space before punctuation)`).toBe(false);
        expect(text.includes('"'), `${where} (use «» in ru, “” in en)`).toBe(false);
        expect(text.includes("'"), `${where} (use ’)`).toBe(false);
      }
    }
  });

  it("translate what is English in the source", () => {
    // A Russian string equal to the English one, with English words in it.
    const english = /[a-z]{3,} [a-z]{2,}/;
    for (const [key, text] of Object.entries(flats.ru)) {
      if (text === flats.en[key] && english.test(text)) expect.fail(`ru: ${key} is not translated: ${text}`);
    }
  });

  it("translate every notice code the core reports", () => {
    const codes = noticeCodes();
    expect(codes.length).toBeGreaterThan(50);
    for (const language of languages) {
      const missing = codes.filter((code) => !has(flats[language], `notices.${code}`));
      expect(missing, language).toEqual([]);
    }
  });

  it("translate every value the UI turns into a key", () => {
    const keyed: [string, string[]][] = [
      ["StepOp", ["plan.steps"]],
      ["JobStatus", ["jobs.status"]],
      ["TxOutcome", ["jobs.outcome"]],
      ["TxLogState", ["jobs.txState"]],
      ["StepLogState", ["jobs.stepState"]],
      ["ArtLogState", ["jobs.artState"]],
      ["VerifyStatus", ["dump.status", "dump.count", "jobs.verifyState"]],
      ["UnknownReason", ["dump.reason", "dump.reasonLong"]],
      ["RedumpUpdate", ["dump.update"]],
      ["ConvertTarget", ["convert.targets", "convert.hints"]],
      ["NoticeLevel", ["noticeLevel"]],
      ["NameStyle", ["settings.nameStyles", "settings.nameStyleHints"]],
      ["LoaderProfile", ["settings.profiles"]],
      ["NameFormat", ["game.nameFormats"]],
      ["DriveBus", ["devices.bus"]],
      ["DriveKind", ["devices.kind"]],
      ["PartitionTable", ["devices.tables"]],
    ];
    for (const [type, prefixes] of keyed) {
      const values = unionValues(generatedType(type));
      expect(values.length, type).toBeGreaterThan(0);
      for (const language of languages) {
        for (const prefix of prefixes) {
          const missing = values.filter((value) => !has(flats[language], `${prefix}.${value}`));
          expect(missing, `${language}: ${type} under ${prefix}`).toEqual([]);
        }
      }
    }
  });
});
