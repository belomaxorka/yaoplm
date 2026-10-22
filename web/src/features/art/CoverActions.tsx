import { isTauri } from "@tauri-apps/api/core";
import { cn } from "cn";
import { ClipboardPaste, FileImage, ImageDown, ImageOff, Loader2, Replace } from "lucide-react";
import { type DragEvent, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import type { ArtInput } from "@/api/generated/ArtInput";
import type { Game } from "@/api/generated/Game";
import { coverOf, hasCover } from "@/api/media";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { listenForDrops, pickImage } from "@/features/add/desktop";
import { FolderBrowserDialog } from "@/features/add/FolderBrowserDialog";
import { ArtFetchDialog } from "@/features/art/ArtFetchDialog";
import { imageBlobToPng, readClipboardImage } from "@/features/art/clipboard";
import { GameCover } from "@/features/library/GameCover";
import { PlanDialog } from "@/features/plan/PlanDialog";
import { usePlan } from "@/features/plan/usePlan";
import { useSettings } from "@/settings/context";

/**
 * The cover on the game card with its actions (ART-1, ART-3): download,
 * replace from a file, a drop or the clipboard, delete. Every change is a
 * plan the user confirms.
 */
export function CoverActions({ game, root, games }: { game: Game; root: string; games: readonly Game[] }) {
  const { t } = useTranslation();
  const { settings } = useSettings();
  const [fetchOpen, setFetchOpen] = useState(false);
  const [browseOpen, setBrowseOpen] = useState(false);
  const [hovering, setHovering] = useState(false);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const flow = usePlan();
  const desktop = isTauri();
  const cover = coverOf(game);

  const setArt = (input: ArtInput) => {
    setNotice(null);
    void flow.request({
      type: "SetArt",
      root,
      startup: game.startup,
      input,
      profile: settings.profile,
      overwrite: true,
    });
  };

  const fromBlob = async (blob: Blob, name: string) => {
    setBusy(true);
    try {
      setArt({ kind: "bytes", data: await imageBlobToPng(blob), name });
    } catch (err) {
      setNotice(t("art.notAnImage", { error: String(err) }));
    } finally {
      setBusy(false);
    }
  };

  const paste = async () => {
    setBusy(true);
    setNotice(null);
    try {
      const image = await readClipboardImage();
      if (!image) setNotice(t("art.clipboardEmpty"));
      else setArt({ kind: "bytes", data: image.data, name: image.name });
    } catch (err) {
      setNotice(t("art.clipboardFailed", { error: String(err) }));
    } finally {
      setBusy(false);
    }
  };

  // Desktop drops arrive as host paths; the core reads the file itself.
  useEffect(
    () =>
      listenForDrops((paths) => {
        const [first] = paths;
        if (first) setArt({ kind: "file", path: first });
      }, setHovering),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [root, game.startup],
  );

  // Browser drops carry the bytes.
  const onDrop = (event: DragEvent<HTMLDivElement>) => {
    event.preventDefault();
    setHovering(false);
    const file = event.dataTransfer.files[0];
    if (file) void fromBlob(file, file.name);
  };

  return (
    <div className="flex flex-col gap-3" data-testid="cover-actions">
      <div
        className={cn("relative rounded-lg transition-shadow", hovering && "ring-3 ring-primary")}
        onDragOver={(event) => {
          if (desktop) return;
          event.preventDefault();
          setHovering(true);
        }}
        onDragLeave={() => !desktop && setHovering(false)}
        onDrop={desktop ? undefined : onDrop}
      >
        <GameCover game={game} className="w-48" />
        {busy && (
          <div className="absolute inset-0 flex items-center justify-center rounded-lg bg-background/60">
            <Loader2 className="size-6 animate-spin" aria-hidden />
          </div>
        )}
      </div>
      {cover && (
        <p className="max-w-48 truncate font-mono text-xs text-muted-foreground" title={cover.path}>
          {cover.width !== null && cover.height !== null ? `${cover.width}×${cover.height} ` : ""}
          {cover.path.replace(/^.*[\\/]/, "")}
        </p>
      )}
      <div className="flex max-w-48 flex-col gap-1.5">
        <Button size="sm" onClick={() => setFetchOpen(true)} data-testid="cover-download">
          <ImageDown aria-hidden />
          {hasCover(game) ? t("art.downloadAgain") : t("art.download")}
        </Button>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button size="sm" variant="outline" data-testid="cover-replace">
              <Replace aria-hidden />
              {hasCover(game) ? t("art.replace") : t("art.setManually")}
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="start">
            <DropdownMenuItem
              onSelect={() => {
                if (desktop) void pickImage().then((path) => path && setArt({ kind: "file", path }));
                else setBrowseOpen(true);
              }}
              data-testid="cover-from-file"
            >
              <FileImage aria-hidden />
              {desktop ? t("art.fromFile") : t("art.fromServerFile")}
            </DropdownMenuItem>
            <DropdownMenuItem onSelect={() => void paste()}>
              <ClipboardPaste aria-hidden />
              {t("art.fromClipboard")}
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
        {hasCover(game) && (
          <Button
            size="sm"
            variant="outline"
            className="text-destructive"
            onClick={() => void flow.request({ type: "DeleteArt", root, startup: game.startup })}
            data-testid="cover-delete"
          >
            <ImageOff aria-hidden />
            {t("art.delete")}
          </Button>
        )}
        <p className="text-xs text-muted-foreground">{desktop ? t("art.dropHintDesktop") : t("art.dropHintBrowser")}</p>
        {notice && (
          <p className="text-xs text-destructive" role="alert">
            {notice}
          </p>
        )}
      </div>
      <ArtFetchDialog root={root} games={games} only={game} open={fetchOpen} onOpenChange={setFetchOpen} />
      <FolderBrowserDialog
        open={browseOpen}
        onOpenChange={setBrowseOpen}
        mode="image"
        onPick={([path]) => path && setArt({ kind: "file", path })}
      />
      <PlanDialog flow={flow} />
    </div>
  );
}
