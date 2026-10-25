import { useQuery, useQueryClient } from "@tanstack/react-query";
import { CheckCircle2, Download, Loader2 } from "lucide-react";
import { useId, useState } from "react";
import { useTranslation } from "react-i18next";

import { useApi } from "@/api/context";
import type { DeviceInfo } from "@/api/generated/DeviceInfo";
import type { InstalledLoader } from "@/api/generated/InstalledLoader";
import type { JobId } from "@/api/generated/JobId";
import type { LoaderBootHint } from "@/api/generated/LoaderBootHint";
import type { LoaderBuild } from "@/api/generated/LoaderBuild";
import type { LoaderOffer } from "@/api/generated/LoaderOffer";
import type { LoaderTarget } from "@/api/generated/LoaderTarget";
import type { Request } from "@/api/generated/Request";
import {
  downloadLoader,
  loaderBootHints,
  loaderCatalog,
  loaderImpact,
  loaderRelease,
  loaderReleases,
  waitForJob,
} from "@/api/rpc";
import { ExternalLink } from "@/components/ExternalLink";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { PlanDialog } from "@/features/plan/PlanDialog";
import { usePlan } from "@/features/plan/usePlan";
import { formatBytes, formatDate } from "@/lib/format";
import { isLanguage, type Language } from "@/i18n";
import { errorText } from "@/lib/phrase";
import { useSettings, useSimpleMode } from "@/settings/context";

import { ReplaceImpact } from "./ReplaceImpact";
import { versionLabel } from "./versions";

type InstallRequest = Extract<Request, { type: "PlanInstallLoader" }>;
type ReplaceRequest = Extract<Request, { type: "PlanReplaceLoader" }>;

/**
 * How the console starts OPL: a catalogue preset, both stock paths
 * ("I don't know"), the user's own path, or OPL on the memory card, which
 * needs only the folders.
 */
interface Method {
  id: string;
  presets: string[];
  paths: string[];
}

/** OPL's language pack names (`lang_Russian.lng`) by interface language; English is OPL's own. */
const oplLanguages: Partial<Record<Language, string>> = { ru: "Russian" };

type MethodLabel = "fmcb" | "ps2bbl" | "unknown" | "app" | "appMx4sio" | "card" | "custom";
const labelled = new Set<string>(["fmcb", "ps2bbl", "unknown", "app", "card", "custom"]);

/** The label of a method; a preset a newer catalogue added goes by its path. */
function methodLabel(id: string, target: LoaderTarget): MethodLabel | null {
  if (!labelled.has(id)) return null;
  return id === "app" && target === "mx4sio" ? "appMx4sio" : (id as MethodLabel);
}

function methodsFor(build: LoaderBuild | undefined, target: LoaderTarget, advanced: boolean): Method[] {
  const presets = (build?.presets ?? []).filter((preset) => preset.devices.includes(target));
  const methods: Method[] = presets.map((preset) => ({ id: preset.id, presets: [preset.id], paths: [preset.path] }));
  const fmcb = presets.find((preset) => preset.id === "fmcb");
  const ps2bbl = presets.find((preset) => preset.id === "ps2bbl");
  if (fmcb && ps2bbl) {
    const after = methods.findIndex((method) => method.id === "ps2bbl");
    methods.splice(after + 1, 0, { id: "unknown", presets: [fmcb.id, ps2bbl.id], paths: [fmcb.path, ps2bbl.path] });
  }
  methods.push({ id: "card", presets: [], paths: [] });
  if (advanced) methods.push({ id: "custom", presets: [], paths: [] });
  return methods;
}

/** What the user chose; `null` follows the device and the catalogue. */
interface Form {
  target: LoaderTarget | null;
  method: string | null;
  build: string | null;
  path: string;
  language: boolean;
  starter: boolean;
  /** Another release of the channel (LDR-6); `null`: the channel's own. */
  tag: string | null;
  /** ZSO images a replacement leaves as they are. */
  keepZso: string[];
  reencodeArt: boolean;
  /** Move the replaced loader into the backup when the new one goes elsewhere. */
  retire: boolean;
}

