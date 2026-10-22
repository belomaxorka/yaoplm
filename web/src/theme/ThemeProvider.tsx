import { type ReactNode, useCallback, useEffect, useMemo, useState, useSyncExternalStore } from "react";

import { ThemeContext } from "@/theme/context";
import { applyTheme, readTheme, resolveTheme, saveTheme, type Theme } from "@/theme/theme";

const darkQuery = "(prefers-color-scheme: dark)";

function subscribeToSystemTheme(onChange: () => void): () => void {
  const media = window.matchMedia(darkQuery);
  media.addEventListener("change", onChange);
  return () => media.removeEventListener("change", onChange);
}

function systemPrefersDark(): boolean {
  return window.matchMedia(darkQuery).matches;
}

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [theme, setThemeState] = useState<Theme>(readTheme);
  const prefersDark = useSyncExternalStore(subscribeToSystemTheme, systemPrefersDark);
  const resolvedTheme = resolveTheme(theme, prefersDark);

  useEffect(() => applyTheme(resolvedTheme), [resolvedTheme]);

  const setTheme = useCallback((next: Theme) => {
    saveTheme(next);
    setThemeState(next);
  }, []);

  const value = useMemo(() => ({ theme, resolvedTheme, setTheme }), [theme, resolvedTheme, setTheme]);
  return <ThemeContext value={value}>{children}</ThemeContext>;
}
