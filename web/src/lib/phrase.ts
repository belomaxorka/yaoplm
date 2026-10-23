import type { ApiError } from "@/api/generated/ApiError";
import type { Phrase } from "@/api/generated/Phrase";
import { RpcError } from "@/api/transport";
import i18n from "@/i18n";
import { formatBytes } from "@/lib/format";

// `{{size, bytes}}` in the core's phrases: a size in the current language.
i18n.services.formatter?.add("bytes", (value: unknown) => formatBytes(Number(value)));

/**
 * A text from the core in the current language (docs/I18N.md, ADR-0029):
 * the phrase's key with its parameters, nested phrases translated first.
 * If the catalogue lacks any of its keys (a newer core), the whole text is
 * the core's English instead.
 */
export function phraseText(phrase: Phrase | null | undefined, english: string): string {
  if (!phrase) return english;
  return translate(phrase) ?? english;
}

function translate(phrase: Phrase): string | undefined {
  const params: Record<string, string | number> = {};
  for (const [name, value] of Object.entries(phrase.params)) {
    if (value === undefined) continue;
    if (typeof value === "object") {
      const nested = translate(value);
      if (nested === undefined) return undefined;
      params[name] = nested;
    } else {
      params[name] = value;
    }
  }
  if (!i18n.exists(phrase.key, params)) return undefined;
  const text: string = i18n.t(phrase.key, { ...params, defaultValue: "" });
  return text;
}

function isApiError(value: unknown): value is ApiError {
  return (
    typeof value === "object" &&
    value !== null &&
    typeof (value as ApiError).code === "string" &&
    typeof (value as ApiError).message === "string"
  );
}

/** An error from the API, or anything thrown, in the current language. */
export function errorText(error: unknown): string {
  if (error instanceof RpcError) return phraseText(error.phrase, error.message);
  if (isApiError(error)) return phraseText(error.phrase, error.message);
  return error instanceof Error ? error.message : String(error);
}
