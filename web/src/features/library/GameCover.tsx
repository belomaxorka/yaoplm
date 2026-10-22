import { cn } from "cn";
import { Disc3 } from "lucide-react";
import { useState } from "react";

import type { Game } from "@/api/generated/Game";
import { artUrl, coverOf } from "@/api/media";

/** A stable hue per game ID, so a library without covers still has texture. */
function hue(text: string): number {
  let hash = 0;
  for (const char of text) hash = (hash * 31 + char.codePointAt(0)!) >>> 0;
  return hash % 360;
}

function Badges({ game }: { game: Game }) {
  return (
    <>
      <span className="absolute top-1.5 left-1.5 rounded bg-black/45 px-1.5 py-0.5 text-[0.65rem] font-semibold text-white">
        {game.media}
      </span>
      <span className="absolute top-1.5 right-1.5 rounded bg-black/45 px-1.5 py-0.5 text-[0.65rem] font-semibold text-white">
        {game.format}
      </span>
    </>
  );
}

/** A small cover for list rows; the disc icon when there is none. */
export function CoverThumb({ game, className }: { game: Game; className?: string }) {
  const cover = coverOf(game);
  const url = cover ? artUrl(cover) : null;
  const [failed, setFailed] = useState<string | null>(null);
  if (url === null || failed === url) {
    return <Disc3 className={cn("size-4 shrink-0 text-muted-foreground", className)} aria-hidden />;
  }
  return (
    <img
      src={url}
      alt=""
      loading="lazy"
      decoding="async"
      draggable={false}
      className={cn("h-10 w-7 shrink-0 rounded-sm border object-cover", className)}
      onError={() => setFailed(url)}
    />
  );
}

/**
 * The game's front cover from `ART/<ID>_COV.png` (ART-2), in OPL's COV
 * proportions (140×200). Without one, or when the file cannot be loaded,
 * the placeholder: the game ID and media, tinted by the ID.
 */
export function GameCover({ game, className }: { game: Game; className?: string }) {
  const cover = coverOf(game);
  const url = cover ? artUrl(cover) : null;
  const [failed, setFailed] = useState<string | null>(null);
  const showImage = url !== null && failed !== url;
  const h = hue(game.startup);
  return (
    <div
      className={cn(
        "relative flex aspect-[7/10] w-full flex-col items-center justify-center overflow-hidden rounded-lg border text-center select-none",
        className,
      )}
      style={
        showImage
          ? undefined
          : {
              background: `linear-gradient(160deg, oklch(0.72 0.09 ${h}) 0%, oklch(0.42 0.10 ${(h + 40) % 360}) 100%)`,
            }
      }
      data-testid="game-cover"
      data-has-cover={showImage ? "true" : "false"}
      aria-hidden
    >
      {showImage ? (
        <img
          src={url}
          alt=""
          loading="lazy"
          decoding="async"
          draggable={false}
          className="size-full object-cover"
          onError={() => setFailed(url)}
        />
      ) : (
        <>
          <Disc3 className="size-10 text-white/70 drop-shadow" strokeWidth={1.5} />
          <span className="mt-2 px-2 font-mono text-[0.7rem] font-medium tracking-wide text-white/90 drop-shadow">
            {game.startup}
          </span>
        </>
      )}
      <Badges game={game} />
    </div>
  );
}
