import { useQuery } from "@tanstack/react-query";
import { cn } from "cn";
import { Plus, Save } from "lucide-react";
import { type ReactNode, useId, useState } from "react";
import { useTranslation } from "react-i18next";

import type { CfgView } from "@/api/generated/CfgView";
import type { Game } from "@/api/generated/Game";
import type { GameSettings } from "@/api/generated/GameSettings";
import type { NewVmc } from "@/api/generated/NewVmc";
import type { VmcCard } from "@/api/generated/VmcCard";
import { useApi } from "@/api/context";
import { listVmc, readCfg } from "@/api/rpc";
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
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { PlanDialog } from "@/features/plan/PlanDialog";
import { usePlan } from "@/features/plan/usePlan";
import { errorText, phraseText } from "@/lib/phrase";
import { gameKey, gameRef } from "@/lib/gameKey";
import { useLoaderProfile } from "@/features/devices/useLoaderProfile";
import { useSimpleMode } from "@/settings/context";

/** OPL's compatibility modes, bit by bit (research/02 §6.3); 0x40 and 0x80 are RiptOPL's. */
const MODE_BITS = [0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80] as const;
/** OPL's GSM video modes 0 to 28 (`src/gsm.c`). */
const GSM_MODES = [
  0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28,
] as const;
/** OPL's menu keeps GSM offsets within ±100 (`diaGSConfig` in `src/dialogs.c`). */
const GSM_OFFSET_MAX = 100;
const VMC_SIZES = [8, 16, 32, 64] as const;
/** `$AltStartup` must fit `cdrom0:\<name>;1` in OPL's 32-byte buffer. */
const ALT_STARTUP_MAX = 21;
/** A Select item cannot be empty: this stands for an empty slot. */
const NO_CARD = "\u0000none";

/** `$PadEmuSettings` bits (`guiGamePadEmuUpdater()`): bit 0 Bluetooth, 8 + port emulation, 16 + port vibration. */
const PAD_BLUETOOTH = 1;
const padPort = (port: number) => 1 << (8 + port);
const padVibration = (port: number) => 1 << (16 + port);

function sameName(a: string, b: string): boolean {
  return a.toLowerCase() === b.toLowerCase();
}

/**
 * A game's settings (CFG-2) and memory cards (VMC-1). Simple mode shows the
 * compatibility modes and the card in slot 1; the rest is advanced (ADR-0036).
 * Nothing is written until the plan is confirmed: Save asks the core for a
 * plan that lists each key before and after.
 */
