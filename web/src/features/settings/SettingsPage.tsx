import { useQuery } from "@tanstack/react-query";
import { isTauri } from "@tauri-apps/api/core";
import { FolderOpen, Trash2 } from "lucide-react";
import type { ReactNode } from "react";
import { useId, useState } from "react";
import { useTranslation } from "react-i18next";

import { useApi } from "@/api/context";
import type { LoaderProfile } from "@/api/generated/LoaderProfile";
import type { NameStyle } from "@/api/generated/NameStyle";
import type { SourceEntry } from "@/api/generated/SourceEntry";
import { artCache, attribution, clearArtCache } from "@/api/rpc";
import { call } from "@/api/transport";
import { ExternalLink } from "@/components/ExternalLink";
import { LanguageSwitch } from "@/components/LanguageSwitch";
import { PageScroll } from "@/components/PageScroll";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Kbd } from "@/components/ui/kbd";
import { Label } from "@/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { pickFolder } from "@/features/add/desktop";
import { artSourceInfo, takesMirror } from "@/features/art/sources";
import { RedumpPanel } from "@/features/verify/RedumpPanel";
import { formatBytes } from "@/lib/format";
import { useSettings } from "@/settings/context";
import { artSourceIds, nameStyles, profiles } from "@/settings/settings";
import { useTheme } from "@/theme/context";
import { isTheme, themes } from "@/theme/theme";

export function SettingsPage() {
  const { t } = useTranslation();
  const { transport } = useApi();
  const { theme, setTheme } = useTheme();
  const { settings, update } = useSettings();
  const simple = settings.simpleMode;
  const ids = useId();
  const version = useQuery({
    queryKey: ["version"],
    queryFn: () => call(transport, { type: "Version" }, "Version"),
  });
  const about = useQuery({ queryKey: ["attribution"], queryFn: () => attribution(transport) });
  const cache = useQuery({ queryKey: ["artCache"], queryFn: () => artCache(transport) });
  const [clearing, setClearing] = useState(false);

  const versionText =
    version.data?.version ?? (version.isPending ? t("settings.loading") : t("settings.unavailable"));

  const clearCache = async () => {
    setClearing(true);
    try {
      await clearArtCache(transport);
      await cache.refetch();
    } finally {
      setClearing(false);
    }
  };

  return (
    <PageScroll>
      <div className="mx-auto flex max-w-2xl flex-col gap-6">
        <Card>
          <CardHeader>
            <CardTitle>{t("settings.interface")}</CardTitle>
          </CardHeader>
          <CardContent className="flex flex-col gap-4">
            <SettingRow label={t("settings.simpleMode")} hint={t("settings.simpleModeHint")} htmlFor={`${ids}-simple`}>
              <Switch
                id={`${ids}-simple`}
                checked={settings.simpleMode}
                onCheckedChange={(simpleMode) => update({ simpleMode })}
                data-testid="simple-mode"
              />
            </SettingRow>
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
            {!simple && (
              <SettingRow label={t("settings.verify")} hint={t("settings.verifyHint")} htmlFor={`${ids}-verify`}>
                <Switch id={`${ids}-verify`} checked={settings.verify} onCheckedChange={(verify) => update({ verify })} />
              </SettingRow>
            )}
          </CardContent>
        </Card>

        {!simple && (
          <Card>
            <CardHeader>
              <CardTitle>{t("settings.covers")}</CardTitle>
              <CardDescription>{t("settings.coversDescription")}</CardDescription>
            </CardHeader>
            <CardContent className="flex flex-col gap-5">
              <ArtSourceSettingsList art={about.data?.art} />
              <SettingRow
                label={t("settings.artCache")}
                hint={
                  cache.data
                    ? t("settings.artCacheState", { count: Number(cache.data.objects), size: formatBytes(Number(cache.data.bytes)), path: cache.data.path })
                    : t("settings.loading")
                }
              >
                <Button variant="outline" size="sm" onClick={() => void clearCache()} disabled={clearing || !cache.data} data-testid="clear-art-cache">
                  <Trash2 aria-hidden />
                  {t("settings.clearArtCache")}
                </Button>
              </SettingRow>
            </CardContent>
          </Card>
        )}

        <Card>
          <CardHeader>
            <CardTitle>{t("settings.dumps")}</CardTitle>
            <CardDescription>{t("settings.dumpsDescription")}</CardDescription>
          </CardHeader>
          <CardContent className="flex flex-col gap-5">
            <RedumpPanel compact={simple} />
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
            <CardDescription>{t("settings.aboutDescription")}</CardDescription>
          </CardHeader>
          <CardContent className="flex flex-col gap-5" data-testid="about">
            <dl className="grid grid-cols-[max-content_1fr] gap-x-6 gap-y-2 text-sm">
              <dt className="text-muted-foreground">{t("settings.version")}</dt>
              <dd data-testid="app-version">{versionText}</dd>
              <dt className="text-muted-foreground">{t("settings.mode")}</dt>
              <dd>{t(`settings.modes.${transport.kind}`)}</dd>
              <dt className="text-muted-foreground">{t("settings.license")}</dt>
              <dd>{about.data?.license ?? "GPL-3.0-or-later"}</dd>
              <dt className="text-muted-foreground">{t("settings.repository")}</dt>
              <dd>
                <ExternalLink href={about.data?.repository ?? "https://github.com/belomaxorka/yaoplm"} />
              </dd>
              {about.data && (
                <>
                  <dt className="text-muted-foreground">{t("settings.titlesDb")}</dt>
                  <dd>
                    {t("settings.titlesDbState", {
                      count: about.data.titles.entries,
                      built: about.data.titles.built,
                      dat: about.data.titles.libretroVersion,
                    })}
                  </dd>
                </>
              )}
            </dl>
            {about.data && (
              <>
                <section>
                  <h3 className="mb-2 text-sm font-medium">{t("settings.bundledData")}</h3>
                  <SourceList entries={about.data.data} />
                </section>
                <section>
                  <h3 className="mb-2 text-sm font-medium">{t("settings.coverSources")}</h3>
                  <p className="mb-2 text-xs text-muted-foreground">{t("settings.coverSourcesHint")}</p>
                  <SourceList entries={about.data.art} />
                </section>
              </>
            )}
          </CardContent>
        </Card>
      </div>
    </PageScroll>
  );
}

