import { ExternalLink } from "lucide-react";
import { useId, useState } from "react";
import { useTranslation } from "react-i18next";

import type { GameVerification } from "@/api/generated/GameVerification";
import type { HashValues } from "@/api/generated/HashValues";
import type { VerificationReport } from "@/api/generated/VerificationReport";
import { Badge } from "@/components/ui/badge";
import { statusColours, statusCounts } from "@/features/verify/dump";
import { DumpBadge, DumpIcon } from "@/features/verify/DumpBadge";
import { formatBytes } from "@/lib/format";
import { phraseText } from "@/lib/phrase";

/** The outcome per game, problems first. */
export function VerificationResults({
  report,
  summary = true,
}: {
  report: VerificationReport;
  /** The counts per verdict, unless the caller shows them already. */
  summary?: boolean;
}) {
  const { t } = useTranslation();
  const order = { failed: 0, badDump: 1, unknown: 2, verified: 3 } as const;
  const sorted = [...report.games].sort((a, b) => order[a.status] - order[b.status] || a.title.localeCompare(b.title));
  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3" data-testid="verify-results">
      <ul className="flex flex-wrap gap-1.5" aria-label={t("dump.summary")} hidden={!summary}>
        {statusCounts(report.games).map(([status, count]) => (
          <li key={status}>
            <Badge variant="outline" className={statusColours[status]} data-testid={`verify-count-${status}`}>
              <DumpIcon status={status} className="size-3" />
              {t(`dump.count.${status}`, { count })}
            </Badge>
          </li>
        ))}
      </ul>
      {report.dat && (
        <p className="text-xs text-muted-foreground">{t("dump.comparedWith", { version: report.dat.version, games: report.dat.games })}</p>
      )}
      <ul className="min-h-0 flex-1 overflow-auto rounded-md border text-sm">
        {sorted.map((game) => (
          <ResultRow key={`${game.path}:${game.ulIndex ?? ""}`} game={game} />
        ))}
      </ul>
    </div>
  );
}

function ResultRow({ game }: { game: GameVerification }) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const trackId = useId();
  return (
    <li className="border-b p-2 last:border-0" data-testid="verify-row" data-status={game.status}>
      <div className="flex flex-wrap items-center gap-2">
        <DumpBadge status={game.status} reason={game.reason} />
        <span className="min-w-0 flex-1 truncate font-medium" title={game.title}>
          {game.title}
        </span>
        <span className="font-mono text-xs text-muted-foreground">{game.startup}</span>
        {game.hashes && (
          <button
            type="button"
            className="text-xs text-primary underline-offset-2 hover:underline"
            aria-expanded={open}
            onClick={() => setOpen(!open)}
          >
            {open ? t("dump.hideHashes") : t("dump.showHashes")}
          </button>
        )}
      </div>
      <div className="mt-1 flex flex-col gap-0.5 text-xs text-muted-foreground">
        {game.redump && (
          <span>
            {t("dump.matches")}{" "}
            <RedumpLink name={game.redump.name} url={game.redump.url} />
            {game.serialMatches === false && <span className="text-amber-700 dark:text-amber-400"> · {t("dump.serialMismatch")}</span>}
          </span>
        )}
        {game.reason && <span>{t(`dump.reasonLong.${game.reason}`)}</span>}
        {game.status === "badDump" && <span>{t("dump.badDumpHint")}</span>}
        {game.candidates.length > 0 && (
          <span>
            {t("dump.candidates")}{" "}
            {game.candidates.map((candidate, index) => (
              <span key={`${candidate.name}:${candidate.rom}`}>
                {index > 0 && ", "}
                <RedumpLink name={candidate.name} url={candidate.url} />
                {candidate.sameSize &&
                  ` (${candidate.rom.toLowerCase().endsWith(".bin") ? t("dump.sameSectors") : t("dump.sameSize")})`}
              </span>
            ))}
          </span>
        )}
        {game.byTrack && <span>{t("dump.byTrack")}</span>}
        {game.cached && <span>{t("dump.fromCache")}</span>}
        {game.error && <span className="text-destructive">{phraseText(game.errorPhrase, game.error)}</span>}
      </div>
      {open && game.hashes && (
        <div className="mt-2 flex flex-col gap-2 rounded-md bg-muted/50 p-2 text-xs">
          <HashList hashes={game.hashes} />
          {game.track && (
            <section aria-labelledby={trackId}>
              <h4 id={trackId} className="mb-0.5 text-muted-foreground">
                {t("dump.trackHashes")}
              </h4>
              <HashList hashes={game.track} />
            </section>
          )}
        </div>
      )}
    </li>
  );
}

function HashList({ hashes }: { hashes: HashValues }) {
  const { t } = useTranslation();
  return (
    <dl className="grid grid-cols-[max-content_1fr] gap-x-4 gap-y-0.5 font-mono">
      <dt className="text-muted-foreground">{t("game.size")}</dt>
      <dd>
        {formatBytes(hashes.size)} ({hashes.size})
      </dd>
      <dt className="text-muted-foreground">CRC-32</dt>
      <dd className="wrap-anywhere">{hashes.crc32}</dd>
      <dt className="text-muted-foreground">SHA-1</dt>
      <dd className="wrap-anywhere">{hashes.sha1}</dd>
      {hashes.md5 && (
        <>
          <dt className="text-muted-foreground">MD5</dt>
          <dd className="wrap-anywhere">{hashes.md5}</dd>
        </>
      )}
    </dl>
  );
}

function RedumpLink({ name, url }: { name: string; url: string | null }) {
  if (!url) return <span className="text-foreground">{name}</span>;
  return (
    <a href={url} target="_blank" rel="noreferrer" className="inline-flex items-center gap-1 text-primary underline-offset-2 hover:underline">
      {name}
      <ExternalLink className="size-3" aria-hidden />
    </a>
  );
}
