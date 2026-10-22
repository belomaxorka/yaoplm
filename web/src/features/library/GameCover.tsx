import { cn } from "cn";
import { Disc3 } from "lucide-react";

import type { Game } from "@/api/generated/Game";

/** A stable hue per game ID, so a library without covers still has texture. */
function hue(text: string): number {
  let hash = 0;
  for (const char of text) hash = (hash * 31 + char.codePointAt(0)!) >>> 0;
  return hash % 360;
}

/**
 * Cover placeholder until M4 brings art: OPL's COV proportions (140×200),
 * the game ID and the media as text, a tint from the ID.
 */
export function GameCover({ game, className }: { game: Game; className?: string }) {
  const h = hue(game.startup);
  return (
    <div
      className={cn(
        "relative flex aspect-[7/10] w-full flex-col items-center justify-center overflow-hidden rounded-lg border text-center select-none",
        className,
      )}
      style={{
        background: `linear-gradient(160deg, oklch(0.72 0.09 ${h}) 0%, oklch(0.42 0.10 ${(h + 40) % 360}) 100%)`,
      }}
      aria-hidden
    >
      <Disc3 className="size-10 text-white/70 drop-shadow" strokeWidth={1.5} />
      <span className="mt-2 px-2 font-mono text-[0.7rem] font-medium tracking-wide text-white/90 drop-shadow">
        {game.startup}
      </span>
      <span className="absolute top-1.5 left-1.5 rounded bg-black/35 px-1.5 py-0.5 text-[0.65rem] font-semibold text-white">
        {game.media}
      </span>
      <span className="absolute top-1.5 right-1.5 rounded bg-black/35 px-1.5 py-0.5 text-[0.65rem] font-semibold text-white">
        {game.format}
      </span>
    </div>
  );
}