/**
 * The switch and the mirror folder of every picture source, and the
 * ScreenScraper account (ADR-0035). There is no order to set: each type
 * tries the sources in its own order. A source this build cannot ask is
 * left out.
 */
function ArtSourceSettingsList({ art }: { art: readonly SourceEntry[] | undefined }) {
  const { t } = useTranslation();
  const { settings, update } = useSettings();
  const ids = useId();
  return (
    <>
      {artSourceIds.map((id) => {
        const source = settings.artSources[id];
        const set = (patch: Partial<typeof source>) =>
          update({ artSources: { ...settings.artSources, [id]: { ...source, ...patch } } });
        // A source this build cannot ask (ScreenScraper without a key) is
        // not shown at all.
        if (art?.find((entry) => entry.id === id)?.available === false) return null;
        return (
          <div key={id} className="flex flex-col gap-2" data-testid={`art-source-${id}`}>
            <SettingRow label={artSourceInfo[id].name} hint={t(`settings.artSources.${id}`)} htmlFor={`${ids}-src-${id}`}>
              <Switch id={`${ids}-src-${id}`} checked={source.enabled} onCheckedChange={(enabled) => set({ enabled })} />
            </SettingRow>
            {takesMirror(id) ? (
              <div className="flex items-center gap-2">
                <Label htmlFor={`${ids}-mirror-${id}`} className="shrink-0 text-xs text-muted-foreground">
                  {t("settings.mirror")}
                </Label>
                <Input
                  id={`${ids}-mirror-${id}`}
                  value={source.mirror}
                  placeholder={t("settings.mirrorPlaceholder")}
                  onChange={(event) => set({ mirror: event.target.value })}
                  className="font-mono text-xs"
                />
                {isTauri() && (
                  <Button
                    variant="outline"
                    size="icon-sm"
                    aria-label={t("settings.pickMirror")}
                    onClick={() => void pickFolder().then((folder) => folder && set({ mirror: folder }))}
                  >
                    <FolderOpen aria-hidden />
                  </Button>
                )}
              </div>
            ) : (
              <div className="flex flex-col gap-1">
                <div className="flex flex-wrap items-center gap-2">
                  <Label htmlFor={`${ids}-ss-user`} className="shrink-0 text-xs text-muted-foreground">
                    {t("settings.screenscraperUser")}
                  </Label>
                  <Input
                    id={`${ids}-ss-user`}
                    value={settings.screenscraper.user}
                    autoComplete="username"
                    onChange={(event) => update({ screenscraper: { ...settings.screenscraper, user: event.target.value } })}
                    className="w-40 text-xs"
                  />
                  <Label htmlFor={`${ids}-ss-password`} className="shrink-0 text-xs text-muted-foreground">
                    {t("settings.screenscraperPassword")}
                  </Label>
                  <Input
                    id={`${ids}-ss-password`}
                    type="password"
                    value={settings.screenscraper.password}
                    autoComplete="current-password"
                    onChange={(event) =>
                      update({ screenscraper: { ...settings.screenscraper, password: event.target.value } })
                    }
                    className="w-40 text-xs"
                  />
                </div>
                <p className="text-xs text-muted-foreground">{t("settings.screenscraperAccountHint")}</p>
              </div>
            )}
          </div>
        );
      })}
      <p className="text-xs text-muted-foreground">{t("settings.mirrorHint")}</p>
    </>
  );
}

/** Credits of the bundled data and the picture sources (ADR-0007). */
function SourceList({ entries }: { entries: readonly SourceEntry[] }) {
  const { t } = useTranslation();
  return (
    <ul className="flex flex-col gap-3 text-sm">
      {entries
        .filter((entry) => entry.available)
        .map((entry) => (
        <li key={entry.id} className="rounded-md border p-3" data-testid={`source-${entry.id}`}>
          <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
            <span className="font-medium">{entry.name}</span>
            <span className="text-xs text-muted-foreground">
              {entry.license ? (
                entry.licenseUrl ? (
                  <ExternalLink href={entry.licenseUrl}>{entry.license}</ExternalLink>
                ) : (
                  entry.license
                )
              ) : (
                t("settings.noLicense")
              )}
            </span>
          </div>
          <div className="mt-1 text-xs text-muted-foreground">
            {t(`settings.sourceUse.${entry.use}`, { defaultValue: entry.use })}
            {entry.fields && ` · ${t("settings.fields", { fields: entry.fields })}`}
          </div>
          <div className="mt-1 text-xs">{entry.credits}</div>
          <div className="mt-1 text-xs">
            <ExternalLink href={entry.url} />
          </div>
        </li>
      ))}
    </ul>
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
  // The control keeps its place on the right; a long hint wraps beside it.
  return (
    <div className="flex items-center justify-between gap-x-6 gap-y-3 max-sm:flex-wrap">
      <span className="flex min-w-0 flex-col">
        {htmlFor ? (
          <Label htmlFor={htmlFor}>{label}</Label>
        ) : (
          <span className="text-sm font-medium">{label}</span>
        )}
        {hint && <span className="text-xs text-muted-foreground">{hint}</span>}
      </span>
      <div className="shrink-0">{children}</div>
    </div>
  );
}