const freshForm = (): Form => ({
  target: null,
  method: null,
  build: null,
  path: "",
  language: false,
  starter: false,
  tag: null,
  keepZso: [],
  reencodeArt: true,
  retire: false,
});

/**
 * "Install loader…" (LDR-3, BOOT-1, ADR-0044): where the device will
 * work, how the console starts OPL and which build. The release's version,
 * licence and changes show before anything is downloaded; the download is
 * held to the hash shown, the install is a plan, and after it the lines
 * for the launcher show. Memory card configs are never written.
 *
 * With `replace` it is "Replace…" (LDR-7, SPEC §5.10.3): the installed
 * build is marked, the place defaults to where it is, and "What changes
 * for the games" lists what the new build's rules need converted,
 * re-encoded or left as advice; the plan runs those first and the loader
 * last.
 */
export function InstallLoaderDialog({
  device,
  open,
  onOpenChange,
  initial,
  replace,
}: {
  device: DeviceInfo;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** "Reinstall…": the build and preset already on the device. */
  initial?: { build: string | null; preset: string | null };
  /** "Replace…": the loader to replace. */
  replace?: InstalledLoader;
}) {
  const { t, i18n } = useTranslation();
  const { transport } = useApi();
  const { settings } = useSettings();
  const simple = useSimpleMode();
  const queryClient = useQueryClient();
  const ids = useId();
  const start = (): Form =>
    replace
      ? { ...freshForm(), method: replace.preset ?? "custom", path: replace.preset ? "" : replace.path }
      : { ...freshForm(), build: initial?.build ?? null, method: initial?.preset ?? null };
  const [form, setForm] = useState<Form>(start);
  const [busy, setBusy] = useState<"download" | "finish" | null>(null);
  const [error, setError] = useState<string | null>(null);
  // What was planned last: a re-plan (replace the foreign file) starts from it.
  const [planned, setPlanned] = useState<InstallRequest | ReplaceRequest | "card" | null>(null);
  const [showVersions, setShowVersions] = useState(false);
  const [done, setDone] = useState<{ hints: LoaderBootHint[]; card: boolean } | null>(null);

  const close = () => {
    setForm(start());
    setShowVersions(false);
    setError(null);
    setPlanned(null);
    setDone(null);
    onOpenChange(false);
  };
  const flow = usePlan({
    onExecuted: (job) => void finish(job),
    onDiscarded: () => setPlanned(null),
  });

  const catalog = useQuery({
    queryKey: ["loaderCatalog"],
    queryFn: () => loaderCatalog(transport),
    enabled: open,
    staleTime: Infinity,
  });
  const target: LoaderTarget = form.target ?? (device.bus === "sd" ? "mx4sio" : "usb");
  const builds = (catalog.data?.builds ?? []).filter((b) => b.devices.includes(target) && b.presets.length > 0);
  // Replacing starts on another build, the recommended one when it is.
  const other = (b: LoaderBuild) => !replace || b.id !== replace.build;
  const build =
    builds.find((b) => b.id === form.build) ??
    builds.find((b) => b.recommended && other(b)) ??
    builds.find(other) ??
    builds[0];
  // A replacement keeps the loader's place: a path of its own needs "custom".
  const methods = methodsFor(build, target, !simple || (replace !== undefined && replace.preset === null)).filter(
    (m) => !replace || m.id !== "card",
  );
  const fallback = build?.defaultPresets.find((d) => d.device === target)?.preset;
  const method =
    methods.find((m) => m.id === form.method) ?? methods.find((m) => m.id === fallback) ?? methods[0];
  const card = method?.id === "card";
  const mirror = settings.loaderMirror.trim();

  const tag = form.tag;
  const offer = useQuery({
    queryKey: ["loaderRelease", build?.id, mirror, tag],
    queryFn: ({ signal }) => loaderRelease(transport, build?.id ?? "", mirror, signal, tag),
    enabled: open && build !== undefined && !card,
    staleTime: 10 * 60_000,
    retry: false,
  });
  const releases = useQuery({
    queryKey: ["loaderReleases", build?.id, mirror],
    queryFn: ({ signal }) => loaderReleases(transport, build?.id ?? "", mirror, signal),
    enabled: open && showVersions && build !== undefined,
    staleTime: 10 * 60_000,
    retry: false,
  });
  const replacing = replace !== undefined && build !== undefined && build.id !== replace.build;
  const profile = device.loader.profile;
  const impact = useQuery({
    queryKey: ["loaderImpact", device.root, build?.id, profile],
    queryFn: ({ signal }) => loaderImpact(transport, { root: device.root, build: build?.id ?? "", profile }, signal),
    enabled: open && replacing,
    staleTime: 60_000,
    retry: false,
  });
  // The new build's paths, to tell whether the replaced loader stays behind.
  const newPaths =
    method?.id === "custom" ? [form.path.trim().replace(/\\/g, "/").replace(/^\/+/, "")] : (method?.paths ?? []);
  const leftBehind =
    replace !== undefined && !newPaths.some((p) => p.toLowerCase() === replace.path.toLowerCase());
  const uiLanguage: Language = isLanguage(i18n.resolvedLanguage) ? i18n.resolvedLanguage : "en";
  const language = oplLanguages[uiLanguage];
  const offersLanguage = language !== undefined && offer.data?.languages != null;
  const wantsLanguage = form.language && offersLanguage;
  const starterOffered = !simple && target === "usb";

  const set = (patch: Partial<Form>) => setForm((f) => ({ ...f, ...patch }));

  async function finish(job: JobId) {
    setBusy("finish");
    try {
      const info = await waitForJob(transport, job);
      void queryClient.invalidateQueries({ queryKey: ["devices"] });
      void queryClient.invalidateQueries({ queryKey: ["library", device.root] });
      void queryClient.invalidateQueries({ queryKey: ["loaderRelease"] });
      if (info.status !== "finished") {
        setError(info.error ? errorText(info.error) : t("loaderInstall.notFinished"));
        return;
      }
      if (planned === "card" || planned === null) {
        setDone({ hints: [], card: true });
        return;
      }
      const hints = await loaderBootHints(transport, {
        build: planned.build,
        presets: planned.presets,
        ...(planned.path !== undefined ? { path: planned.path } : {}),
        device: planned.device,
      });
      setDone({ hints, card: false });
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(null);
    }
  }

  const install = async () => {
    setError(null);
    if (card) {
      setPlanned("card");
      void flow.request({ type: "PlanInit", root: device.root });
      return;
    }
    if (!offer.data || !build || !method) return;
    setBusy("download");
    try {
      const file = await downloadLoader(transport, offer.data, wantsLanguage, mirror);
      const install: InstallRequest = {
        type: "PlanInstallLoader",
        root: device.root,
        build: build.id,
        sha256: file.sha256,
        presets: method.presets,
        ...(method.id === "custom" ? { path: form.path.trim() } : {}),
        device: target,
        ...(wantsLanguage ? { language } : {}),
        ...(starterOffered && form.starter ? { starterSettings: true } : {}),
      };
      const request: InstallRequest | ReplaceRequest = replace
        ? {
            ...install,
            type: "PlanReplaceLoader",
            profile,
            convert: (impact.data?.zso ?? [])
              .map((g) => g.path ?? "")
              .filter((p) => p !== "" && !form.keepZso.includes(p)),
            reencodeArt: form.reencodeArt,
            retire: leftBehind && form.retire ? [replace.path] : [],
          }
        : install;
      setPlanned(request);
      void flow.request(request);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(null);
    }
  };

  // The plan stopped at a file that is not a known OPL: replacing it is the user's call.
  const foreign =
    planned !== null &&
    planned !== "card" &&
    (planned.replaceForeign === true || (flow.plan?.notices.some((n) => n.code === "loaderForeignFile") ?? false));
  const replaceChoice = foreign ? (
    <Label htmlFor={`${ids}-replace`} className="font-normal">
      <Checkbox
        id={`${ids}-replace`}
        checked={planned.replaceForeign === true}
        onCheckedChange={(on) => {
          const next = { ...planned, replaceForeign: on === true };
          setPlanned(next);
          void flow.request(next, { keep: true });
        }}
        data-testid="loader-replace-foreign"
      />
      {t("loaderInstall.replaceForeign")}
    </Label>
  ) : undefined;

  const ready =
    method !== undefined &&
    busy === null &&
    (card || (offer.data !== undefined && (method.id !== "custom" || form.path.trim() !== ""))) &&
    (!replacing || (impact.data !== undefined && !impact.data.blocked));

  return (
    <>
      <Dialog
        open={open && flow.plan === null && !flow.pending && flow.error === null}
        onOpenChange={(next) => {
          if (next) onOpenChange(true);
          else if (busy === null) close();
        }}
      >
        <DialogContent className="flex max-h-[85dvh] flex-col sm:max-w-2xl" data-testid="loader-install-dialog">
          <DialogHeader>
            <DialogTitle>
              {done
                ? done.card
                  ? t("loaderInstall.foldersReady")
                  : replace
                    ? t("loaderReplace.replaced")
                    : t("loaderInstall.installed")
                : replace
                  ? t("loaderReplace.title")
                  : t("loaderInstall.title")}
            </DialogTitle>
            <DialogDescription>
              {done
                ? t("loaderInstall.howToStart")
                : replace
                  ? t("loaderReplace.description", {
                      build: builds.find((b) => b.id === replace.build)?.name ?? replace.family,
                      version: replace.version ? versionLabel(replace.version) : t("loaderUpdate.versionUnknown"),
                      path: replace.path,
                      root: device.root,
                    })
                  : t("loaderInstall.description", { root: device.root })}
            </DialogDescription>
          </DialogHeader>
          <div className="relative min-h-0 flex-1 overflow-auto pr-1">
            {done ? (
              <BootHints hints={done.hints} card={done.card} target={planned === "card" ? target : (planned?.device ?? target)} build={build} />
            ) : (
              <div className="flex flex-col gap-5">
                <fieldset>
                  <legend className="mb-2 text-sm font-medium">{t("loaderInstall.targetLegend")}</legend>
                  <RadioGroup
                    value={target}
                    onValueChange={(value) => set({ target: value as LoaderTarget, method: null })}
                    className="grid gap-2 sm:grid-cols-2"
                  >
                    {(["usb", "mx4sio"] as const).map((value) => (
                      <Label key={value} htmlFor={`${ids}-target-${value}`} className="items-start font-normal">
                        <RadioGroupItem
                          id={`${ids}-target-${value}`}
                          value={value}
                          className="mt-0.5"
                          data-testid={`loader-target-${value}`}
                        />
                        <span>
                          {t(`loaderInstall.targets.${value}`)}
                          <span className="block text-xs text-muted-foreground">{t(`loaderInstall.targetHints.${value}`)}</span>
                        </span>
                      </Label>
                    ))}
                  </RadioGroup>
                </fieldset>

                {catalog.isPending && (
                  <p className="flex items-center gap-2 text-sm text-muted-foreground" role="status">
                    <Loader2 className="size-4 animate-spin" aria-hidden />
                    {t("loaderInstall.loadingCatalog")}
                  </p>
                )}
                {catalog.error && (
                  <p className="rounded-md border border-destructive/40 bg-destructive/10 p-3 text-sm text-destructive" role="alert">
                    {errorText(catalog.error)}
                  </p>
                )}

                {build && (
                  <fieldset>
                    <legend className="mb-2 text-sm font-medium">{t("loaderInstall.methodLegend")}</legend>
                    <RadioGroup value={method?.id ?? ""} onValueChange={(value) => set({ method: value })}>
                      {methods.map((m) => {
                        const label = methodLabel(m.id, target);
                        return (
                          <Label key={m.id} htmlFor={`${ids}-method-${m.id}`} className="items-start font-normal">
                            <RadioGroupItem
                              id={`${ids}-method-${m.id}`}
                              value={m.id}
                              className="mt-0.5"
                              data-testid={`loader-method-${m.id}`}
                            />
                            <span className="min-w-0">
                              {label ? t(`loaderInstall.methods.${label}`) : m.paths.join(", ")}
                              {label && (
                                <span className="block text-xs text-muted-foreground wrap-anywhere">
                                  {t(`loaderInstall.methods.${label}Hint`, { path: m.paths[0] ?? "", paths: m.paths.join(", ") })}
                                </span>
                              )}
                            </span>
                          </Label>
                        );
                      })}
                    </RadioGroup>
                    {method?.id === "custom" && (
                      <div className="mt-2 flex items-center gap-2 pl-6">
                        <Label htmlFor={`${ids}-path`} className="shrink-0 text-xs text-muted-foreground">
                          {t("loaderInstall.path")}
                        </Label>
                        <Input
                          id={`${ids}-path`}
                          value={form.path}
                          placeholder="APPS/OPL/OPNPS2LD.ELF"
                          onChange={(event) => set({ path: event.target.value })}
                          className="font-mono text-xs"
                          data-testid="loader-path"
                        />
                      </div>
                    )}
                  </fieldset>
                )}

                {build && !card && (
                  <fieldset>
                    <legend className="mb-2 text-sm font-medium">{t("loaderInstall.buildLegend")}</legend>
                    <RadioGroup
                      value={build.id}
                      onValueChange={(value) => set({ build: value, tag: null, ...(replace ? {} : { method: null }) })}
                    >
                      {builds.map((b) => (
                        <Label
                          key={b.id}
                          htmlFor={`${ids}-build-${b.id}`}
                          className="items-start rounded-md border p-3 font-normal has-[[data-state=checked]]:border-primary"
                        >
                          <RadioGroupItem
                            id={`${ids}-build-${b.id}`}
                            value={b.id}
                            className="mt-0.5"
                            data-testid={`loader-build-${b.id}`}
                          />
                          <span className="min-w-0">
                            <span className="flex flex-wrap items-center gap-2">
                              <span className="font-medium">{b.name}</span>
                              {b.recommended && <Badge>{t("loaderInstall.recommended")}</Badge>}
                              {b.publisher === "fork" && <Badge variant="outline">{t("loaderInstall.fork")}</Badge>}
                              {replace?.build === b.id && (
                                <Badge variant="secondary" data-testid="loader-build-installed">
                                  {t("loaderReplace.installed")}
                                </Badge>
                              )}
                            </span>
                            <span className="block text-xs text-muted-foreground">
                              {b.summary[uiLanguage] ?? b.summary.en}
                            </span>
                            {(b.limits[uiLanguage] ?? b.limits.en) && (
                              <span className="mt-0.5 block text-xs text-muted-foreground/80">
                                {t("loaderInstall.limits", { text: b.limits[uiLanguage] ?? b.limits.en ?? "" })}
                              </span>
                            )}
                          </span>
                        </Label>
                      ))}
                    </RadioGroup>
                  </fieldset>
                )}

                {build && !card && (
                  <>
                    {offer.isFetching && !offer.data && (
                      <p className="flex items-center gap-2 text-sm text-muted-foreground" role="status">
                        <Loader2 className="size-4 animate-spin" aria-hidden />
                        {t("loaderInstall.loadingRelease")}
                      </p>
                    )}
                    {offer.error && (
                      <p className="rounded-md border border-destructive/40 bg-destructive/10 p-3 text-sm text-destructive" role="alert">
                        {errorText(offer.error)}
                      </p>
                    )}
                    {offer.data && <OfferDetails offer={offer.data} />}
                    {!simple && (
                      <div className="-mt-2 text-xs" data-testid="loader-versions">
                        {!showVersions ? (
                          <Button
                            variant="link"
                            size="sm"
                            className="h-auto p-0 text-xs"
                            onClick={() => setShowVersions(true)}
                            data-testid="loader-other-version"
                          >
                            {t("loaderInstall.otherVersion")}
                          </Button>
                        ) : releases.isPending ? (
                          <span className="flex items-center gap-2 text-muted-foreground" role="status">
                            <Loader2 className="size-3 animate-spin" aria-hidden />
                            {t("loaderInstall.loadingReleases")}
                          </span>
                        ) : releases.error ? (
                          <span className="text-destructive" role="alert">
                            {errorText(releases.error)}
                          </span>
                        ) : (releases.data?.releases.length ?? 0) < 2 ? (
                          <span className="text-muted-foreground">{t("loaderInstall.onlyRelease")}</span>
                        ) : (
                          <RadioGroup
                            value={tag ?? releases.data?.releases.find((r) => r.default)?.tag ?? ""}
                            onValueChange={(value) =>
                              set({ tag: releases.data?.releases.find((r) => r.tag === value)?.default ? null : value })
                            }
                            aria-label={t("loaderInstall.versionLegend")}
                          >
                            {releases.data?.releases.map((r) => (
                              <Label key={r.tag} htmlFor={`${ids}-tag-${r.tag}`} className="font-normal">
                                <RadioGroupItem id={`${ids}-tag-${r.tag}`} value={r.tag} data-testid={`loader-tag-${r.tag}`} />
                                <span className="font-mono">{r.version ? versionLabel(r.version) : r.tag}</span>
                                {r.date && <span className="text-muted-foreground">{r.date}</span>}
                                {r.default && <Badge variant="outline">{t("loaderInstall.defaultRelease")}</Badge>}
                              </Label>
                            ))}
                          </RadioGroup>
                        )}
                      </div>
                    )}
                  </>
                )}

                {replacing && (
                  <fieldset>
                    <legend className="mb-2 text-sm font-medium">{t("loaderReplace.impactLegend")}</legend>
                    {impact.isPending && (
                      <p className="flex items-center gap-2 text-sm text-muted-foreground" role="status">
                        <Loader2 className="size-4 animate-spin" aria-hidden />
                        {t("loaderReplace.impactLoading")}
                      </p>
                    )}
                    {impact.error && (
                      <p className="rounded-md border border-destructive/40 bg-destructive/10 p-3 text-sm text-destructive" role="alert">
                        {errorText(impact.error)}
                      </p>
                    )}
                    {impact.data && (
                      <ReplaceImpact
                        impact={impact.data}
                        keepZso={form.keepZso}
                        onKeepZso={(keepZso) => set({ keepZso })}
                        reencodeArt={form.reencodeArt}
                        onReencodeArt={(reencodeArt) => set({ reencodeArt })}
                      />
                    )}
                  </fieldset>
                )}

                {replace && leftBehind && (
                  <Label htmlFor={`${ids}-retire`} className="items-start font-normal">
                    <Checkbox
                      id={`${ids}-retire`}
                      checked={form.retire}
                      onCheckedChange={(on) => set({ retire: on === true })}
                      className="mt-0.5"
                      data-testid="loader-retire"
                    />
                    <span>
                      {t("loaderReplace.retire", { path: replace.path })}
                      <span className="block text-xs text-muted-foreground">{t("loaderReplace.retireHint")}</span>
                    </span>
                  </Label>
                )}

                {!card && (offersLanguage || starterOffered) && (
                  <fieldset className="flex flex-col gap-2">
                    <legend className="mb-2 text-sm font-medium">{t("loaderInstall.optionsLegend")}</legend>
                    {offersLanguage && (
                      <Label htmlFor={`${ids}-language`} className="items-start font-normal">
                        <Checkbox
                          id={`${ids}-language`}
                          checked={form.language}
                          onCheckedChange={(on) => set({ language: on === true })}
                          className="mt-0.5"
                          data-testid="loader-language"
                        />
                        <span>
                          {t("loaderInstall.language", { language: t(`languages.${uiLanguage}`) })}
                          <span className="block text-xs text-muted-foreground">{t("loaderInstall.languageHint")}</span>
                        </span>
                      </Label>
                    )}
                    {starterOffered && (
                      <Label htmlFor={`${ids}-starter`} className="items-start font-normal">
                        <Checkbox
                          id={`${ids}-starter`}
                          checked={form.starter}
                          onCheckedChange={(on) => set({ starter: on === true })}
                          className="mt-0.5"
                          data-testid="loader-starter"
                        />
                        <span>
                          {t("loaderInstall.starter")}
                          <span className="block text-xs text-muted-foreground">{t("loaderInstall.starterHint")}</span>
                        </span>
                      </Label>
                    )}
                  </fieldset>
                )}
              </div>
            )}
            {error && (
              <p className="mt-4 rounded-md border border-destructive/40 bg-destructive/10 p-3 text-sm text-destructive" role="alert">
                {error}
              </p>
            )}
          </div>
          <DialogFooter>
            {done ? (
              <Button onClick={close} data-testid="loader-install-done">
                {t("common.close")}
              </Button>
            ) : (
              <>
                <Button variant="outline" onClick={close} disabled={busy !== null}>
                  {t("common.cancel")}
                </Button>
                <Button onClick={() => void install()} disabled={!ready} data-testid="loader-install-start">
                  {busy !== null ? <Loader2 className="animate-spin" aria-hidden /> : <Download aria-hidden />}
                  {busy === "download"
                    ? t("loaderInstall.downloading")
                    : busy === "finish"
                      ? t("loaderInstall.finishing")
                      : card
                        ? t("loaderInstall.createFolders")
                        : replace
                          ? offer.data?.stored
                            ? t("loaderReplace.replace")
                            : t("loaderReplace.downloadAndReplace")
                          : offer.data?.stored
                            ? t("loaderInstall.install")
                            : t("loaderInstall.downloadAndInstall")}
                </Button>
              </>
            )}
          </DialogFooter>
        </DialogContent>
      </Dialog>
      <PlanDialog flow={flow} choice={replaceChoice} />
    </>
  );
}

