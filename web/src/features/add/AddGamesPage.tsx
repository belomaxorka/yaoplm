import { isTauri } from "@tauri-apps/api/core";
import { cn } from "cn";
import { ArrowLeft, FilePlus2, FolderOpen, FolderSearch, Loader2, Trash2, Upload } from "lucide-react";
import { useCallback, useEffect, useId, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router";

import type { ImportOverride } from "@/api/generated/ImportOverride";
import type { Media } from "@/api/generated/Media";
import type { PlanTransaction } from "@/api/generated/PlanTransaction";
import type { Request } from "@/api/generated/Request";
import { EmptyState } from "@/components/EmptyState";
import { PageScroll } from "@/components/PageScroll";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { listenForDrops, pickFiles, pickFolder } from "@/features/add/desktop";
import { CoverSearchPanel } from "@/features/add/CoverSearchPanel";
import { FolderBrowserDialog } from "@/features/add/FolderBrowserDialog";
import { useCoverSearch } from "@/features/add/useCoverSearch";
import { enabledSources } from "@/features/art/sources";
import { PlanDetails } from "@/features/plan/PlanDetails";
import { usePlan } from "@/features/plan/usePlan";
import { baseName } from "@/lib/format";
import { phraseText } from "@/lib/phrase";
import { useSettings } from "@/settings/context";

interface Override {
  title?: string;
  media?: Media;
}

/**
 * Add games (IMP-1, IMP-3, IMP-10, IMP-11): pick sources, see the plan the
 * core builds for them, adjust a title or CD/DVD per game, confirm. Moving
 * instead of copying is an explicit switch and the plan says so.
 */
export function AddGamesPage() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { settings, update } = useSettings();
  const root = settings.root;
  const [sources, setSources] = useState<string[]>([]);
  const [move, setMove] = useState(false);
  const [alwaysUl, setAlwaysUl] = useState(false);
  const [compress, setCompress] = useState(false);
  const [overrides, setOverrides] = useState<Record<string, Override>>({});
  const [browserOpen, setBrowserOpen] = useState(false);
  const [hovering, setHovering] = useState(false);
  const ids = useId();
  const flow = usePlan({ onExecuted: () => void navigate("/jobs") });
  const desktop = isTauri();
  const zsoReadable = settings.profile !== "Opl110";
  const coverSources = enabledSources(settings).length > 0;
  const coversWanted = settings.coversOnAdd && coverSources;
  const covers = useCoverSearch(root, flow.plan, coversWanted);
  const coverSet = coversWanted && covers.state?.status === "done" ? (covers.state.result?.id ?? null) : null;
  const searching = covers.state?.status === "running";

  const addSources = useCallback((paths: string[]) => {
    setSources((current) => [...current, ...paths.filter((p) => !current.includes(p))]);
  }, []);

  useEffect(() => listenForDrops(addSources, setHovering), [addSources]);

  const request = useMemo((): Request | null => {
    if (!root || sources.length === 0) return null;
    const list: ImportOverride[] = Object.entries(overrides)
      .filter(([, o]) => o.title !== undefined || o.media !== undefined)
      .map(([source, o]) => ({ source, ...(o.title !== undefined ? { title: o.title } : {}), ...(o.media ? { media: o.media } : {}) }));
    return {
      type: "PlanImport",
      root,
      sources,
      profile: settings.profile,
      nameStyle: settings.nameStyle,
      transfer: move ? "move" : "copy",
      layout: alwaysUl ? "ul" : "auto",
      ...(compress && zsoReadable ? { compress: true } : {}),
      ...(list.length > 0 ? { overrides: list } : {}),
      verify: settings.checkOnAdd,
      ...(coverSet ? { covers: coverSet } : {}),
    };
  }, [root, sources, overrides, settings.profile, settings.nameStyle, move, alwaysUl, compress, zsoReadable, settings.checkOnAdd, coverSet]);

  // Re-plan a moment after the last edit; the plan is the only truth about names and media.
  const requestJson = useMemo(() => (request ? JSON.stringify(request) : null), [request]);
  const [plannedJson, setPlannedJson] = useState<string | null>(null);
  useEffect(() => {
    if (!request || plannedJson === null || requestJson === plannedJson) return;
    const timer = setTimeout(() => {
      setPlannedJson(requestJson);
      void flow.request(request, { keep: true });
    }, 400);
    return () => clearTimeout(timer);
    // flow.request is stable; the plan itself must not retrigger planning.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [requestJson, plannedJson]);
  // The plan on screen predates an edit, or the covers just found: it is not
  // the one to run.
  const stale = plannedJson !== null && requestJson !== plannedJson;

  const preview = () => {
    if (!request) return;
    setPlannedJson(requestJson);
    void flow.request(request);
  };

  const cancel = () => {
    setPlannedJson(null);
    flow.discard();
  };

  if (!root) {
    return (
      <PageScroll className="flex flex-col items-center">
        <EmptyState icon={FolderOpen} title={t("library.emptyTitle")} description={t("library.emptyDescription")} />
        <Button onClick={() => void navigate("/devices")}>{t("library.chooseDevice")}</Button>
      </PageScroll>
    );
  }

  const sourceOf = (transaction: PlanTransaction): string | null =>
    transaction.steps.find((step) => (step.op === "writeFile" || step.op === "writeZso") && step.source)?.source ?? null;

  const renderOverride = (transaction: PlanTransaction) => {
    const source = sourceOf(transaction);
    // A cover written with its game follows the game's title and media.
    if (!source || !transaction.game || transaction.titlePhrase?.key === "titles.coverFor") return null;
    const current = overrides[source] ?? {};
    return (
      <div className="mt-2 grid gap-2 sm:grid-cols-[1fr_auto]">
        <div className="flex flex-col gap-1">
          <Label htmlFor={`${ids}-title-${source}`} className="text-xs text-muted-foreground">
            {t("add.title")}
          </Label>
          <Input
            id={`${ids}-title-${source}`}
            value={current.title ?? transaction.game.title}
            maxLength={transaction.game.format === "UL" ? 32 : 160}
            onChange={(event) => setOverrides((o) => ({ ...o, [source]: { ...o[source], title: event.target.value } }))}
          />
        </div>
        <div className="flex flex-col gap-1">
          <span className="text-xs text-muted-foreground">{t("game.media")}</span>
          <ToggleGroup
            type="single"
            variant="outline"
            size="sm"
            spacing={0}
            value={current.media ?? transaction.game.media}
            onValueChange={(value) => value && setOverrides((o) => ({ ...o, [source]: { ...o[source], media: value as Media } }))}
            aria-label={t("game.media")}
          >
            <ToggleGroupItem value="CD">CD</ToggleGroupItem>
            <ToggleGroupItem value="DVD">DVD</ToggleGroupItem>
          </ToggleGroup>
        </div>
      </div>
    );
  };

  return (
    <PageScroll>
      <div className="mx-auto flex max-w-4xl flex-col gap-6">
        <div className="flex items-center gap-2">
          <Button variant="ghost" size="sm" onClick={() => void navigate("/library")}>
            <ArrowLeft aria-hidden />
            {t("library.title")}
          </Button>
          <span className="truncate font-mono text-xs text-muted-foreground" title={root}>
            {root}
          </span>
        </div>

        <section
          className={cn(
            "rounded-lg border-2 border-dashed p-6 transition-colors",
            hovering ? "border-primary bg-primary/5" : "border-border",
          )}
          data-testid="drop-zone"
        >
          <div className="flex flex-col items-center gap-3 text-center">
            <Upload className="size-8 text-muted-foreground" aria-hidden />
            <p className="text-sm text-muted-foreground">
              {desktop ? t("add.dropHint") : t("add.serverHint")}
            </p>
            <div className="flex flex-wrap justify-center gap-2">
              {desktop ? (
                <>
                  <Button onClick={() => void pickFiles().then(addSources)}>
                    <FilePlus2 aria-hidden />
                    {t("add.pickFiles")}
                  </Button>
                  <Button variant="outline" onClick={() => void pickFolder().then((f) => f && addSources([f]))}>
                    <FolderOpen aria-hidden />
                    {t("add.pickFolder")}
                  </Button>
                </>
              ) : (
                <Button onClick={() => setBrowserOpen(true)} data-testid="browse-server">
                  <FolderSearch aria-hidden />
                  {t("add.browse")}
                </Button>
              )}
            </div>
          </div>
        </section>

        {sources.length > 0 && (
          <section className="rounded-lg border bg-card">
            <h3 className="border-b px-4 py-2 text-sm font-medium">{t("add.sources", { count: sources.length })}</h3>
            <ul data-testid="source-list">
              {sources.map((source) => (
                <li key={source} className="flex items-center gap-2 border-b px-4 py-1.5 text-sm last:border-0">
                  <span className="min-w-0 flex-1">
                    <span className="block truncate">{baseName(source)}</span>
                    <span className="block truncate font-mono text-xs text-muted-foreground" title={source}>
                      {source}
                    </span>
                  </span>
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    aria-label={t("add.removeSource", { name: baseName(source) })}
                    onClick={() => setSources((s) => s.filter((p) => p !== source))}
                  >
                    <Trash2 aria-hidden />
                  </Button>
                </li>
              ))}
            </ul>
          </section>
        )}

        <section className="flex flex-col gap-3 rounded-lg border bg-card p-4">
          <h3 className="text-sm font-medium">{t("add.options")}</h3>
          <Label htmlFor={`${ids}-move`} className="items-start font-normal">
            <Switch id={`${ids}-move`} checked={move} onCheckedChange={setMove} className="mt-0.5" />
            <span>
              {t("add.move")}
              <span className="block text-xs text-muted-foreground">{t("add.moveHint")}</span>
            </span>
          </Label>
          <Label htmlFor={`${ids}-ul`} className="items-start font-normal">
            <Switch id={`${ids}-ul`} checked={alwaysUl} onCheckedChange={setAlwaysUl} className="mt-0.5" />
            <span>
              {t("add.alwaysUl")}
              <span className="block text-xs text-muted-foreground">{t("add.alwaysUlHint")}</span>
            </span>
          </Label>
          <Label htmlFor={`${ids}-zso`} className="items-start font-normal">
            <Switch
              id={`${ids}-zso`}
              checked={compress && zsoReadable}
              disabled={!zsoReadable}
              onCheckedChange={setCompress}
              className="mt-0.5"
              data-testid="compress-zso"
            />
            <span>
              {t("add.compress")}
              <span className="block text-xs text-muted-foreground">
                {zsoReadable ? t("add.compressHint") : t("add.compressNotForProfile")}
              </span>
            </span>
          </Label>
          <Label htmlFor={`${ids}-covers`} className="items-start font-normal">
            <Switch
              id={`${ids}-covers`}
              checked={coversWanted}
              disabled={!coverSources}
              onCheckedChange={(coversOnAdd) => update({ coversOnAdd })}
              className="mt-0.5"
              data-testid="covers-on-add"
            />
            <span>
              {t("settings.coversOnAdd")}
              <span className="block text-xs text-muted-foreground">
                {coverSources ? t("settings.coversOnAddHint") : t("art.noSources")}
              </span>
            </span>
          </Label>
          <Label htmlFor={`${ids}-check`} className="items-start font-normal">
            <Switch
              id={`${ids}-check`}
              checked={settings.checkOnAdd}
              onCheckedChange={(checkOnAdd) => update({ checkOnAdd })}
              className="mt-0.5"
              data-testid="check-on-add"
            />
            <span>
              {t("settings.checkOnAdd")}
              <span className="block text-xs text-muted-foreground">{t("settings.checkOnAddHint")}</span>
            </span>
          </Label>
          <p className="text-xs text-muted-foreground">
            {t("add.usingSettings", {
              profile: t(`settings.profiles.${settings.profile}`),
              style: t(`settings.nameStyles.${settings.nameStyle}`),
            })}
          </p>
        </section>

        {!flow.plan && !flow.pending && (
          <div className="flex justify-end">
            <Button size="lg" disabled={!request} onClick={preview} data-testid="preview-plan">
              {t("add.preview")}
            </Button>
          </div>
        )}

        {flow.pending && !flow.plan && (
          <p className="flex items-center gap-2 text-sm text-muted-foreground" role="status">
            <Loader2 className="size-4 animate-spin" aria-hidden />
            {t("plan.building")}
          </p>
        )}
        {flow.error && (
          <p className="rounded-md border border-destructive/40 bg-destructive/10 p-3 text-sm text-destructive" role="alert">
            {flow.error}
          </p>
        )}
        {flow.plan && (
          <section className="flex flex-col gap-4 rounded-lg border bg-card p-4" data-testid="import-plan">
            <div>
              <h3 className="text-base font-semibold">{phraseText(flow.plan.titlePhrase, flow.plan.title)}</h3>
              <p className="text-xs text-muted-foreground">{t("add.planHint")}</p>
            </div>
            {covers.state && <CoverSearchPanel search={covers.state} onStop={covers.stop} />}
            <PlanDetails plan={flow.plan} renderTransaction={renderOverride} />
            <div className="flex flex-wrap items-center justify-end gap-2 border-t pt-4">
              {(flow.pending || stale) && (
                <span className="mr-auto flex items-center gap-2 text-sm text-muted-foreground" role="status">
                  <Loader2 className="size-4 animate-spin" aria-hidden />
                  {t("plan.updating")}
                </span>
              )}
              <Button variant="outline" onClick={cancel} disabled={flow.executing}>
                {t("common.cancel")}
              </Button>
              <Button
                variant={flow.plan.deletesUserData ? "destructive" : "default"}
                disabled={!flow.plan.executable || flow.executing || flow.pending || stale || searching}
                onClick={() => void flow.execute(flow.plan?.deletesUserData ?? false)}
                data-testid="plan-confirm"
              >
                {flow.executing && <Loader2 className="animate-spin" aria-hidden />}
                {flow.plan.deletesUserData ? t("add.confirmMove") : t("add.confirm")}
              </Button>
            </div>
          </section>
        )}
      </div>
      <FolderBrowserDialog open={browserOpen} onOpenChange={setBrowserOpen} onPick={addSources} />
    </PageScroll>
  );
}
