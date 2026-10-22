import { useVirtualizer } from "@tanstack/react-virtual";
import { cn } from "cn";
import {
  AlertTriangle,
  ArrowDownAZ,
  ArrowUpZA,
  Disc3,
  FolderPlus,
  HardDrive,
  ImageDown,
  ImageOff,
  LayoutGrid,
  List,
  Plus,
  RefreshCw,
  Search,
  X,
} from "lucide-react";
import { type KeyboardEvent, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Link, useNavigate, useSearchParams } from "react-router";

import type { Game } from "@/api/generated/Game";
import type { Library } from "@/api/generated/Library";
import { hasCover } from "@/api/media";
import { EmptyState } from "@/components/EmptyState";
import { NoticeList } from "@/components/Notices";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Progress } from "@/components/ui/progress";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Toggle } from "@/components/ui/toggle";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { ArtFetchDialog } from "@/features/art/ArtFetchDialog";
import { CoverThumb, GameCover } from "@/features/library/GameCover";
import {
  applyFilters,
  filtersToParams,
  type LibraryFilters,
  parseFilters,
  type SortKey,
  sortKeys,
} from "@/features/library/filters";
import { useLibrary } from "@/features/library/useLibrary";
import { PlanDialog } from "@/features/plan/PlanDialog";
import { usePlan } from "@/features/plan/usePlan";
import { RecoveryBanner } from "@/features/recovery/RecoveryBanner";
import { useJobs } from "@/jobs/context";
import { formatBytes, percent } from "@/lib/format";
import { gameKey } from "@/lib/gameKey";
import { useSettings } from "@/settings/context";
import type { ViewMode } from "@/settings/settings";

const GRID_MIN_COLUMN = 168;
const GRID_GAP = 16;
const GRID_ROW_HEIGHT = 300;
const LIST_ROW_HEIGHT = 56;

export function LibraryPage() {
  const { t } = useTranslation();
  const { settings, update } = useSettings();
  const library = useLibrary();
  const navigate = useNavigate();

  if (library.root === null) {
    return (
      <div className="flex flex-1 flex-col items-center justify-center p-6">
        <EmptyState icon={Disc3} title={t("library.emptyTitle")} description={t("library.emptyDescription")} />
        <Button onClick={() => void navigate("/devices")}>
          <HardDrive aria-hidden />
          {t("library.chooseDevice")}
        </Button>
      </div>
    );
  }

  return (
    <LibraryView
      root={library.root}
      data={library.data}
      isPending={library.isPending}
      isFetching={library.isFetching}
      error={library.error ? library.error.message : null}
      rescan={() => void library.rescan()}
      view={settings.view}
      setView={(view) => update({ view })}
    />
  );
}

