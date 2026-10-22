import { Moon, Sun } from "lucide-react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { useTheme } from "@/theme/context";

export function ThemeToggle() {
  const { t } = useTranslation();
  const { resolvedTheme, setTheme } = useTheme();
  const next = resolvedTheme === "dark" ? "light" : "dark";
  const label = next === "dark" ? t("theme.switchToDark") : t("theme.switchToLight");

  return (
    <Button variant="ghost" size="icon" aria-label={label} title={label} onClick={() => setTheme(next)}>
      {resolvedTheme === "dark" ? <Sun aria-hidden /> : <Moon aria-hidden />}
    </Button>
  );
}
