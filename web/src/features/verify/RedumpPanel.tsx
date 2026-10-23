import { useQuery, useQueryClient } from "@tanstack/react-query";
import { isTauri } from "@tauri-apps/api/core";
import { cn } from "cn";
import { Database, FileUp, Loader2, RefreshCw } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";

import { useApi } from "@/api/context";
import { redumpStatus, updateRedump, waitForJob } from "@/api/rpc";
import { Button } from "@/components/ui/button";
import { pickDat } from "@/features/add/desktop";
import { FolderBrowserDialog } from "@/features/add/FolderBrowserDialog";
import { formatDate } from "@/lib/format";
import { errorText } from "@/lib/phrase";

/**
 * The Redump DAT on this computer (META-2): what it is, when Redump may be
 * asked again, and the two ways to get one. Redump is asked only when the
 * user presses the button, and at most once a week; a DAT downloaded by
 * hand can be imported instead (offline).
 */
export function RedumpPanel({ compact = false }: { compact?: boolean }) {
  const { t } = useTranslation();
  const { transport } = useApi();
  const queryClient = useQueryClient();
  const status = useQuery({ queryKey: ["redump"], queryFn: () => redumpStatus(transport) });
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [browserOpen, setBrowserOpen] = useState(false);
  // When the panel opened: a week is long enough not to tick.
  const [now] = useState(Date.now);
  const dat = status.data ?? null;
  const tooSoon = dat?.nextCheck != null && now < dat.nextCheck;

  const run = async (file?: string) => {
    setBusy(true);
    setMessage(null);
    setError(null);
    try {
      const job = await updateRedump(transport, file);
      const info = await waitForJob(transport, job);
      if (info.status === "finished" && info.result?.type === "Redump") {
        const { update, dat: stored, nextCheck } = info.result;
        setMessage(
          t(`dump.update.${update}`, {
            games: stored?.games ?? 0,
            date: nextCheck === null ? "" : formatDate(nextCheck),
          }),
        );
      } else if (info.error) {
        setError(errorText(info.error));
      }
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
      await queryClient.invalidateQueries({ queryKey: ["redump"] });
    }
  };

  const importDat = () => {
    if (isTauri()) void pickDat(t("dump.datFilter")).then((file) => file && void run(file));
    else setBrowserOpen(true);
  };

  return (
    <div className={cn("flex flex-col gap-2 text-sm", !compact && "rounded-md border bg-muted/40 p-3")} data-testid="redump-panel">
      <div className="flex items-start gap-2">
        <Database className="mt-0.5 size-4 shrink-0 text-muted-foreground" aria-hidden />
        <div className="min-w-0 flex-1">
          {status.isPending ? (
            <span className="text-muted-foreground">{t("common.loading")}</span>
          ) : dat === null ? (
            <span data-testid="redump-none">{t("dump.noDat")}</span>
          ) : (
            <>
              <div data-testid="redump-state">{t("dump.datState", { games: dat.games, version: dat.version })}</div>
              <div className="truncate text-xs text-muted-foreground" title={dat.source}>
                {dat.source.startsWith("http")
                  ? t("dump.downloadedOn", { date: formatDate(dat.storedAt) })
                  : t("dump.importedOn", { date: formatDate(dat.storedAt), file: dat.fileName ?? dat.source })}
                {tooSoon && dat.nextCheck !== null && ` · ${t("dump.nextCheck", { date: formatDate(dat.nextCheck) })}`}
              </div>
              {!dat.serials && <div className="text-xs text-amber-600 dark:text-amber-400">{t("dump.noSerials")}</div>}
            </>
          )}
        </div>
      </div>
      <div className="flex flex-wrap gap-2">
        <Button
          size="sm"
          // Primary only once the status says there is no DAT: switching
          // after the load would flash a dark button.
          variant={status.isSuccess && dat === null ? "default" : "outline"}
          disabled={busy || status.isPending || tooSoon}
          onClick={() => void run()}
          data-testid="redump-update"
        >
          {busy ? <Loader2 className="animate-spin" aria-hidden /> : <RefreshCw aria-hidden />}
          {dat === null ? t("dump.download") : t("dump.checkUpdate")}
        </Button>
        <Button size="sm" variant="outline" disabled={busy} onClick={importDat} data-testid="redump-import">
          <FileUp aria-hidden />
          {t("dump.import")}
        </Button>
      </div>
      {!compact && <p className="text-xs text-muted-foreground">{t("dump.datHint")}</p>}
      {message && (
        <p className="text-xs" role="status" data-testid="redump-message">
          {message}
        </p>
      )}
      {error && (
        <p className="rounded-md border border-destructive/40 bg-destructive/10 p-2 text-xs text-destructive" role="alert">
          {error}
        </p>
      )}
      <FolderBrowserDialog
        open={browserOpen}
        onOpenChange={setBrowserOpen}
        mode="dat"
        onPick={([file]) => file && void run(file)}
      />
    </div>
  );
}
