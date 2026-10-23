import { ArrowLeft, ArrowRightLeft, ImageDown, Pencil, Replace, ShieldCheck, Trash2 } from "lucide-react";
import { type ReactNode, useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { Link, useNavigate, useParams } from "react-router";

import type { Game } from "@/api/generated/Game";
import type { Media } from "@/api/generated/Media";
import type { NameStyle } from "@/api/generated/NameStyle";
import { EmptyState } from "@/components/EmptyState";
import { NoticeList } from "@/components/Notices";
import { PageScroll } from "@/components/PageScroll";
import { Badge } from "@/components/ui/badge";
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
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { ArtFetchDialog } from "@/features/art/ArtFetchDialog";
import { ArtSlots } from "@/features/art/ArtSlots";
import { GameCover } from "@/features/library/GameCover";
import { ConvertDialog } from "@/features/convert/ConvertDialog";
import { useLibrary } from "@/features/library/useLibrary";
import { PlanDialog } from "@/features/plan/PlanDialog";
import { usePlan } from "@/features/plan/usePlan";
import { DumpBadge } from "@/features/verify/DumpBadge";
import { VerifyDialog } from "@/features/verify/VerifyDialog";
import { formatBytes } from "@/lib/format";
import { findGame, gameRef } from "@/lib/gameKey";
import { useSettings, useSimpleMode } from "@/settings/context";
import { nameStyles } from "@/settings/settings";

/** OPL's compatibility modes, bit by bit (research/02 §6; 0x40/0x80 are RiptOPL's). */
const COMPAT_BITS = [0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80] as const;

export function GamePage() {
  const { t } = useTranslation();
  const { key } = useParams();
  const library = useLibrary();
  const game = library.data && key ? findGame(library.data.games, key) : undefined;

  if (library.root === null) {
    return (
      <PageScroll>
        <EmptyState icon={ArrowLeft} title={t("library.emptyTitle")} description={t("library.emptyDescription")} />
      </PageScroll>
    );
  }
  if (library.isPending) {
    return (
      <PageScroll>
        <p className="text-sm text-muted-foreground" role="status">
          {t("library.scanning")}
        </p>
      </PageScroll>
    );
  }
  if (!game) {
    return (
      <PageScroll className="flex flex-col items-center">
        <EmptyState icon={ArrowLeft} title={t("game.notFoundTitle")} description={t("game.notFoundDescription")} />
        <Button asChild variant="outline">
          <Link to="/library">{t("game.backToLibrary")}</Link>
        </Button>
      </PageScroll>
    );
  }
  return <GameDetails game={game} root={library.root} games={library.data?.games ?? []} />;
}

function Fact({ label, children, mono = false }: { label: string; children: ReactNode; mono?: boolean }) {
  return (
    <div className="contents">
      <dt className="text-muted-foreground">{label}</dt>
      <dd className={mono ? "font-mono wrap-anywhere" : "wrap-anywhere"}>{children}</dd>
    </div>
  );
}

function GameDetails({ game, root, games }: { game: Game; root: string; games: readonly Game[] }) {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { settings } = useSettings();
  const simple = useSimpleMode();
  const ids = useId();
  const [renameOpen, setRenameOpen] = useState(false);
  const [artOpen, setArtOpen] = useState(false);
  const [presetTitle, setPresetTitle] = useState<string | null>(null);
  const [deleteOpen, setDeleteOpen] = useState(false);
  const [verifyOpen, setVerifyOpen] = useState(false);
  const [convertOpen, setConvertOpen] = useState(false);
  const flow = usePlan({
    onExecuted: () => void navigate("/library"),
  });
  const otherMedia: Media = game.media === "CD" ? "DVD" : "CD";
  const compat = game.cfg?.compatibility ?? null;

  return (
    <PageScroll>
      <div className="mx-auto flex max-w-4xl flex-col gap-6">
        <div className="flex items-center gap-2">
          <Button variant="ghost" size="sm" onClick={() => void navigate(-1)}>
            <ArrowLeft aria-hidden />
            {t("common.back")}
          </Button>
        </div>
        <div className="grid gap-6 sm:grid-cols-[12rem_1fr]">
          <div data-testid="game-hero">
            <GameCover game={game} className="w-48" />
          </div>
          <div className="flex min-w-0 flex-col gap-4">
            <div>
              <h2 className="text-2xl font-semibold wrap-anywhere" data-testid="game-title">
                {game.title}
              </h2>
              <div className="mt-2 flex flex-wrap items-center gap-1.5">
                <Badge variant="secondary" className="font-mono">
                  {game.startup}
                </Badge>
                <Badge variant="outline">{game.media}</Badge>
                <Badge variant="outline">{game.format}</Badge>
                <Badge variant="outline">{formatBytes(game.size)}</Badge>
                {game.layer1Start !== null && <Badge variant="outline">DVD-9</Badge>}
              </div>
            </div>
            <div className="flex flex-wrap gap-2" role="group" aria-label={t("game.actions")}>
              <Button onClick={() => setRenameOpen(true)}>
                <Pencil aria-hidden />
                {t("game.rename")}
              </Button>
              <Button
                variant="outline"
                onClick={() => void flow.request({ type: "MoveGame", root, game: gameRef(game), media: otherMedia })}
              >
                <ArrowRightLeft aria-hidden />
                {t("game.moveTo", { media: otherMedia })}
              </Button>
              {!simple && (
                <>
                  <Button variant="outline" onClick={() => setConvertOpen(true)} data-testid="game-convert">
                    <Replace aria-hidden />
                    {t("game.convert")}
                  </Button>
                  <Button variant="outline" onClick={() => setVerifyOpen(true)} data-testid="game-verify">
                    <ShieldCheck aria-hidden />
                    {t("game.verify")}
                  </Button>
                </>
              )}
              <Button variant="destructive" onClick={() => setDeleteOpen(true)}>
                <Trash2 aria-hidden />
                {t("game.delete")}
              </Button>
            </div>
            {game.warnings.length > 0 && (
              <section className="rounded-lg border bg-card p-3" data-testid="game-warnings">
                <h3 className="mb-2 text-sm font-medium">{t("game.warnings")}</h3>
                <NoticeList notices={game.warnings} />
              </section>
            )}
          </div>
        </div>

        <section className="rounded-lg border bg-card p-4">
          <h3 className="mb-3 text-sm font-medium">{t("game.details")}</h3>
          <dl className="grid grid-cols-[max-content_1fr] gap-x-6 gap-y-2 text-sm">
            <Fact label={t("game.id")} mono>
              {game.startup}
            </Fact>
            {!simple && game.imageStartup !== null && game.imageStartup !== game.startup && (
              <Fact label={t("game.imageId")} mono>
                {game.imageStartup}
              </Fact>
            )}
            {game.knownTitle !== null && game.knownTitle !== game.title && (
              <Fact label={t("game.knownTitle")}>
                <span data-testid="known-title">{game.knownTitle}</span>
                <button
                  type="button"
                  className="ml-2 text-xs text-primary underline-offset-2 hover:underline"
                  onClick={() => {
                    setPresetTitle(game.knownTitle);
                    setRenameOpen(true);
                  }}
                >
                  {t("game.useKnownTitle")}
                </button>
              </Fact>
            )}
            <Fact label={t("game.media")}>{game.media}</Fact>
            <Fact label={t("game.format")}>{game.format}</Fact>
            <Fact label={t("game.dump")}>
              {game.dump ? (
                <span className="flex flex-wrap items-center gap-2" data-testid="game-dump">
                  <DumpBadge status={game.dump.status} reason={game.dump.reason} title={game.dump.redump} byTrack={game.dump.byTrack} />
                  {game.dump.redump && <span className="wrap-anywhere">{game.dump.redump}</span>}
                </span>
              ) : (
                <span className="text-muted-foreground" data-testid="game-dump">
                  {t("game.dumpUnchecked")}
                </span>
              )}
            </Fact>
            <Fact label={t("game.size")}>{formatBytes(game.size)}</Fact>
            {!simple && (
              <>
                <Fact label={t("game.path")} mono>
                  {game.path}
                </Fact>
                {game.nameFormat !== null && (
                  <Fact label={t("game.nameFormat")}>{t(`game.nameFormats.${game.nameFormat}`)}</Fact>
                )}
                {game.layer1Start !== null && (
                  <Fact label={t("game.layers")}>{t("game.dualLayer", { sector: game.layer1Start })}</Fact>
                )}
              </>
            )}
          </dl>
        </section>

        {!simple && game.parts.length > 0 && (
          <section className="rounded-lg border bg-card p-4" data-testid="game-parts">
            <h3 className="mb-3 text-sm font-medium">{t("game.parts", { count: game.parts.length })}</h3>
            <table className="w-full text-sm">
              <thead className="text-xs text-muted-foreground">
                <tr>
                  <th className="pb-1 text-left font-medium">{t("game.partName")}</th>
                  <th className="pb-1 text-right font-medium">{t("game.size")}</th>
                  <th className="pb-1 text-right font-medium">{t("game.partState")}</th>
                </tr>
              </thead>
              <tbody>
                {game.parts.map((part) => (
                  <tr key={part.name} className="border-t">
                    <td className="py-1 font-mono text-xs">{part.name}</td>
                    <td className="py-1 text-right font-mono text-xs tabular-nums">
                      {part.size !== null ? formatBytes(part.size) : "—"}
                    </td>
                    <td className="py-1 text-right text-xs">
                      {part.present ? t("game.partPresent") : <span className="text-destructive">{t("game.partMissing")}</span>}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </section>
        )}

        <section className="rounded-lg border bg-card p-4" aria-labelledby={`${ids}-pictures`}>
          <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
            <h3 id={`${ids}-pictures`} className="text-sm font-medium">
              {t("art.pictures")}
            </h3>
            <Button variant="outline" size="sm" onClick={() => setArtOpen(true)} data-testid="game-download-art">
              <ImageDown aria-hidden />
              {t("art.downloadAll")}
            </Button>
          </div>
          <ArtSlots game={game} root={root} />
        </section>

        {!simple && (
          <>
            <section className="rounded-lg border bg-card p-4">
              <h3 className="mb-3 text-sm font-medium">{t("game.settings")}</h3>
              {game.cfg ? (
                <dl className="grid grid-cols-[max-content_1fr] gap-x-6 gap-y-2 text-sm">
                  <Fact label={t("game.cfgFile")} mono>
                    {game.cfg.path}
                  </Fact>
                  {!game.cfg.oplSyntax && <Fact label={t("game.cfgSyntax")}>{t("game.cfgLibconfig")}</Fact>}
                  <Fact label={t("game.compatibility")}>
                    {compat === null || compat === 0 ? (
                      t("game.compatibilityNone")
                    ) : (
                      <ul className="flex flex-wrap gap-1">
                        {COMPAT_BITS.filter((bit) => (compat & bit) !== 0).map((bit) => (
                          <li key={bit}>
                            <Badge variant="outline">{t(`game.compatibilityModes.${bit}`)}</Badge>
                          </li>
                        ))}
                      </ul>
                    )}
                  </Fact>
                  {game.cfg.vmc.map((card, slot) => (
                    // OPL numbers the slots 1 and 2 ($VMC_0 and $VMC_1 in the CFG).
                    <Fact key={slot} label={t("game.vmcSlot", { slot: slot + 1 })} mono>
                      {card ?? <span className="font-sans text-muted-foreground">{t("game.vmcNone")}</span>}
                    </Fact>
                  ))}
                </dl>
              ) : (
                <p className="text-sm text-muted-foreground">{t("game.noCfg")}</p>
              )}
            </section>
            <section className="rounded-lg border bg-card p-4">
              <dl className="grid grid-cols-[max-content_1fr] gap-x-6 gap-y-2 text-sm">
                <Fact label={t("game.cheats")}>
                  {game.cheats === null ? (
                    <span className="text-muted-foreground">{t("game.noCheats")}</span>
                  ) : (
                    t("game.cheatCount", { count: game.cheats })
                  )}
                </Fact>
              </dl>
            </section>
          </>
        )}
      </div>

      <RenameDialog
        game={game}
        open={renameOpen}
        onOpenChange={(open) => {
          setRenameOpen(open);
          if (!open) setPresetTitle(null);
        }}
        presetTitle={presetTitle}
        defaultStyle={settings.nameStyle}
        simple={simple}
        onSubmit={(title, startup, nameStyle) => {
          setRenameOpen(false);
          void flow.request({
            type: "RenameGame",
            root,
            game: gameRef(game),
            title,
            ...(startup ? { startup } : {}),
            nameStyle,
          });
        }}
      />
      <DeleteDialog
        game={game}
        open={deleteOpen}
        onOpenChange={setDeleteOpen}
        onSubmit={(related) => {
          setDeleteOpen(false);
          void flow.request({ type: "DeleteGame", root, game: gameRef(game), related });
        }}
      />
      <VerifyDialog root={root} games={games} only={game} open={verifyOpen} onOpenChange={setVerifyOpen} />
      <ArtFetchDialog root={root} games={games} only={game} open={artOpen} onOpenChange={setArtOpen} quick />
      <ConvertDialog
        root={root}
        games={[game]}
        open={convertOpen}
        onOpenChange={setConvertOpen}
        onExecuted={() => void navigate("/library")}
      />
      <PlanDialog flow={flow} />
    </PageScroll>
  );
}

function RenameDialog({
  game,
  open,
  onOpenChange,
  presetTitle,
  defaultStyle,
  simple,
  onSubmit,
}: {
  game: Game;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** A title to start from, e.g. the database title (META-1). */
  presetTitle?: string | null;
  defaultStyle: NameStyle;
  /** Only the title: fixing the ID and the file name style are advanced. */
  simple: boolean;
  onSubmit: (title: string, startup: string | null, nameStyle: NameStyle) => void;
}) {
  const { t } = useTranslation();
  const id = useId();
  const [title, setTitle] = useState(game.title);
  const [startup, setStartup] = useState(game.startup);
  const [style, setStyle] = useState<NameStyle>(defaultStyle);
  // The dialog mounts closed; a preset title arrives with the open flag.
  const [seenPreset, setSeenPreset] = useState<string | null>(null);
  if (open && presetTitle && presetTitle !== seenPreset) {
    setSeenPreset(presetTitle);
    setTitle(presetTitle);
  }
  if (!open && seenPreset !== null) setSeenPreset(null);
  const isUl = game.format === "UL";
  const maxTitle = isUl ? 32 : 160;
  const idChanged = startup.trim() !== game.startup;
  const canSubmit = title.trim().length > 0 && (title.trim() !== game.title || idChanged || (!isUl && style !== styleOf(game)));

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (next) {
          setTitle(presetTitle ?? game.title);
          setStartup(game.startup);
          setStyle(defaultStyle);
        }
        onOpenChange(next);
      }}
    >
      <DialogContent>
        <form
          className="contents"
          onSubmit={(event) => {
            event.preventDefault();
            if (canSubmit) onSubmit(title.trim(), idChanged ? startup.trim() : null, style);
          }}
        >
          <DialogHeader>
            <DialogTitle>{t("game.renameTitle")}</DialogTitle>
            <DialogDescription>{t("game.renameDescription")}</DialogDescription>
          </DialogHeader>
          <div className="flex flex-col gap-4">
            <div className="flex flex-col gap-1.5">
              <Label htmlFor={`${id}-title`}>{t("game.newTitle")}</Label>
              <Input
                id={`${id}-title`}
                value={title}
                maxLength={maxTitle}
                onChange={(event) => setTitle(event.target.value)}
              />
              <p className="text-xs text-muted-foreground">
                {isUl ? t("game.ulTitleHint") : t("game.titleHint", { max: maxTitle })}
              </p>
            </div>
            {!simple && (
              <div className="flex flex-col gap-1.5">
                <Label htmlFor={`${id}-startup`}>{t("game.fixId")}</Label>
                <Input
                  id={`${id}-startup`}
                  value={startup}
                  maxLength={11}
                  className="font-mono"
                  onChange={(event) => setStartup(event.target.value.toUpperCase())}
                />
                <p className="text-xs text-muted-foreground">{t("game.fixIdHint")}</p>
              </div>
            )}
            {!simple && !isUl && (
              <fieldset className="flex flex-col gap-1.5">
                <legend className="mb-1.5 text-sm font-medium">{t("settings.nameStyle")}</legend>
                <RadioGroup value={style} onValueChange={(value) => setStyle(value as NameStyle)}>
                  {nameStyles.map((option) => (
                    <Label key={option} htmlFor={`${id}-${option}`} className="font-normal">
                      <RadioGroupItem id={`${id}-${option}`} value={option} />
                      {t(`settings.nameStyles.${option}`)}
                    </Label>
                  ))}
                </RadioGroup>
              </fieldset>
            )}
          </div>
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => onOpenChange(false)}>
              {t("common.cancel")}
            </Button>
            <Button type="submit" disabled={!canSubmit}>
              {t("game.previewRename")}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

function styleOf(game: Game): NameStyle {
  return game.nameFormat === "new" ? "title" : "idTitle";
}

function DeleteDialog({
  game,
  open,
  onOpenChange,
  onSubmit,
}: {
  game: Game;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onSubmit: (related: boolean) => void;
}) {
  const { t } = useTranslation();
  const id = useId();
  const [related, setRelated] = useState(false);
  const hasRelated = game.cfg !== null || game.art.length > 0 || game.cheats !== null;
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t("game.deleteTitle", { title: game.title })}</DialogTitle>
          <DialogDescription>{t("game.deleteDescription")}</DialogDescription>
        </DialogHeader>
        <Label htmlFor={`${id}-related`} className="items-start font-normal">
          <Checkbox
            id={`${id}-related`}
            checked={related}
            disabled={!hasRelated}
            onCheckedChange={(value) => setRelated(value === true)}
            className="mt-0.5"
          />
          <span>
            {t("game.deleteRelated")}
            <span className="block text-xs text-muted-foreground">{t("game.deleteRelatedHint")}</span>
          </span>
        </Label>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            {t("common.cancel")}
          </Button>
          <Button variant="destructive" onClick={() => onSubmit(related)} data-testid="delete-preview">
            {t("game.previewDelete")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
