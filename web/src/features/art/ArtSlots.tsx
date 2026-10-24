import { isTauri } from "@tauri-apps/api/core";
import { cn } from "cn";
import { ClipboardPaste, FileImage, ImageDown, ImageOff, Loader2, Replace } from "lucide-react";
import { type DragEvent, useEffect, useId, useState } from "react";
import { useTranslation } from "react-i18next";

import type { ArtInput } from "@/api/generated/ArtInput";
import type { ArtType } from "@/api/generated/ArtType";
import type { Game } from "@/api/generated/Game";
import { artOf, artUrl } from "@/api/media";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { listenForDrops, pickImage } from "@/features/add/desktop";
import { FolderBrowserDialog } from "@/features/add/FolderBrowserDialog";
import { ArtVariantsDialog } from "@/features/art/ArtVariantsDialog";
import { imageBlobToPng, readClipboardImage } from "@/features/art/clipboard";
import { artAspect, artTypes, useFetchableTypes } from "@/features/art/kinds";
import { PlanDialog } from "@/features/plan/PlanDialog";
import { usePlan } from "@/features/plan/usePlan";
import { useLoaderProfile } from "@/features/devices/useLoaderProfile";

/**
 * The game's pictures, one slot per type OPL themes load (ART-2, ART-3,
 * SPEC §5.4.1): a preview, its size, and Download (to pick from what the
 * sources have), Replace (a file, the clipboard, a drop) and Delete. Every
 * change is a plan the user confirms; a replaced or deleted file keeps a
 * copy in the device's backup folder.
 */
