import { createContext, use } from "react";

import type { Settings } from "@/settings/settings";

export interface SettingsContextValue {
  settings: Settings;
  update: (patch: Partial<Settings>) => void;
}

export const SettingsContext = createContext<SettingsContextValue | null>(null);

export function useSettings(): SettingsContextValue {
  const value = use(SettingsContext);
  if (!value) throw new Error("useSettings must be used inside <SettingsProvider>");
  return value;
}

/**
 * Simple mode (UI-4, ADR-0034): the one switch every screen reads to hide
 * advanced controls, so what one screen hides never shows on another.
 */
export function useSimpleMode(): boolean {
  return useSettings().settings.simpleMode;
}
