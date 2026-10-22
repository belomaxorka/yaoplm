import { readStorage, writeStorage } from "@/lib/storage";

export const themes = ["light", "dark", "system"] as const;
export type Theme = (typeof themes)[number];
export type ResolvedTheme = Exclude<Theme, "system">;

// Keep in sync with public/theme-init.js, which applies it before first paint.
const STORAGE_KEY = "yaoplm.theme";

export function isTheme(value: unknown): value is Theme {
  return themes.includes(value as Theme);
}

export function readTheme(): Theme {
  const saved = readStorage(STORAGE_KEY);
  return isTheme(saved) ? saved : "system";
}

export function saveTheme(theme: Theme): void {
  writeStorage(STORAGE_KEY, theme);
}

export function resolveTheme(theme: Theme, prefersDark: boolean): ResolvedTheme {
  if (theme === "system") return prefersDark ? "dark" : "light";
  return theme;
}

export function applyTheme(theme: ResolvedTheme): void {
  const root = document.documentElement;
  root.classList.toggle("dark", theme === "dark");
  root.style.colorScheme = theme;
}
