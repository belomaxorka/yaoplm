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