/** What will be installed (LDR-3, §5.10.2): shown before the download. */
function OfferDetails({ offer }: { offer: LoaderOffer }) {
  const { t } = useTranslation();
  const simple = useSimpleMode();
  const facts = [
    offer.date && t("loaderInstall.released", { date: formatDate(new Date(`${offer.date}T12:00:00`).getTime()) }),
    t("loaderInstall.size", { size: formatBytes(offer.file.size) }),
  ].filter(Boolean);
  return (
    <div className="flex flex-col gap-2 rounded-md border bg-muted/40 p-3 text-sm" data-testid="loader-offer">
      <div className="flex flex-wrap items-center gap-2">
        <span className="font-medium">{t("loaderInstall.version")}</span>
        <span className="font-mono wrap-anywhere">{offer.version}</span>
        {offer.stored && <Badge variant="outline">{t("loaderInstall.stored")}</Badge>}
      </div>
      <p className="text-xs text-muted-foreground">{facts.join(" · ")}</p>
      {offer.offline && (
        <p className="text-xs text-amber-700 dark:text-amber-400" role="status">
          {t("loaderInstall.offline")}
        </p>
      )}
      {offer.changelog.length > 0 ? (
        <details>
          <summary className="cursor-pointer text-xs font-medium">{t("loaderInstall.changes", { count: offer.changelog.length })}</summary>
          <ul className="mt-1 list-inside list-disc text-xs text-muted-foreground">
            {offer.changelog.map((change, index) => (
              <li key={index} className="wrap-anywhere">
                {change.link ? <ExternalLink href={change.link}>{change.text}</ExternalLink> : change.text}
              </li>
            ))}
          </ul>
          {offer.changelogMore && <p className="mt-1 text-xs text-muted-foreground">{t("loaderInstall.changesMore")}</p>}
        </details>
      ) : (
        <p className="text-xs text-muted-foreground">{t("loaderInstall.noChanges")}</p>
      )}
      <p className="text-xs">
        {t("loaderInstall.license", { license: offer.license.spdx, authors: offer.license.authors })}{" "}
        <ExternalLink href={offer.license.url}>{t("loaderInstall.licenseText")}</ExternalLink>
      </p>
      <p className="flex flex-wrap gap-x-3 text-xs">
        <ExternalLink href={offer.releaseUrl}>{t("loaderInstall.release")}</ExternalLink>
        {offer.commitUrl && <ExternalLink href={offer.commitUrl}>{t("loaderInstall.commit")}</ExternalLink>}
      </p>
      {!simple && (
        <p className="text-xs text-muted-foreground">
          SHA-256: <span className="font-mono break-all">{offer.file.sha256 ?? t("loaderInstall.hashOnDownload")}</span>
        </p>
      )}
    </div>
  );
}