function LibraryView({
  root,
  data,
  isPending,
  isFetching,
  error,
  rescan,
  view,
  setView,
}: {
  root: string;
  data: Library | undefined;
  isPending: boolean;
  isFetching: boolean;
  error: string | null;
  rescan: () => void;
  view: ViewMode;
  setView: (view: ViewMode) => void;
}) {
  const { t } = useTranslation();
  const [params, setParams] = useSearchParams();
  const filters = useMemo(() => parseFilters(params), [params]);
  const setFilters = useCallback(
    (patch: Partial<LibraryFilters>) => setParams(filtersToParams({ ...filters, ...patch }), { replace: true }),
    [filters, setParams],
  );
  const games = useMemo(() => (data ? applyFilters(data.games, filters) : []), [data, filters]);
  const withWarnings = data?.games.filter((g) => g.warnings.length > 0).length ?? 0;
  const withoutCover = data?.games.filter((g) => !hasCover(g)).length ?? 0;
  const missingFolders = data?.folders.filter((f) => !f.present) ?? [];
  const initFlow = usePlan();
  const searchRef = useRef<HTMLInputElement>(null);
  const [skippedOpen, setSkippedOpen] = useState(false);
  const [artOpen, setArtOpen] = useState(false);

  // "/" focuses the search box, like most lists.
  useEffect(() => {
    const onKey = (event: globalThis.KeyboardEvent) => {
      if (event.key !== "/" || event.ctrlKey || event.metaKey || event.altKey) return;
      const target = event.target as HTMLElement | null;
      if (target && ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName)) return;
      event.preventDefault();
      searchRef.current?.focus();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex flex-col gap-3 border-b px-6 py-3">
        <div className="flex flex-wrap items-center gap-2">
          <div className="relative min-w-48 flex-1 max-w-sm">
            <Search className="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-muted-foreground" aria-hidden />
            <Input
              ref={searchRef}
              type="search"
              value={filters.q}
              onChange={(event) => setFilters({ q: event.target.value })}
              placeholder={t("library.searchPlaceholder")}
              aria-label={t("library.search")}
              className="pl-8"
            />
          </div>
          <ToggleGroup
            type="single"
            variant="outline"
            size="sm"
            spacing={0}
            value={filters.media}
            onValueChange={(value) => value && setFilters({ media: value as LibraryFilters["media"] })}
            aria-label={t("library.filterMedia")}
          >
            <ToggleGroupItem value="all">{t("library.all")}</ToggleGroupItem>
            <ToggleGroupItem value="CD">CD</ToggleGroupItem>
            <ToggleGroupItem value="DVD">DVD</ToggleGroupItem>
          </ToggleGroup>
          <ToggleGroup
            type="single"
            variant="outline"
            size="sm"
            spacing={0}
            value={filters.format}
            onValueChange={(value) => value && setFilters({ format: value as LibraryFilters["format"] })}
            aria-label={t("library.filterFormat")}
          >
            <ToggleGroupItem value="all">{t("library.all")}</ToggleGroupItem>
            <ToggleGroupItem value="ISO">ISO</ToggleGroupItem>
            <ToggleGroupItem value="ZSO">ZSO</ToggleGroupItem>
            <ToggleGroupItem value="UL">UL</ToggleGroupItem>
          </ToggleGroup>
          <Toggle
            variant="outline"
            size="sm"
            pressed={filters.warnings}
            onPressedChange={(warnings) => setFilters({ warnings })}
            aria-label={t("library.onlyWarnings")}
          >
            <AlertTriangle aria-hidden />
            {t("library.warnings", { count: withWarnings })}
          </Toggle>
          <Toggle
            variant="outline"
            size="sm"
            pressed={filters.noCover}
            onPressedChange={(noCover) => setFilters({ noCover })}
            aria-label={t("library.onlyNoCover")}
            data-testid="filter-no-cover"
          >
            <ImageOff aria-hidden />
            {t("library.noCover", { count: withoutCover })}
          </Toggle>
          <div className="ml-auto flex items-center gap-2">
            <Select value={filters.sort} onValueChange={(sort) => setFilters({ sort: sort as SortKey })}>
              <SelectTrigger size="sm" aria-label={t("library.sort")}>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {sortKeys.map((key) => (
                  <SelectItem key={key} value={key}>
                    {t(`library.sortBy.${key}`)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <Button
              variant="outline"
              size="icon-sm"
              onClick={() => setFilters({ desc: !filters.desc })}
              aria-label={filters.desc ? t("library.sortDescending") : t("library.sortAscending")}
              aria-pressed={filters.desc}
            >
              {filters.desc ? <ArrowUpZA aria-hidden /> : <ArrowDownAZ aria-hidden />}
            </Button>
            <ToggleGroup
              type="single"
              variant="outline"
              size="sm"
              spacing={0}
              value={view}
              onValueChange={(value) => (value === "grid" || value === "list") && setView(value)}
              aria-label={t("library.view")}
            >
              <ToggleGroupItem value="grid" aria-label={t("library.viewGrid")}>
                <LayoutGrid aria-hidden />
              </ToggleGroupItem>
              <ToggleGroupItem value="list" aria-label={t("library.viewList")}>
                <List aria-hidden />
              </ToggleGroupItem>
            </ToggleGroup>
            <Tooltip>
              <TooltipTrigger asChild>
                <Button variant="outline" size="icon-sm" onClick={rescan} disabled={isFetching} aria-label={t("library.rescan")}>
                  <RefreshCw className={cn(isFetching && "animate-spin")} aria-hidden />
                </Button>
              </TooltipTrigger>
              <TooltipContent>{t("library.rescan")}</TooltipContent>
            </Tooltip>
            <Button
              variant="outline"
              size="sm"
              onClick={() => setArtOpen(true)}
              disabled={!data || data.games.length === 0}
              data-testid="download-covers"
            >
              <ImageDown aria-hidden />
              {t("library.downloadCovers")}
            </Button>
            <Button asChild size="sm">
              <Link to="/library/add">
                <Plus aria-hidden />
                {t("library.addGames")}
              </Link>
            </Button>
          </div>
        </div>
        <div className="flex flex-wrap items-center gap-x-4 gap-y-1 text-xs text-muted-foreground">
          <span className="truncate font-mono" title={root}>
            {root}
          </span>
          {data && (
            <>
              <span data-testid="library-count">
                {t("library.count", { shown: games.length, total: data.games.length })}
              </span>
              <span>{formatBytes(data.games.reduce((sum, g) => sum + g.size, 0))}</span>
              {data.skipped.length > 0 && (
                <button type="button" className="underline-offset-2 hover:underline" onClick={() => setSkippedOpen(true)}>
                  {t("library.skipped", { count: data.skipped.length })}
                </button>
              )}
            </>
          )}
        </div>
      </div>

      <div className="flex flex-col gap-3 px-6 pt-3 empty:hidden">
        <RecoveryBanner root={root} />
        {isPending && <ScanProgress root={root} />}
        {error && (
          <p className="rounded-md border border-destructive/40 bg-destructive/10 p-3 text-sm text-destructive" role="alert">
            {t("library.scanFailed", { error })}
          </p>
        )}
        {data && data.warnings.length > 0 && <NoticeList notices={data.warnings} />}
        {missingFolders.length > 0 && (
          <div className="flex flex-wrap items-center gap-2 rounded-md border bg-card p-3 text-sm">
            <FolderPlus className="size-4 text-muted-foreground" aria-hidden />
            <span className="flex-1">
              {t("library.missingFolders", { folders: missingFolders.map((f) => f.name).join(", ") })}
            </span>
            <Button size="sm" variant="outline" onClick={() => void initFlow.request({ type: "PlanInit", root })}>
              {t("library.createFolders")}
            </Button>
          </div>
        )}
      </div>
      <PlanDialog flow={initFlow} />
      {data && <ArtFetchDialog root={root} games={data.games} open={artOpen} onOpenChange={setArtOpen} />}

      {data && data.games.length === 0 && !isPending && (
        <div className="flex flex-1 flex-col items-center justify-center p-6">
          <EmptyState icon={Disc3} title={t("library.noGamesTitle")} description={t("library.noGamesDescription")} />
          <Button asChild>
            <Link to="/library/add">
              <Plus aria-hidden />
              {t("library.addGames")}
            </Link>
          </Button>
        </div>
      )}
      {data && data.games.length > 0 && games.length === 0 && (
        <div className="flex flex-1 flex-col items-center justify-center p-6">
          <EmptyState icon={Search} title={t("library.noMatchTitle")} description={t("library.noMatchDescription")} />
          <Button variant="outline" onClick={() => setParams(new URLSearchParams(), { replace: true })}>
            <X aria-hidden />
            {t("library.clearFilters")}
          </Button>
        </div>
      )}
      {games.length > 0 && (view === "grid" ? <GameGrid games={games} /> : <GameTable games={games} />)}

      {data && (
        <Dialog open={skippedOpen} onOpenChange={setSkippedOpen}>
          <DialogContent className="flex max-h-[85dvh] flex-col sm:max-w-xl">
            <DialogHeader>
              <DialogTitle>{t("library.skippedTitle")}</DialogTitle>
              <DialogDescription>{t("library.skippedDescription")}</DialogDescription>
            </DialogHeader>
            <ul className="min-h-0 flex-1 overflow-auto text-sm">
              {data.skipped.map((skipped) => (
                <li key={skipped.path} className="border-b py-2 last:border-0">
                  <div className="truncate font-mono text-xs" title={skipped.path}>
                    {skipped.path}
                  </div>
                  <NoticeList notices={[skipped.reason]} compact />
                </li>
              ))}
            </ul>
          </DialogContent>
        </Dialog>
      )}
    </div>
  );
}

/** The running scan of this root, from the job stream. */
function ScanProgress({ root }: { root: string }) {
  const { t } = useTranslation();
  const { active } = useJobs();
  const scan = active.find((job) => job.kind === "scanLibrary" && job.root === root);
  const value = scan ? percent(scan.done, scan.total) : null;
  return (
    <div className="flex flex-col gap-1.5" role="status" data-testid="scan-progress">
      <div className="flex items-center justify-between text-sm">
        <span>{t("library.scanning")}</span>
        {scan && scan.total !== null && (
          <span className="font-mono text-xs text-muted-foreground tabular-nums">
            {scan.done} / {scan.total}
          </span>
        )}
      </div>
      <Progress value={value ?? 0} className={cn(value === null && "animate-pulse")} />
    </div>
  );
}

/** Arrow keys move between cards; the virtualizer scrolls the target in first. */
function useRovingFocus(count: number, columns: number, scrollTo: (index: number) => void) {
  const [pending, setPending] = useState<number | null>(null);
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const target = event.target as HTMLElement;
    const index = Number(target.dataset.index);
    if (Number.isNaN(index)) return;
    let next: number;
    switch (event.key) {
      case "ArrowRight":
        next = index + 1;
        break;
      case "ArrowLeft":
        next = index - 1;
        break;
      case "ArrowDown":
        next = index + columns;
        break;
      case "ArrowUp":
        next = index - columns;
        break;
      case "Home":
        next = 0;
        break;
      case "End":
        next = count - 1;
        break;
      default:
        return;
    }
    if (next < 0 || next >= count) return;
    event.preventDefault();
    scrollTo(next);
    setPending(next);
  };
  useLayoutEffect(() => {
    if (pending === null) return;
    const frame = requestAnimationFrame(() => {
      document.querySelector<HTMLElement>(`[data-index="${pending}"]`)?.focus();
      setPending(null);
    });
    return () => cancelAnimationFrame(frame);
  }, [pending]);
  return onKeyDown;
}

function useColumns(ref: React.RefObject<HTMLElement | null>): number {
  const [columns, setColumns] = useState(4);
  useEffect(() => {
    const element = ref.current;
    if (!element) return;
    const observer = new ResizeObserver(([entry]) => {
      const width = entry?.contentRect.width ?? element.clientWidth;
      setColumns(Math.max(1, Math.floor((width + GRID_GAP) / (GRID_MIN_COLUMN + GRID_GAP))));
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, [ref]);
  return columns;
}

function GameGrid({ games }: { games: Game[] }) {
  const { t } = useTranslation();
  const scrollRef = useRef<HTMLDivElement>(null);
  const columns = useColumns(scrollRef);
  const rows = Math.ceil(games.length / columns);
  const virtualizer = useVirtualizer({
    count: rows,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => GRID_ROW_HEIGHT,
    overscan: 2,
  });
  const onKeyDown = useRovingFocus(games.length, columns, (index) =>
    virtualizer.scrollToIndex(Math.floor(index / columns), { align: "auto" }),
  );

  return (
    <div ref={scrollRef} className="min-h-0 flex-1 overflow-auto px-6 py-4" data-testid="game-grid">
      <div
        role="list"
        aria-label={t("library.games")}
        className="relative w-full"
        style={{ height: virtualizer.getTotalSize() }}
        onKeyDown={onKeyDown}
      >
        {virtualizer.getVirtualItems().map((row) => (
          <div
            key={row.key}
            className="absolute top-0 left-0 grid w-full"
            style={{
              transform: `translateY(${row.start}px)`,
              height: row.size,
              gridTemplateColumns: `repeat(${columns}, minmax(0, 1fr))`,
              gap: GRID_GAP,
            }}
          >
            {games.slice(row.index * columns, (row.index + 1) * columns).map((game, i) => (
              <GameCard key={gameKey(game)} game={game} index={row.index * columns + i} />
            ))}
          </div>
        ))}
      </div>
    </div>
  );
}

function GameCard({ game, index }: { game: Game; index: number }) {
  const { t } = useTranslation();
  return (
    <Link
      to={`/library/game/${encodeURIComponent(gameKey(game))}`}
      role="listitem"
      data-index={index}
      className="group flex flex-col gap-2 rounded-lg p-2 outline-none transition-colors hover:bg-muted focus-visible:ring-3 focus-visible:ring-ring/50"
      aria-label={`${game.title}, ${game.startup}`}
    >
      <GameCover game={game} className="transition-transform group-hover:scale-[1.02]" />
      <div className="min-w-0">
        <div className="truncate text-sm font-medium" title={game.title}>
          {game.title}
        </div>
        <div className="flex items-center justify-between gap-2 text-xs text-muted-foreground">
          <span className="font-mono">{game.startup}</span>
          <span className="tabular-nums">{formatBytes(game.size, 1)}</span>
        </div>
        {game.warnings.length > 0 && (
          <div className="mt-1 flex items-center gap-1 text-xs text-amber-600 dark:text-amber-400">
            <AlertTriangle className="size-3" aria-hidden />
            {t("library.warningCount", { count: game.warnings.length })}
          </div>
        )}
      </div>
    </Link>
  );
}

function GameTable({ games }: { games: Game[] }) {
  const { t } = useTranslation();
  const scrollRef = useRef<HTMLDivElement>(null);
  const virtualizer = useVirtualizer({
    count: games.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => LIST_ROW_HEIGHT,
    overscan: 8,
  });
  const onKeyDown = useRovingFocus(games.length, 1, (index) => virtualizer.scrollToIndex(index, { align: "auto" }));

  return (
    <div ref={scrollRef} className="min-h-0 flex-1 overflow-auto px-6 py-4" data-testid="game-list">
      <div className="sticky top-0 z-10 grid grid-cols-[1fr_8rem_4rem_4rem_6rem] gap-3 border-b bg-background px-2 pb-2 text-xs font-medium text-muted-foreground">
        <span>{t("game.title")}</span>
        <span>{t("game.id")}</span>
        <span>{t("game.media")}</span>
        <span>{t("game.format")}</span>
        <span className="text-right">{t("game.size")}</span>
      </div>
      <div
        role="list"
        aria-label={t("library.games")}
        className="relative w-full"
        style={{ height: virtualizer.getTotalSize() }}
        onKeyDown={onKeyDown}
      >
        {virtualizer.getVirtualItems().map((row) => {
          const game = games[row.index];
          if (!game) return null;
          return (
            <Link
              key={row.key}
              to={`/library/game/${encodeURIComponent(gameKey(game))}`}
              role="listitem"
              data-index={row.index}
              className="absolute top-0 left-0 grid w-full grid-cols-[1fr_8rem_4rem_4rem_6rem] items-center gap-3 rounded-md border-b px-2 text-sm outline-none hover:bg-muted focus-visible:ring-3 focus-visible:ring-ring/50"
              style={{ transform: `translateY(${row.start}px)`, height: row.size }}
              aria-label={`${game.title}, ${game.startup}`}
            >
              <span className="flex min-w-0 items-center gap-2">
                <CoverThumb game={game} />
                <span className="truncate font-medium" title={game.title}>
                  {game.title}
                </span>
                {game.warnings.length > 0 && (
                  <Badge variant="outline" className="shrink-0 gap-1 text-amber-600 dark:text-amber-400">
                    <AlertTriangle aria-hidden />
                    {game.warnings.length}
                  </Badge>
                )}
              </span>
              <span className="font-mono text-xs">{game.startup}</span>
              <span className="text-xs">{game.media}</span>
              <span className="text-xs">{game.format}</span>
              <span className="text-right font-mono text-xs tabular-nums">{formatBytes(game.size)}</span>
            </Link>
          );
        })}
      </div>
    </div>
  );
}
