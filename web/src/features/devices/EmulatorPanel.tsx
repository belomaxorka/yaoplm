import { cn } from "cn";
import { ChevronRight, MemoryStick } from "lucide-react";
import { useId } from "react";
import { useTranslation } from "react-i18next";

import type { BootContents } from "@/api/generated/BootContents";
import type { EmulatorInfo } from "@/api/generated/EmulatorInfo";
import { Badge } from "@/components/ui/badge";
import { useSimpleMode } from "@/settings/context";

/**
 * What a boot card holds by its content (research/19 §4): the SD Builder
 * image it is or the launchers it starts, in both modes, since that is what
 * the channel-name warnings compare with; the OPL builds inside, with their
 * versions and MMCE, in advanced mode.
 */
function BootHolds({ contents, simple }: { contents: BootContents; simple: boolean }) {
  const { t } = useTranslation();
  if (contents.unreadable !== null) {
    return <span className="ml-1 text-xs text-muted-foreground">{t("devices.emulator.unreadable")}</span>;
  }
  const what =
    contents.builder !== null ? t("devices.emulator.builderImage", { name: contents.builder }) : contents.launchers.join(" + ");
  return (
    <div className="ml-1 flex flex-col text-xs text-muted-foreground" data-testid="emulator-boot-holds">
      {what && <span>{t("devices.emulator.holds", { what })}</span>}
      {!simple &&
        contents.loaders.map((loader) => (
          <span key={loader.path} className="flex flex-wrap items-center gap-x-1.5" data-testid="emulator-boot-loader">
            <span>
              {loader.family} {loader.version ?? t("devices.emulator.versionUnknown")}
            </span>
            {loader.mmce && (
              <Badge variant="outline" className="h-4 px-1 text-[10px]">
                {t("devices.emulator.mmce")}
              </Badge>
            )}
            <span className="selectable font-mono break-all">{loader.path}</span>
          </span>
        ))}
    </div>
  );
}

/**
 * A memory card emulator's SD on the device card (ADR-0048): what it is,
 * its boot channels by their `BootCard.ini` names, and that YAOPLM leaves
 * the emulator's files alone and puts no loader there. Simple mode keeps
 * exactly that; the card counts, `settings.ini` and the folder's place on
 * the SD are details.
 *
 * Folded by default (the owner, 2026-10-27, UI-7): the summary line says
 * what the SD is and counts the boot cards of every variant; the channels
 * are a click away. Whether it is open is not remembered.
 */
export function EmulatorPanel({ emulator, className }: { emulator: EmulatorInfo; className?: string }) {
  const { t } = useTranslation();
  const simple = useSimpleMode();
  // One per card: the page lists every SD the computer sees.
  const titleId = useId();
  const kinds = emulator.kinds.map((kind) => t(`devices.emulator.kinds.${kind}`)).join(", ");
  const counts = emulator.variants.map((variant) => `${variant.name} — ${variant.boot.length}`).join(", ");
  return (
    <section
      className={cn("rounded-md border bg-card px-3 py-2 text-sm", className)}
      aria-labelledby={titleId}
      data-testid="emulator-panel"
    >
      <details className="group" data-testid="emulator-details">
        <summary
          className="flex cursor-pointer list-none flex-wrap items-center gap-x-2 gap-y-0.5 select-none [&::-webkit-details-marker]:hidden"
          data-testid="emulator-summary"
        >
          <ChevronRight className="size-4 shrink-0 text-muted-foreground transition-transform group-open:rotate-90" aria-hidden />
          <MemoryStick className="size-4 shrink-0 text-muted-foreground" aria-hidden />
          <h3 id={titleId} className="font-medium">
            {t("devices.emulator.title")}
          </h3>
          {kinds && <span className="text-muted-foreground">— {kinds}</span>}
          {counts && (
            <span className="basis-full pl-12 text-xs text-muted-foreground">
              {t("devices.emulator.summary", { counts })}
            </span>
          )}
        </summary>
        <div className="mt-2 flex flex-col gap-2">
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
                <li key={boot.channel} className="flex flex-col" data-testid="emulator-boot-channel">
                  <div className="flex flex-wrap gap-x-2">
                    <span className="font-mono text-xs tabular-nums text-muted-foreground">
                      {t("devices.emulator.channel", { channel: boot.channel })}
                    </span>
                    <span>{boot.name ?? t("devices.emulator.noName")}</span>
                    {!simple && <span className="selectable font-mono text-xs break-all text-muted-foreground">{boot.file}</span>}
                  </div>
                  {boot.contents && <BootHolds contents={boot.contents} simple={simple} />}
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
            <div key={section.name} className="selectable font-mono break-all">
              [{section.name}] {section.entries.map((e) => `${e.key}=${e.value}`).join(" ")}
            </div>
          ))}
        </div>
      )}
        </div>
      </details>
    </section>
  );
}