/**
 * How to start the loader on the console (BOOT-1): the lines go into the
 * launcher's config by the user's hand; YAOPLM never writes memory card
 * configs.
 */
function BootHints({
  hints,
  card,
  target,
  build,
}: {
  hints: readonly LoaderBootHint[];
  card: boolean;
  target: LoaderTarget;
  build: LoaderBuild | undefined;
}) {
  const { t } = useTranslation();
  return (
    <div className="flex flex-col gap-3 text-sm" data-testid="loader-boot-hints">
      <p className="flex items-center gap-2 text-emerald-700 dark:text-emerald-400">
        <CheckCircle2 className="size-4" aria-hidden />
        {card ? t("loaderInstall.cardDone") : t("loaderInstall.eject")}
      </p>
      {hints.map((hint, index) => (
        <section key={index} className="rounded-md border p-3" data-testid="loader-boot-hint" data-launcher={hint.launcher}>
          <h3 className="font-medium">{t(`loaderInstall.launchers.${hint.launcher}`)}</h3>
          {hint.launcher === "oplApps" ? (
            <p className="text-muted-foreground">{t("loaderInstall.appsPage", { title: hint.title ?? "" })}</p>
          ) : hint.stock ? (
            <p className="text-muted-foreground wrap-anywhere">{t("loaderInstall.stock", { path: hint.ps2Path ?? "" })}</p>
          ) : (
            <>
              <p className="text-muted-foreground wrap-anywhere">{t("loaderInstall.addLines", { config: hint.config ?? "" })}</p>
              <pre className="relative mt-2 overflow-x-auto rounded-md bg-muted p-2 font-mono text-xs">{hint.lines.join("\n")}</pre>
              {hint.lines.some((line) => line.includes("_N")) && (
                <p className="mt-1 text-xs text-muted-foreground">{t("loaderInstall.itemNumber")}</p>
              )}
            </>
          )}
        </section>
      ))}
      <p>{t(`loaderInstall.turnOn.${target}`)}</p>
      {build && build.docs.length > 0 && (
        <p className="flex flex-wrap items-center gap-x-3 text-xs">
          <span className="text-muted-foreground">{t("loaderInstall.docs")}</span>
          {build.docs.map((doc) => (
            <ExternalLink key={doc.url} href={doc.url}>
              {t(`loaderInstall.docKinds.${doc.kind}`)}
            </ExternalLink>
          ))}
        </p>
      )}
    </div>
  );
}