export function ArtSlots({ game, root }: { game: Game; root: string }) {
  const { t } = useTranslation();
  const { profile: loaderProfile } = useLoaderProfile();
  const fetchable = useFetchableTypes();
  // Every disabled Download has the same reason at any one time.
  const unavailable = fetchable.blocked?.text ?? t("art.noSourceForKind");
  const unavailableId = useId();
  const [variantsFor, setVariantsFor] = useState<ArtType | null>(null);
  const [browseFor, setBrowseFor] = useState<ArtType | null>(null);
  const [hovering, setHovering] = useState<ArtType | "any" | null>(null);
  const [busy, setBusy] = useState<ArtType | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const flow = usePlan();
  const desktop = isTauri();

  const setArt = (kind: ArtType, input: ArtInput) => {
    setNotice(null);
    void flow.request({
      type: "SetArt",
      root,
      startup: game.startup,
      kind,
      input,
      profile: loaderProfile,
      overwrite: true,
    });
  };

  const fromBlob = async (kind: ArtType, blob: Blob, name: string) => {
    setBusy(kind);
    try {
      setArt(kind, { kind: "bytes", data: await imageBlobToPng(blob), name });
    } catch (err) {
      setNotice(t("art.notAnImage", { error: String(err) }));
    } finally {
      setBusy(null);
    }
  };

  const paste = async (kind: ArtType) => {
    setBusy(kind);
    setNotice(null);
    try {
      const image = await readClipboardImage();
      if (!image) setNotice(t("art.clipboardEmpty"));
      else setArt(kind, { kind: "bytes", data: image.data, name: image.name });
    } catch (err) {
      setNotice(t("art.clipboardFailed", { error: String(err) }));
    } finally {
      setBusy(null);
    }
  };

  // Desktop drops arrive as host paths for the whole window: the slot under
  // the pointer takes the first one.
  useEffect(
    () =>
      listenForDrops(
        (paths, at) => {
          const slot = document.elementFromPoint(at.x, at.y)?.closest<HTMLElement>("[data-art-slot]");
          const kind = slot?.dataset.artSlot as ArtType | undefined;
          const [first] = paths;
          if (kind && first) setArt(kind, { kind: "file", path: first });
        },
        (on) => setHovering(on ? "any" : null),
      ),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [root, game.startup],
  );

  return (
    <div className="flex flex-col gap-3" data-testid="art-slots">
      <ul className="grid grid-cols-2 gap-3 sm:grid-cols-4">
        {artTypes.map((kind) => {
          const art = artOf(game, kind);
          const canFetch = fetchable.types.has(kind);
          const kindName = t(`art.kinds.${kind}`);
          const onDrop = (event: DragEvent<HTMLLIElement>) => {
            event.preventDefault();
            setHovering(null);
            const file = event.dataTransfer.files[0];
            if (file) void fromBlob(kind, file, file.name);
          };
          return (
            <li
              key={kind}
              data-art-slot={kind}
              data-testid={`art-slot-${kind}`}
              data-has-art={art ? "true" : "false"}
              className={cn(
                "flex flex-col gap-1.5 rounded-lg border p-2 transition-shadow",
                (hovering === kind || hovering === "any") && "ring-2 ring-primary",
              )}
              onDragOver={(event) => {
                if (desktop) return;
                event.preventDefault();
                setHovering(kind);
              }}
              onDragLeave={() => !desktop && setHovering(null)}
              onDrop={desktop ? undefined : onDrop}
            >
              <div className="relative flex h-28 items-center justify-center overflow-hidden rounded bg-muted/50">
                {art ? (
                  <img
                    src={artUrl(art)}
                    alt=""
                    loading="lazy"
                    decoding="async"
                    draggable={false}
                    className="max-h-full max-w-full object-contain"
                    style={{ aspectRatio: artAspect[kind] }}
                  />
                ) : (
                  <span className="font-mono text-xs text-muted-foreground">{kind}</span>
                )}
                {busy === kind && (
                  <div className="absolute inset-0 flex items-center justify-center bg-background/60">
                    <Loader2 className="size-5 animate-spin" aria-hidden />
                  </div>
                )}
              </div>
              <div className="min-w-0">
                <div className="truncate text-sm font-medium">{kindName}</div>
                <div className="truncate font-mono text-xs text-muted-foreground" title={art?.path}>
                  {art
                    ? art.width !== null && art.height !== null
                      ? `${art.width}×${art.height}`
                      : art.path.replace(/^.*[\\/]/, "")
                    : t("art.slotEmpty")}
                </div>
              </div>
              <div className="flex gap-1">
                <Button
                  size="icon-sm"
                  variant="outline"
                  disabled={!canFetch}
                  title={canFetch ? t("art.downloadKind") : unavailable}
                  aria-label={t("art.downloadKindLabel", { kind: kindName })}
                  aria-describedby={canFetch ? undefined : unavailableId}
                  onClick={() => setVariantsFor(kind)}
                  data-testid={`art-download-${kind}`}
                >
                  <ImageDown aria-hidden />
                </Button>
                <DropdownMenu>
                  <DropdownMenuTrigger asChild>
                    <Button
                      size="icon-sm"
                      variant="outline"
                      title={t("art.replace")}
                      aria-label={t("art.replaceKindLabel", { kind: kindName })}
                      data-testid={`art-replace-${kind}`}
                    >
                      <Replace aria-hidden />
                    </Button>
                  </DropdownMenuTrigger>
                  <DropdownMenuContent align="start" className="w-max">
                    <DropdownMenuItem
                      onSelect={() => {
                        if (desktop) void pickImage(t("art.imagesFilter")).then((path) => path && setArt(kind, { kind: "file", path }));
                        else setBrowseFor(kind);
                      }}
                      data-testid={`art-from-file-${kind}`}
                    >
                      <FileImage aria-hidden />
                      {desktop ? t("art.fromFile") : t("art.fromServerFile")}
                    </DropdownMenuItem>
                    <DropdownMenuItem onSelect={() => void paste(kind)}>
                      <ClipboardPaste aria-hidden />
                      {t("art.fromClipboard")}
                    </DropdownMenuItem>
                  </DropdownMenuContent>
                </DropdownMenu>
                {art && (
                  <Button
                    size="icon-sm"
                    variant="outline"
                    className="text-destructive"
                    title={t("art.delete")}
                    aria-label={t("art.deleteKindLabel", { kind: kindName })}
                    onClick={() => void flow.request({ type: "DeleteArt", root, startup: game.startup, kind })}
                    data-testid={`art-delete-${kind}`}
                  >
                    <ImageOff aria-hidden />
                  </Button>
                )}
              </div>
            </li>
          );
        })}
      </ul>
      <p className="text-xs text-muted-foreground">{desktop ? t("art.dropHintDesktop") : t("art.dropHintBrowser")}</p>
      {/* Read as the description of a disabled Download, not on its own. */}
      <span id={unavailableId} hidden>
        {unavailable}
      </span>
      {notice && (
        <p className="text-xs text-destructive" role="alert">
          {notice}
        </p>
      )}
      {variantsFor && (
        <ArtVariantsDialog
          root={root}
          game={game}
          kind={variantsFor}
          open
          onOpenChange={(open) => !open && setVariantsFor(null)}
          onPick={(variant) =>
            setArt(variantsFor, { kind: "bytes", data: variant.png, name: `${variant.source} (${variant.origin})` })
          }
        />
      )}
      <FolderBrowserDialog
        open={browseFor !== null}
        onOpenChange={(open) => !open && setBrowseFor(null)}
        mode="image"
        onPick={([path]) => {
          if (path && browseFor) setArt(browseFor, { kind: "file", path });
        }}
      />
      <PlanDialog flow={flow} />
    </div>
  );
}
