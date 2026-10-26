import { cn } from "cn";
import { MemoryStick } from "lucide-react";
import { useId } from "react";
import { useTranslation } from "react-i18next";

import type { EmulatorInfo } from "@/api/generated/EmulatorInfo";
import { useSimpleMode } from "@/settings/context";

/**
 * A memory card emulator's SD on the device card (ADR-0048): what it is,
 * its boot channels by their `BootCard.ini` names, and that YAOPLM leaves
 * the emulator's files alone and puts no loader there. Simple mode keeps
 * exactly that; the card counts, `settings.ini` and the folder's place on
 * the SD are details.
 */
export function EmulatorPanel({ emulator, className }: { emulator: EmulatorInfo; className?: string }) {
  const { t } = useTranslation();
  const simple = useSimpleMode();
  // One per card: the page lists every SD the computer sees.
  const titleId = useId();
  const kinds = emulator.kinds.map((kind) => t(`devices.emulator.kinds.${kind}`)).join(", ");
  return (
    <section
      className={cn("flex flex-col gap-2 rounded-md border bg-card p-3 text-sm", className)}
      aria-labelledby={titleId}
      data-testid="emulator-panel"
    >
      <div className="flex flex-wrap items-center gap-2">
        <MemoryStick className="size-4 shrink-0 text-muted-foreground" aria-hidden />
        <h3 id={titleId} className="font-medium">
          {t("devices.emulator.title")}
        </h3>
        {kinds && <span className="text-muted-foreground">— {kinds}</span>}
      </div>
      <p className="text-muted-foreground" data-testid="emulator-protected">
        {t("devices.emulator.protected", { files: emulator.protected.join(", ") })}
      </p>
      {!simple && emulator.folder && (
        <p className="text-xs text-muted-foreground">{t("devices.emulator.folder", { folder: emulator.folder })}</p>
      )}
      {emulator.variants.map((variant) => (
        <div key={variant.name} className="flex flex-col gap-1" data-testid={`emulator-variant-${variant.name}`}>
          <div className="text-xs font-medium text-muted-foreground">
            {t("devices.emulator.bootCards", { variant: variant.name })}
          </div>
          {variant.boot.length === 0 ? (
            <p className="text-xs text-muted-foreground">{t("devices.emulator.noBootCards")}</p>
          ) : (
            <ol className="flex flex-col gap-0.5">
              {variant.boot.map((boot) => (
                <li key={boot.channel} className="flex flex-wrap gap-x-2" data-testid="emulator-boot-channel">
                  <span className="font-mono text-xs tabular-nums text-muted-foreground">
                    {t("devices.emulator.channel", { channel: boot.channel })}
                  </span>
                  <span>{boot.name ?? t("devices.emulator.noName")}</span>
                  {!simple && <span className="font-mono text-xs break-all text-muted-foreground">{boot.file}</span>}
                </li>
              ))}
            </ol>
          )}
          {!simple && (
            <p className="text-xs text-muted-foreground" data-testid="emulator-cards">
              {t("devices.emulator.cards", { cards: variant.cards, games: variant.gameCards })}
              {variant.maxChannels !== null && ` · MaxChannels=${variant.maxChannels}`}
              {variant.cardSize !== null && ` · CardSize=${variant.cardSize}`}
            </p>
          )}
        </div>
      ))}
      {!simple && emulator.settings.length > 0 && (
        <div className="flex flex-col gap-0.5 text-xs text-muted-foreground" data-testid="emulator-settings">
          <div className="font-medium">.sd2psx/settings.ini</div>
          {emulator.settings.map((section) => (
            <div key={section.name} className="font-mono break-all">
              [{section.name}] {section.entries.map((e) => `${e.key}=${e.value}`).join(" ")}
            </div>
          ))}
        </div>
      )}
    </section>
  );
}
