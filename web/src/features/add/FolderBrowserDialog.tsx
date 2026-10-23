import { useQuery } from "@tanstack/react-query";
import { ArrowUp, Disc3, File, Folder, HardDrive, Image as ImageIcon } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";

import { useApi } from "@/api/context";
import type { DirEntry } from "@/api/generated/DirEntry";
import { listDir } from "@/api/rpc";
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
import { DAT_EXTENSIONS } from "@/features/add/desktop";
import { isImageName } from "@/features/art/clipboard";
import { formatBytes } from "@/lib/format";
import { errorText } from "@/lib/phrase";

function isDatName(name: string): boolean {
  const dot = name.lastIndexOf(".");
  return dot >= 0 && DAT_EXTENSIONS.includes(name.slice(dot + 1).toLowerCase());
}

/**
 * Picks import sources in server mode, where the browser has no file paths
 * to give: folders are listed by the core, inside its roots only (IMP-1,
 * ADR-0009). In `image` mode it picks one image file for a cover (ART-3),
 * in `dat` mode one Redump DAT to import (META-2).
 */
export function FolderBrowserDialog({
  open,
  onOpenChange,
  onPick,
  mode = "games",
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onPick: (paths: string[]) => void;
  mode?: "games" | "image" | "dat";
}) {
  const { t } = useTranslation();
  const images = mode === "image";
  const single = mode !== "games";
  const usableFile = (entry: DirEntry): boolean =>
    mode === "image" ? isImageName(entry.name) : mode === "dat" ? isDatName(entry.name) : entry.importable;
  const { transport } = useApi();
  const [path, setPath] = useState<string | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const listing = useQuery({
    queryKey: ["dir", path],
    queryFn: () => listDir(transport, path),
    enabled: open,
  });

  const toggle = (entry: DirEntry, checked: boolean) => {
    setSelected((current) => {
      const next = single ? new Set<string>() : new Set(current);
      if (checked) next.add(entry.path);
      else next.delete(entry.path);
      return next;
    });
  };

  const pickable = (entry: DirEntry): boolean => (entry.kind === "dir" ? !single && path !== null : usableFile(entry));

  const reset = () => {
    setPath(null);
    setSelected(new Set());
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) reset();
        onOpenChange(next);
      }}
    >
      <DialogContent className="flex max-h-[85dvh] flex-col sm:max-w-xl" data-testid="folder-browser">
        <DialogHeader>
          <DialogTitle>
            {images ? t("art.browseTitle") : mode === "dat" ? t("dump.browseTitle") : t("add.browseTitle")}
          </DialogTitle>
          <DialogDescription>
            {images ? t("art.browseDescription") : mode === "dat" ? t("dump.browseDescription") : t("add.browseDescription")}
          </DialogDescription>
        </DialogHeader>
        <div className="flex items-center gap-2 text-sm">
          <Button
            variant="outline"
            size="icon-sm"
            disabled={path === null}
            onClick={() => setPath(listing.data?.parent ?? null)}
            aria-label={t("add.up")}
          >
            <ArrowUp aria-hidden />
          </Button>
          <span className="min-w-0 flex-1 truncate font-mono text-xs" title={path ?? undefined}>
            {path ?? t("add.roots")}
          </span>
        </div>
        <ul className="min-h-48 flex-1 overflow-auto rounded-md border" aria-label={t("add.entries")}>
          {listing.isPending && <li className="p-3 text-sm text-muted-foreground">{t("common.loading")}</li>}
          {listing.error && <li className="p-3 text-sm text-destructive">{errorText(listing.error)}</li>}
          {listing.data?.entries.length === 0 && (
            <li className="p-3 text-sm text-muted-foreground">{t("add.emptyFolder")}</li>
          )}
          {listing.data?.entries.map((entry) => {
            const usable = entry.kind !== "dir" && usableFile(entry);
            const Icon = entry.kind === "dir" ? (path === null ? HardDrive : Folder) : usable ? (images ? ImageIcon : Disc3) : File;
            const selectable = pickable(entry);
            return (
              <li key={entry.path} className="flex items-center gap-2 border-b px-2 py-1 text-sm last:border-0">
                <Checkbox
                  checked={selected.has(entry.path)}
                  disabled={!selectable}
                  onCheckedChange={(value) => toggle(entry, value === true)}
                  aria-label={t("add.select", { name: entry.name })}
                />
                {entry.kind === "dir" ? (
                  <button
                    type="button"
                    className="flex min-w-0 flex-1 items-center gap-2 rounded px-1 py-0.5 text-left hover:bg-muted focus-visible:ring-3 focus-visible:ring-ring/50 outline-none"
                    onClick={() => setPath(entry.path)}
                  >
                    <Icon className="size-4 shrink-0 text-muted-foreground" aria-hidden />
                    <span className="truncate">{entry.name}</span>
                  </button>
                ) : (
                  <span className="flex min-w-0 flex-1 items-center gap-2 px-1 py-0.5">
                    <Icon className="size-4 shrink-0 text-muted-foreground" aria-hidden />
                    <span className={usable ? "truncate" : "truncate text-muted-foreground"}>{entry.name}</span>
                    {entry.size !== null && (
                      <span className="ml-auto shrink-0 font-mono text-xs text-muted-foreground tabular-nums">
                        {formatBytes(entry.size)}
                      </span>
                    )}
                  </span>
                )}
              </li>
            );
          })}
          {listing.data?.truncated && <li className="p-2 text-xs text-muted-foreground">{t("add.truncated")}</li>}
        </ul>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            {t("common.cancel")}
          </Button>
          <Button
            disabled={selected.size === 0}
            onClick={() => {
              onPick([...selected]);
              reset();
              onOpenChange(false);
            }}
            data-testid="browser-add"
          >
            {images ? t("art.useImage") : mode === "dat" ? t("dump.importThis") : t("add.addSelected", { count: selected.size })}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
