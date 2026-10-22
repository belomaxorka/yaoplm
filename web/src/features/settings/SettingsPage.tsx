import { useQuery } from "@tanstack/react-query";
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";

import { useApi } from "@/api/context";
import { call } from "@/api/transport";
import { LanguageSwitch } from "@/components/LanguageSwitch";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { useTheme } from "@/theme/context";
import { isTheme, themes } from "@/theme/theme";

export function SettingsPage() {
  const { t } = useTranslation();
  const { transport } = useApi();
  const { theme, setTheme } = useTheme();
  const version = useQuery({
    queryKey: ["version"],
    queryFn: () => call(transport, { type: "Version" }, "Version"),
  });

  const versionText =
    version.data?.version ?? (version.isPending ? t("settings.loading") : t("settings.unavailable"));

  return (
    <div className="mx-auto flex max-w-2xl flex-col gap-6">
      <Card>
        <CardHeader>
          <CardTitle>{t("settings.appearance")}</CardTitle>
        </CardHeader>
        <CardContent className="flex flex-col gap-4">
          <SettingRow label={t("settings.language")}>
            <LanguageSwitch showNames />
          </SettingRow>
          <SettingRow label={t("settings.theme")}>
            <ToggleGroup
              type="single"
              variant="outline"
              size="sm"
              spacing={0}
              aria-label={t("settings.theme")}
              value={theme}
              onValueChange={(value) => {
                if (isTheme(value)) setTheme(value);
              }}
            >
              {themes.map((option) => (
                <ToggleGroupItem key={option} value={option}>
                  {t(`settings.themes.${option}`)}
                </ToggleGroupItem>
              ))}
            </ToggleGroup>
          </SettingRow>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>{t("settings.about")}</CardTitle>
        </CardHeader>
        <CardContent>
          <dl className="grid grid-cols-[max-content_1fr] gap-x-6 gap-y-2 text-sm">
            <dt className="text-muted-foreground">{t("settings.version")}</dt>
            <dd data-testid="app-version">{versionText}</dd>
            <dt className="text-muted-foreground">{t("settings.mode")}</dt>
            <dd>{t(`settings.modes.${transport.kind}`)}</dd>
            <dt className="text-muted-foreground">{t("settings.license")}</dt>
            <dd>GPL-3.0-or-later</dd>
          </dl>
        </CardContent>
      </Card>
    </div>
  );
}

function SettingRow({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex flex-wrap items-center justify-between gap-3">
      <span className="text-sm">{label}</span>
      {children}
    </div>
  );
}