export function GameSettingsCard({ game, root }: { game: Game; root: string }) {
  const { t } = useTranslation();
  const { transport } = useApi();
  const simple = useSimpleMode();
  const { profile: loaderProfile } = useLoaderProfile();
  const ref = gameRef(game);
  const cfg = useQuery({
    queryKey: ["gameCfg", root, gameKey(game), game.startup],
    queryFn: () => readCfg(transport, root, ref),
  });
  const cards = useQuery({ queryKey: ["vmcs", root], queryFn: () => listVmc(transport, root) });
  const [draft, setDraft] = useState<GameSettings | null>(null);
  const [newCards, setNewCards] = useState<Record<number, NewVmc>>({});
  // Once written, the file is the truth again, even if it reads the same.
  const flow = usePlan({
    onExecuted: () => {
      setDraft(null);
      setNewCards({});
    },
  });
  const ids = useId();

  const base = cfg.data?.settings ?? null;
  // A new read of the file (after a write or a rescan) starts over.
  const [seen, setSeen] = useState(base);
  if (base !== seen) {
    setSeen(base);
    setDraft(null);
    setNewCards({});
  }

  if (cfg.isPending) {
    return (
      <Section>
        <p className="text-sm text-muted-foreground" role="status">
          {t("cfg.loading")}
        </p>
      </Section>
    );
  }
  if (cfg.isError || !cfg.data || !base) {
    return (
      <Section>
        <p className="text-sm text-destructive" role="alert">
          {errorText(cfg.error)}
        </p>
      </Section>
    );
  }
  const view = cfg.data;
  if (!view.editable) {
    return (
      <Section>
        <p className="text-sm text-muted-foreground" data-testid="cfg-libconfig">
          {t("cfg.libconfig", { path: view.path })}
        </p>
      </Section>
    );
  }

  const current = draft ?? base;
  const set = (change: Partial<GameSettings>) => setDraft({ ...current, ...change });
  // Only the cards a slot still names are created.
  const pending = Object.entries(newCards)
    .filter(([slot, card]) => current.vmc[Number(slot)] === card.name)
    .map(([, card]) => card);
  const dirty = JSON.stringify(current) !== JSON.stringify(base) || pending.length > 0;
  // Like the core, only a changed offset has to be in range.
  const offsetOutOfRange = (["xOffset", "yOffset"] as const).some(
    (axis) => current.gsm[axis] !== base.gsm[axis] && Math.abs(current.gsm[axis]) > GSM_OFFSET_MAX,
  );
  const shownBits = loaderProfile === "RiptOpl" ? MODE_BITS : MODE_BITS.slice(0, 6);
  let shownMask = 0;
  for (const bit of shownBits) shownMask |= bit;
  const otherBits = current.compatibility & ~shownMask;

  const setSlot = (slot: number, name: string, card?: NewVmc) => {
    const vmc = [...current.vmc];
    vmc[slot] = name;
    set({ vmc });
    setNewCards((cards) => {
      const next = { ...cards };
      if (card) next[slot] = card;
      else delete next[slot];
      return next;
    });
  };

  return (
    <Section>
      <div className="flex flex-col gap-5">
        <fieldset className="flex flex-col gap-2" data-testid="cfg-modes">
          <legend className="mb-1 text-sm font-medium">{t("cfg.modes")}</legend>
          <p className="text-xs text-muted-foreground">{t("cfg.modesHint")}</p>
          <div className="grid gap-x-6 gap-y-2 sm:grid-cols-2">
            {shownBits.map((bit) => (
              <Label key={bit} htmlFor={`${ids}-mode-${bit}`} className="items-start font-normal">
                <Checkbox
                  id={`${ids}-mode-${bit}`}
                  checked={(current.compatibility & bit) !== 0}
                  onCheckedChange={(on) =>
                    set({ compatibility: on === true ? current.compatibility | bit : current.compatibility & ~bit })
                  }
                  className="mt-0.5"
                />
                <span>
                  {t(`game.compatibilityModes.${bit}`)}
                  <span className="block text-xs text-muted-foreground">{t(`cfg.modeHints.${bit}`)}</span>
                </span>
              </Label>
            ))}
          </div>
          {otherBits !== 0 && (
            <p className="text-xs text-muted-foreground" data-testid="cfg-other-bits">
              {t("cfg.otherBits", { bits: `0x${otherBits.toString(16).toUpperCase().padStart(2, "0")}` })}
            </p>
          )}
        </fieldset>

        {[0, 1]
          .filter((slot) => !simple || slot === 0)
          .map((slot) => (
            <VmcSlot
              key={slot}
              slot={slot}
              view={view}
              name={current.vmc[slot] ?? ""}
              cards={cards.data ?? []}
              pending={newCards[slot] ?? null}
              onChange={(name, card) => setSlot(slot, name, card)}
            />
          ))}

        {!simple && (
          <>
            <SourceGroup
              title={t("cfg.gsm")}
              testId="cfg-gsm"
              perGame={current.gsm.perGame}
              onPerGame={(perGame) => set({ gsm: { ...current.gsm, perGame } })}
            >
              <ToggleRow
                label={t("cfg.gsmEnabled")}
                checked={current.gsm.enabled}
                disabled={!current.gsm.perGame}
                onChange={(enabled) => set({ gsm: { ...current.gsm, enabled } })}
              />
              <div className="flex flex-col gap-1.5">
                <Label htmlFor={`${ids}-vmode`}>{t("cfg.gsmVmode")}</Label>
                <Select
                  value={String(current.gsm.vmode)}
                  disabled={!current.gsm.perGame}
                  onValueChange={(value) => set({ gsm: { ...current.gsm, vmode: Number(value) } })}
                >
                  <SelectTrigger id={`${ids}-vmode`} className="w-full sm:w-80">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {/* A value OPL has no name for still shows as itself. */}
                    {!(GSM_MODES as readonly number[]).includes(current.gsm.vmode) && (
                      <SelectItem value={String(current.gsm.vmode)}>{String(current.gsm.vmode)}</SelectItem>
                    )}
                    {GSM_MODES.map((mode) => (
                      <SelectItem key={mode} value={String(mode)}>
                        {t(`cfg.gsmModes.${mode}`)}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
              <div className="grid grid-cols-2 gap-3 sm:w-80">
                {(["xOffset", "yOffset"] as const).map((axis) => (
                  <div key={axis} className="flex flex-col gap-1.5">
                    <Label htmlFor={`${ids}-${axis}`}>{t(`cfg.${axis}`)}</Label>
                    <OffsetInput
                      id={`${ids}-${axis}`}
                      value={current.gsm[axis]}
                      disabled={!current.gsm.perGame}
                      onChange={(value) => set({ gsm: { ...current.gsm, [axis]: value } })}
                    />
                  </div>
                ))}
              </div>
              {offsetOutOfRange && (
                <p className="text-xs text-destructive" role="alert" data-testid="cfg-offset-range">
                  {t("cfg.offsetRange", { max: GSM_OFFSET_MAX })}
                </p>
              )}
              <ToggleRow
                label={t("cfg.gsmFieldFix")}
                hint={t("cfg.gsmFieldFixHint")}
                checked={current.gsm.fieldFix}
                disabled={!current.gsm.perGame}
                onChange={(fieldFix) => set({ gsm: { ...current.gsm, fieldFix } })}
              />
            </SourceGroup>

            <SourceGroup
              title={t("cfg.cheats")}
              testId="cfg-cheats"
              perGame={current.cheats.perGame}
              onPerGame={(perGame) => set({ cheats: { ...current.cheats, perGame } })}
            >
              <ToggleRow
                label={t("cfg.cheatsEnabled")}
                hint={t("cfg.cheatsHint")}
                checked={current.cheats.enabled}
                disabled={!current.cheats.perGame}
                onChange={(enabled) => set({ cheats: { ...current.cheats, enabled } })}
              />
              <RadioGroup
                value={String(current.cheats.mode)}
                disabled={!current.cheats.perGame}
                onValueChange={(value) => set({ cheats: { ...current.cheats, mode: Number(value) } })}
                aria-label={t("cfg.cheatMode")}
                className={cn(!current.cheats.perGame && "opacity-60")}
              >
                {([0, 1] as const).map((mode) => (
                  <Label key={mode} htmlFor={`${ids}-cheat-${mode}`} className="font-normal">
                    <RadioGroupItem id={`${ids}-cheat-${mode}`} value={String(mode)} />
                    {t(`cfg.cheatModes.${mode}`)}
                  </Label>
                ))}
              </RadioGroup>
            </SourceGroup>

            <SourceGroup
              title={t("cfg.padEmu")}
              testId="cfg-pademu"
              perGame={current.padEmu.perGame}
              onPerGame={(perGame) => set({ padEmu: { ...current.padEmu, perGame } })}
            >
              <ToggleRow
                label={t("cfg.padEmuEnabled")}
                hint={t("cfg.padEmuHint")}
                checked={current.padEmu.enabled}
                disabled={!current.padEmu.perGame}
                onChange={(enabled) => set({ padEmu: { ...current.padEmu, enabled } })}
              />
              <RadioGroup
                value={(current.padEmu.settings & 0xff) !== 0 ? "bt" : "usb"}
                disabled={!current.padEmu.perGame}
                className={cn(!current.padEmu.perGame && "opacity-60")}
                onValueChange={(value) => {
                  const bits = current.padEmu.settings & ~0xff;
                  set({ padEmu: { ...current.padEmu, settings: value === "bt" ? bits | PAD_BLUETOOTH : bits } });
                }}
                aria-label={t("cfg.padEmuMode")}
              >
                {(["usb", "bt"] as const).map((mode) => (
                  <Label key={mode} htmlFor={`${ids}-pad-${mode}`} className="font-normal">
                    <RadioGroupItem id={`${ids}-pad-${mode}`} value={mode} />
                    {t(`cfg.padEmuModes.${mode}`)}
                  </Label>
                ))}
              </RadioGroup>
              <div className={cn("grid grid-cols-2 gap-x-6 gap-y-2 sm:w-96", !current.padEmu.perGame && "opacity-60")}>
                {[0, 1].flatMap((port) =>
                  [padPort(port), padVibration(port)].map((bit, index) => (
                    <Label key={bit} htmlFor={`${ids}-pad-${bit}`} className="font-normal">
                      <Checkbox
                        id={`${ids}-pad-${bit}`}
                        checked={(current.padEmu.settings & bit) !== 0}
                        disabled={!current.padEmu.perGame}
                        onCheckedChange={(on) =>
                          set({
                            padEmu: {
                              ...current.padEmu,
                              settings: on === true ? current.padEmu.settings | bit : current.padEmu.settings & ~bit,
                            },
                          })
                        }
                      />
                      {t(index === 0 ? "cfg.padPort" : "cfg.padVibration", { port: port + 1 })}
                    </Label>
                  )),
                )}
              </div>
            </SourceGroup>

            <div className="flex flex-col gap-1.5" data-testid="cfg-alt-startup">
              <Label htmlFor={`${ids}-alt`}>{t("cfg.altStartup")}</Label>
              <Input
                id={`${ids}-alt`}
                value={current.altStartup}
                maxLength={Math.max(ALT_STARTUP_MAX, current.altStartup.length)}
                className="font-mono sm:w-80"
                onChange={(event) => set({ altStartup: event.target.value.toUpperCase() })}
              />
              <p className="text-xs text-muted-foreground">{t("cfg.altStartupHint", { max: ALT_STARTUP_MAX })}</p>
            </div>

            <details className="text-sm" data-testid="cfg-keys">
              <summary className="cursor-pointer text-xs text-muted-foreground select-none hover:text-foreground">
                {t("cfg.keys", { count: view.entries.length })}
              </summary>
              <p className="mt-2 font-mono text-xs wrap-anywhere text-muted-foreground">
                {view.exists ? view.path : t("cfg.notThereYet", { path: view.path })}
              </p>
              {view.entries.length > 0 && (
                <ul className="mt-1 flex flex-col gap-0.5 font-mono text-xs">
                  {view.entries.map((entry) => (
                    <li key={entry.key} className="wrap-anywhere">
                      {entry.key}={entry.value}
                    </li>
                  ))}
                </ul>
              )}
            </details>
          </>
        )}

        <div className="flex flex-wrap items-center gap-2">
          <Button
            disabled={!dirty || offsetOutOfRange || flow.pending}
            onClick={() =>
              void flow.request({
                type: "WriteCfg",
                root,
                game: ref,
                profile: loaderProfile,
                settings: current,
                ...(pending.length > 0 ? { cards: pending } : {}),
              })
            }
            data-testid="cfg-save"
          >
            <Save aria-hidden />
            {t("cfg.save")}
          </Button>
          {dirty && (
            <Button
              variant="ghost"
              onClick={() => {
                setDraft(null);
                setNewCards({});
              }}
            >
              {t("cfg.reset")}
            </Button>
          )}
        </div>
      </div>
      <PlanDialog flow={flow} />
    </Section>
  );
}

function Section({ children }: { children: ReactNode }) {
  const { t } = useTranslation();
  const id = useId();
  return (
    <section className="rounded-lg border bg-card p-4" aria-labelledby={id} data-testid="game-settings">
      <h3 id={id} className="mb-3 text-sm font-medium">
        {t("cfg.heading")}
      </h3>
      {children}
    </section>
  );
}

function ToggleRow({
  label,
  hint,
  checked,
  disabled,
  onChange,
}: {
  label: string;
  hint?: string;
  checked: boolean;
  disabled?: boolean;
  onChange: (checked: boolean) => void;
}) {
  const id = useId();
  return (
    <div className="flex items-start gap-3">
      <Switch id={id} checked={checked} disabled={disabled} onCheckedChange={onChange} className="mt-0.5" />
      <Label htmlFor={id} className="flex-col items-start gap-0.5 font-normal">
        {label}
        {hint && <span className="text-xs text-muted-foreground">{hint}</span>}
      </Label>
    </div>
  );
}

/**
 * A whole number typed as text: a number box would turn a lone "-" into 0
 * and make a negative shift impossible to type. What is not a number yet
 * stays in the box until it loses focus.
 */
function OffsetInput({
  id,
  value,
  disabled,
  onChange,
}: {
  id: string;
  value: number;
  disabled: boolean;
  onChange: (value: number) => void;
}) {
  const [text, setText] = useState(String(value));
  const [shown, setShown] = useState(value);
  if (value !== shown) {
    setShown(value);
    setText(String(value));
  }
  return (
    <Input
      id={id}
      inputMode="numeric"
      value={text}
      disabled={disabled}
      aria-invalid={Math.abs(value) > GSM_OFFSET_MAX || undefined}
      onChange={(event) => {
        const next = event.target.value.replace("−", "-");
        if (!/^-?\d{0,4}$/.test(next)) return;
        setText(next);
        if (/\d/.test(next)) onChange(Number(next));
      }}
      onBlur={() => setText(String(value))}
    />
  );
}

/** GSM, cheats or PADEMU: OPL's global setting, or this game's own. */
function SourceGroup({
  title,
  testId,
  perGame,
  onPerGame,
  children,
}: {
  title: string;
  testId: string;
  perGame: boolean;
  onPerGame: (perGame: boolean) => void;
  children: ReactNode;
}) {
  const { t } = useTranslation();
  const id = useId();
  return (
    <fieldset className="flex flex-col gap-3" data-testid={testId}>
      <legend className="mb-1 text-sm font-medium">{title}</legend>
      <RadioGroup
        value={perGame ? "game" : "global"}
        onValueChange={(value) => onPerGame(value === "game")}
        className="flex flex-wrap gap-x-6"
        aria-label={t("cfg.source", { title })}
      >
        {(["global", "game"] as const).map((source) => (
          <Label key={source} htmlFor={`${id}-${source}`} className="font-normal">
            <RadioGroupItem id={`${id}-${source}`} value={source} />
            {t(`cfg.sources.${source}`)}
          </Label>
        ))}
      </RadioGroup>
      {children}
    </fieldset>
  );
}

/** One VMC slot: a card from VMC/, none, or a new one (VMC-1). */
function VmcSlot({
  slot,
  view,
  name,
  cards,
  pending,
  onChange,
}: {
  slot: number;
  view: CfgView;
  name: string;
  cards: readonly VmcCard[];
  pending: NewVmc | null;
  onChange: (name: string, card?: NewVmc) => void;
}) {
  const { t } = useTranslation();
  const simple = useSimpleMode();
  const id = useId();
  const [creating, setCreating] = useState(false);
  const defaultName = view.cardNames[slot] ?? `${view.startup}_${slot}`;
  const existing = cards.find((card) => sameName(card.name, defaultName));
  const isPending = pending !== null && pending.name === name;
  const listed = cards.some((card) => sameName(card.name, name));
  const label = (card: VmcCard) =>
    isPending && sameName(card.name, name)
      ? t("cfg.cardReplacing", { name: card.name, size: pending.sizeMb })
      : card.sizeMb !== null
        ? t("cfg.cardSize", { name: card.name, size: card.sizeMb })
        : t("cfg.cardRefused", { name: card.name });

  return (
    <div className="flex flex-col gap-1.5" data-testid={`cfg-vmc-${slot}`}>
      <Label htmlFor={`${id}-select`}>{t("game.vmcSlot", { slot: slot + 1 })}</Label>
      <div className="flex flex-wrap items-center gap-2">
        <Select
          value={name === "" ? NO_CARD : name}
          onValueChange={(value) => onChange(value === NO_CARD ? "" : value)}
        >
          <SelectTrigger id={`${id}-select`} className="w-full max-w-full sm:w-auto sm:min-w-80">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value={NO_CARD}>{t("cfg.noCard")}</SelectItem>
            {cards.map((card) => (
              <SelectItem key={card.name} value={card.name}>
                {label(card)}
              </SelectItem>
            ))}
            {name !== "" && !listed && (
              <SelectItem value={name}>
                {isPending
                  ? t("cfg.cardNew", { name, size: pending.sizeMb })
                  : t("cfg.cardMissing", { name })}
              </SelectItem>
            )}
          </SelectContent>
        </Select>
        {simple ? (
          <Button
            variant="outline"
            disabled={isPending || (existing?.sizeMb !== undefined && existing.sizeMb !== null)}
            onClick={() =>
              // A file OPL refuses (zero-filled, wrong size) is replaced: it has no saves.
              onChange(defaultName, { name: defaultName, sizeMb: 8, ...(existing ? { replace: true } : {}) })
            }
            data-testid={`cfg-vmc-create-${slot}`}
          >
            <Plus aria-hidden />
            {t("cfg.createCard")}
          </Button>
        ) : (
          <Button variant="outline" onClick={() => setCreating(true)} data-testid={`cfg-vmc-create-${slot}`}>
            <Plus aria-hidden />
            {t("cfg.createCardEllipsis")}
          </Button>
        )}
      </div>
      {simple && existing?.sizeMb !== undefined && existing.sizeMb !== null && name !== existing.name && (
        <p className="text-xs text-muted-foreground">{t("cfg.cardExists", { name: existing.name })}</p>
      )}
      {(() => {
        const chosen = cards.find((card) => sameName(card.name, name));
        if (chosen && chosen.sizeMb === null && !isPending) {
          return (
            <p className="text-xs text-destructive">
              {t("cfg.cardRefusedWhy", {
                name: chosen.name,
                reason: phraseText(chosen.problemPhrase, chosen.problem ?? ""),
              })}
            </p>
          );
        }
        const users = chosen?.usedBy.filter((user) => user.startup.toLowerCase() !== view.startup.toLowerCase()) ?? [];
        return users.length > 0 ? (
          <p className="text-xs text-muted-foreground">
            {t("cfg.cardShared", { games: users.map((user) => user.startup).join(", ") })}
          </p>
        ) : null;
      })()}
      <NewCardDialog
        open={creating}
        onOpenChange={setCreating}
        defaultName={defaultName}
        cards={cards}
        onCreate={(card) => {
          setCreating(false);
          onChange(card.name, card);
        }}
      />
    </div>
  );
}

function NewCardDialog({
  open,
  onOpenChange,
  defaultName,
  cards,
  onCreate,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  defaultName: string;
  cards: readonly VmcCard[];
  onCreate: (card: NewVmc) => void;
}) {
  const { t } = useTranslation();
  const id = useId();
  const [name, setName] = useState(defaultName);
  const [size, setSize] = useState<number>(8);
  const [replace, setReplace] = useState(false);
  const clean = name.trim();
  const taken = cards.find((card) => sameName(card.name, clean));
  const valid = clean.length > 0 && clean.length <= 31 && /^[\x20-\x7e]+$/.test(clean) && !/["*/:<>?\\|]/.test(clean);
  const canCreate = valid && (!taken || replace);
  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (next) {
          setName(defaultName);
          setSize(8);
          setReplace(false);
        }
        onOpenChange(next);
      }}
    >
      <DialogContent>
        <form
          className="contents"
          onSubmit={(event) => {
            event.preventDefault();
            if (canCreate) onCreate({ name: clean, sizeMb: size, ...(taken ? { replace: true } : {}) });
          }}
        >
          <DialogHeader>
            <DialogTitle>{t("cfg.newCardTitle")}</DialogTitle>
            <DialogDescription>{t("cfg.newCardDescription")}</DialogDescription>
          </DialogHeader>
          <div className="flex flex-col gap-4">
            <div className="flex flex-col gap-1.5">
              <Label htmlFor={`${id}-name`}>{t("cfg.cardName")}</Label>
              <Input
                id={`${id}-name`}
                value={name}
                maxLength={31}
                className="font-mono"
                aria-invalid={(clean.length > 0 && !valid) || undefined}
                aria-describedby={`${id}-name-hint`}
                onChange={(event) => setName(event.target.value)}
              />
              <p id={`${id}-name-hint`} className="text-xs text-muted-foreground">
                {t("cfg.cardNameHint")}
              </p>
              {clean.length > 0 && !valid && (
                <p className="text-xs text-destructive" role="alert" data-testid="cfg-card-name-invalid">
                  {t("cfg.cardNameInvalid")}
                </p>
              )}
            </div>
            <fieldset className="flex flex-col gap-1.5">
              <legend className="mb-1.5 text-sm font-medium">{t("cfg.cardSizeLabel")}</legend>
              <RadioGroup value={String(size)} onValueChange={(value) => setSize(Number(value))} className="flex gap-4">
                {VMC_SIZES.map((mb) => (
                  <Label key={mb} htmlFor={`${id}-${mb}`} className="font-normal">
                    <RadioGroupItem id={`${id}-${mb}`} value={String(mb)} />
                    {t("cfg.mib", { size: mb })}
                  </Label>
                ))}
              </RadioGroup>
            </fieldset>
            {taken && (
              <Label htmlFor={`${id}-replace`} className="items-start font-normal text-destructive">
                <Checkbox
                  id={`${id}-replace`}
                  checked={replace}
                  onCheckedChange={(value) => setReplace(value === true)}
                  className="mt-0.5"
                />
                <span>{t("cfg.replaceCard", { name: taken.name })}</span>
              </Label>
            )}
          </div>
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => onOpenChange(false)}>
              {t("common.cancel")}
            </Button>
            <Button type="submit" disabled={!canCreate} data-testid="cfg-vmc-new">
              {t("cfg.useNewCard")}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
