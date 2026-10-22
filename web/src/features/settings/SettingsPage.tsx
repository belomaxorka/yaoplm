import { useQuery } from "@tanstack/react-query";
import type { ReactNode } from "react";
import { useId } from "react";
import { useTranslation } from "react-i18next";

import { useApi } from "@/api/context";
import type { LoaderProfile } from "@/api/generated/LoaderProfile";
import type { NameStyle } from "@/api/generated/NameStyle";
import { call } from "@/api/transport";
import { LanguageSwitch } from "@/components/LanguageSwitch";
import { PageScroll } from "@/components/PageScroll";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Kbd } from "@/components/ui/kbd";
import { Label } from "@/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { useSettings } from "@/settings/context";
import { nameStyles, profiles } from "@/settings/settings";
import { useTheme } from "@/theme/context";
import { isTheme, themes } from "@/theme/theme";

export function SettingsPage() {
  const { t } = useTranslation();
  const { transport } = useApi();
  const { theme, setTheme } = useTheme();
  const { settings, update } = useSettings();
  const ids = useId();
  const version = useQuery({
    queryKey: ["version"],
    queryFn: () => call(transport, { type: "Version" }, "Version"),
  });

  const versionText =
    version.data?.version ?? (version.isPending ? t("settings.loading") : t("settings.unavailable"));

  return (
    <PageScroll>
      <div className="mx-auto flex max-w-2xl flex-col gap-6">
        <Card>
          <CardHeader>
            <CardTitle>{t("settings.library")}</CardTitle>
            <CardDescription>{t("settings.libraryDescription")}</CardDescription>
          </CardHeader>
          <CardContent className="flex flex-col gap-5">
            <SettingRow label={t("settings.profile")} hint={t("settings.profileHint")} htmlFor={`${ids}-profile`}>
              <Select value={settings.profile} onValueChange={(value) => update({ profile: value as LoaderProfile })}>
                <SelectTrigger id={`${ids}-profile`} className="w-48" aria-label={t("settings.profile")}>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {profiles.map((profile) => (
                    <SelectItem key={profile} value={profile}>
                      {t(`settings.profiles.${profile}`)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </SettingRow>
            <div className="flex flex-col gap-2">
              <span className="text-sm font-medium" id={`${ids}-style-label`}>
                {t("settings.nameStyle")}
              </span>
              <RadioGroup
                value={settings.nameStyle}
                onValueChange={(value) => update({ nameStyle: value as NameStyle })}
                aria-labelledby={`${ids}-style-label`}
              >
                {nameStyles.map((style) => (
                  <Label key={style} htmlFor={`${ids}-${style}`} className="items-start font-normal">
                    <RadioGroupItem id={`${ids}-${style}`} value={style} className="mt-0.5" />
                    <span>
                      {t(`settings.nameStyles.${style}`)}
                      <span className="block text-xs text-muted-foreground">{t(`settings.nameStyleHints.${style}`)}</span>
                    </span>
                  </Label>
                ))}
              </RadioGroup>
            </div>
            <SettingRow label={t("settings.verify")} hint={t("settings.verifyHint")} htmlFor={`${ids}-verify`}>
              <Switch id={`${ids}-verify`} checked={settings.verify} onCheckedChange={(verify) => update({ verify })} />
            </SettingRow>
          </CardContent>
        </Card>

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
            <CardTitle>{t("settings.keyboard")}</CardTitle>
          </CardHeader>
          <CardContent>
            <dl className="grid grid-cols-[max-content_1fr] gap-x-6 gap-y-2 text-sm">
              <dt>
                <Kbd>Tab</Kbd>
              </dt>
              <dd className="text-muted-foreground">{t("settings.keys.tab")}</dd>
              <dt>
                <Kbd>/</Kbd>
              </dt>
              <dd className="text-muted-foreground">{t("settings.keys.search")}</dd>
              <dt>
                <Kbd>←</Kbd> <Kbd>→</Kbd> <Kbd>↑</Kbd> <Kbd>↓</Kbd>
              </dt>
              <dd className="text-muted-foreground">{t("settings.keys.arrows")}</dd>
              <dt>
                <Kbd>Enter</Kbd>
              </dt>
              <dd className="text-muted-foreground">{t("settings.keys.enter")}</dd>
              <dt>
                <Kbd>Esc</Kbd>
              </dt>
              <dd className="text-muted-foreground">{t("settings.keys.escape")}</dd>
            </dl>
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
    </PageScroll>
  );
}

function SettingRow({
  label,
  hint,
  htmlFor,
  children,
}: {
  label: string;
  hint?: string;
  htmlFor?: string;
  children: ReactNode;
}) {
  return (
    <div className="flex flex-wrap items-center justify-between gap-3">
      <span className="flex flex-col">
        {htmlFor ? (
          <Label htmlFor={htmlFor}>{label}</Label>
        ) : (
          <span className="text-sm font-medium">{label}</span>
        )}
        {hint && <span className="text-xs text-muted-foreground">{hint}</span>}
      </span>
      {children}
    </div>
  );
}
