import { AlertTriangle, CheckCircle2, Info, XCircle } from "lucide-react";
import { useId } from "react";
import { useTranslation } from "react-i18next";

import type { ImpactGame } from "@/api/generated/ImpactGame";
import type { LoaderImpact } from "@/api/generated/LoaderImpact";
import { Checkbox } from "@/components/ui/checkbox";
import { Label } from "@/components/ui/label";
import { formatBytes } from "@/lib/format";
import { phraseText } from "@/lib/phrase";

/**
 * "What changes for the games" (LDR-7, SPEC §5.10.3): the library under
 * the new build's rules. ZSO games and JPG/BMP pictures are fixed in the
 * same plan unless left out; games in several pieces only get the README's
 * advice; an exFAT device under OPL 1.1.0 cannot be replaced at all.
 */
export function ReplaceImpact({
  impact,
  keepZso,
  onKeepZso,
  reencodeArt,
  onReencodeArt,
}: {
  impact: LoaderImpact;
  /** ZSO images left as they are. */
  keepZso: readonly string[];
  onKeepZso: (paths: string[]) => void;
  reencodeArt: boolean;
  onReencodeArt: (on: boolean) => void;
}) {
  const { t } = useTranslation();
  const ids = useId();
  if (impact.blocked) {
    return (
      <p
        className="flex items-start gap-2 rounded-md border border-destructive/40 bg-destructive/10 p-3 text-sm text-destructive"
        role="alert"
        data-testid="loader-impact-blocked"
      >
        <XCircle className="mt-0.5 size-4 shrink-0" aria-hidden />
        {phraseText(impact.blocked, "")}
      </p>
    );
  }
  const nothing =
    impact.zso.length === 0 &&
    impact.fragmented.length === 0 &&
    impact.unknownFragments.length === 0 &&
    impact.art.length === 0;
  if (nothing) {
    return (
      <p className="flex items-center gap-2 text-sm text-emerald-700 dark:text-emerald-400" data-testid="loader-impact-nothing">
        <CheckCircle2 className="size-4" aria-hidden />
        {impact.from === impact.to ? t("loaderReplace.nothing") : t("loaderReplace.nothingAffected")}
      </p>
    );
  }
  const toggle = (path: string, convert: boolean) =>
    onKeepZso(convert ? keepZso.filter((p) => p !== path) : [...keepZso, path]);
  return (
    <div className="flex flex-col gap-3 text-sm" data-testid="loader-impact">
      {impact.zso.length > 0 && (
        <section className="rounded-md border p-3">
          <h4 className="font-medium">{t("loaderReplace.zsoTitle", { count: impact.zso.length })}</h4>
          <p className="text-xs text-muted-foreground">{t("loaderReplace.zsoHint")}</p>
          <ul className="relative mt-2 flex max-h-40 flex-col gap-1 overflow-auto">
            {impact.zso.map((game) => {
              const path = game.path ?? "";
              return (
                <li key={path}>
                  <Label htmlFor={`${ids}-${path}`} className="font-normal">
                    <Checkbox
                      id={`${ids}-${path}`}
                      checked={!keepZso.includes(path)}
                      onCheckedChange={(on) => toggle(path, on === true)}
                      data-testid="loader-impact-zso"
                    />
                    <GameLine game={game} />
                  </Label>
                </li>
              );
            })}
          </ul>
        </section>
      )}
      {impact.fragmented.length > 0 && (
        <section className="rounded-md border border-amber-600/40 p-3">
          <h4 className="flex items-center gap-2 font-medium text-amber-700 dark:text-amber-400">
            <AlertTriangle className="size-4" aria-hidden />
            {t("loaderReplace.fragmentedTitle", { count: impact.fragmented.length })}
          </h4>
          <p className="text-xs text-muted-foreground">{t("loaderReplace.fragmentedHint")}</p>
          <GameList games={impact.fragmented} pieces />
        </section>
      )}
      {impact.unknownFragments.length > 0 && (
        <section className="rounded-md border p-3">
          <h4 className="flex items-center gap-2 font-medium">
            <Info className="size-4" aria-hidden />
            {t("loaderReplace.unknownTitle", { count: impact.unknownFragments.length })}
          </h4>
          <p className="text-xs text-muted-foreground">{t("loaderReplace.unknownHint")}</p>
          <GameList games={impact.unknownFragments} />
        </section>
      )}
      {impact.art.length > 0 && (
        <section className="rounded-md border p-3">
          <Label htmlFor={`${ids}-art`} className="items-start font-normal">
            <Checkbox
              id={`${ids}-art`}
              checked={reencodeArt}
              onCheckedChange={(on) => onReencodeArt(on === true)}
              className="mt-0.5"
              data-testid="loader-impact-art"
            />
            <span>
              <span className="font-medium">{t("loaderReplace.artTitle", { count: impact.art.length })}</span>
              <span className="block text-xs text-muted-foreground">{t("loaderReplace.artHint")}</span>
            </span>
          </Label>
          <details className="mt-1">
            <summary className="cursor-pointer text-xs">{t("loaderReplace.artFiles")}</summary>
            <ul className="relative mt-1 max-h-32 overflow-auto font-mono text-xs text-muted-foreground">
              {impact.art.map((art) => (
                <li key={art.path} className="wrap-anywhere">
                  {art.path}
                </li>
              ))}
            </ul>
          </details>
        </section>
      )}
    </div>
  );
}

function GameLine({ game, pieces = false }: { game: ImpactGame; pieces?: boolean }) {
  const { t } = useTranslation();
  return (
    <span className="min-w-0 wrap-anywhere">
      <span className="font-mono text-xs">{game.startup}</span> {game.title}{" "}
      <span className="text-xs text-muted-foreground">
        {formatBytes(game.size)}
        {pieces && game.fragments !== null && ` · ${t("loaderReplace.pieces", { count: game.fragments })}`}
      </span>
    </span>
  );
}

function GameList({ games, pieces = false }: { games: readonly ImpactGame[]; pieces?: boolean }) {
  return (
    <ul className="relative mt-2 flex max-h-32 flex-col gap-1 overflow-auto">
      {games.map((game) => (
        <li key={`${game.startup}-${game.path ?? ""}`}>
          <GameLine game={game} pieces={pieces} />
        </li>
      ))}
    </ul>
  );
}
