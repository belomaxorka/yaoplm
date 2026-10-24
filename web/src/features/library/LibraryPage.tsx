import { useVirtualizer } from "@tanstack/react-virtual";
import { cn } from "cn";
import {
  AlertTriangle,
  ArrowDownAZ,
  ArrowUpZA,
  ChevronDown,
  Disc3,
  FolderInput,
  FolderPlus,
  HardDrive,
  ImageDown,
  ImageOff,
  LayoutGrid,
  List,
  Plus,
  RefreshCw,
  Replace,
  Search,
  ShieldCheck,
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
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import { Progress } from "@/components/ui/progress";
import { Skeleton } from "@/components/ui/skeleton";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Toggle } from "@/components/ui/toggle";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { ArtFetchDialog } from "@/features/art/ArtFetchDialog";
import { ArtImportDialog } from "@/features/art/ArtImportDialog";
import { ConvertDialog } from "@/features/convert/ConvertDialog";
import { CoverThumb, GameCover } from "@/features/library/GameCover";
import {
  applyFilters,
  effectiveFilters,
  filtersToParams,
  type LibraryFilters,
  parseFilters,
  type SortKey,
  sortKeys,
} from "@/features/library/filters";
import { GRID_GAP, GRID_MIN_COLUMN, type GridLayout, gridLayout } from "@/features/library/gridLayout";
import { useLibrary } from "@/features/library/useLibrary";
import { PlanDialog } from "@/features/plan/PlanDialog";
import { usePlan } from "@/features/plan/usePlan";
import { RecoveryBanner } from "@/features/recovery/RecoveryBanner";
import { DumpIcon } from "@/features/verify/DumpBadge";
import { VerifyDialog } from "@/features/verify/VerifyDialog";
import { useJobs } from "@/jobs/context";
import { formatBytes, percent } from "@/lib/format";
import { gameKey } from "@/lib/gameKey";
import { errorText } from "@/lib/phrase";
import { useSettings, useSimpleMode } from "@/settings/context";
import type { ViewMode } from "@/settings/settings";
import { DeviceModeLine, SmbFolderHint } from "@/features/devices/DeviceMode";
import { LoaderProfileHint, LoaderProfileLine } from "@/features/devices/LoaderProfile";
import { smbStep } from "@/features/devices/smbStep";
import { useLoaderProfile } from "@/features/devices/useLoaderProfile";

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
      error={library.error ? errorText(library.error) : null}
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
  const simple = useSimpleMode();
  const filters = useMemo(() => parseFilters(params), [params]);
  const shown = useMemo(() => effectiveFilters(filters, simple), [filters, simple]);
  const shownView = simple ? "grid" : view;
  const setFilters = useCallback(
    (patch: Partial<LibraryFilters>) => setParams(filtersToParams({ ...filters, ...patch }), { replace: true }),
    [filters, setParams],
  );
  const games = useMemo(() => (data ? applyFilters(data.games, shown) : []), [data, shown]);
  const withWarnings = data?.games.filter((g) => g.warnings.length > 0).length ?? 0;
  const withoutCover = data?.games.filter((g) => !hasCover(g)).length ?? 0;
  const missingFolders = data?.folders.filter((f) => !f.present) ?? [];
  const loader = useLoaderProfile();
  // What the detection found: ZSO 1.1.0 cannot show, JPG art, several loaders.
  const loaderNotices = loader.active?.source === "detected" ? (loader.active.detected?.notices ?? []) : [];
  // A folder read over SMB gets its own next step (ADR-0044): a whole system
  // disk is not filled with OPL's folders.
  const smbNext = loader.device ? smbStep(loader.device) : null;
  const initFlow = usePlan();
  const searchRef = useRef<HTMLInputElement>(null);
  const [skippedOpen, setSkippedOpen] = useState(false);
  const [artOpen, setArtOpen] = useState(false);
  const [importOpen, setImportOpen] = useState(false);
  const [verifyOpen, setVerifyOpen] = useState(false);
  const [convertOpen, setConvertOpen] = useState(false);

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
          <div className="relative max-w-sm min-w-48 flex-1">
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
          {!simple && (
            <AdvancedFilters
              filters={filters}
              setFilters={setFilters}
              withWarnings={withWarnings}
              withoutCover={withoutCover}
              view={view}
              setView={setView}
            />
          )}
          <div className="ml-auto flex flex-wrap items-center justify-end gap-2">
            <Tooltip>
              <TooltipTrigger asChild>
                <Button variant="outline" size="icon-sm" onClick={rescan} disabled={isFetching} aria-label={t("library.rescan")}>
                  <RefreshCw className={cn(isFetching && "animate-spin")} aria-hidden />
                </Button>
              </TooltipTrigger>
              <TooltipContent>{t("library.rescan")}</TooltipContent>
            </Tooltip>
            {!simple && (
              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <Button variant="outline" size="sm" disabled={!data || data.games.length === 0} data-testid="library-more">
                    {t("library.more")}
                    <ChevronDown aria-hidden />
                  </Button>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="end" className="w-max">
                  <DropdownMenuItem onSelect={() => setImportOpen(true)} data-testid="import-pictures">
                    <FolderInput aria-hidden />
                    {t("library.importPictures")}
                  </DropdownMenuItem>
                  <DropdownMenuItem onSelect={() => setVerifyOpen(true)} data-testid="verify-dumps">
                    <ShieldCheck aria-hidden />
                    {t("library.verifyDumps")}
                  </DropdownMenuItem>
                  <DropdownMenuItem
                    onSelect={() => setConvertOpen(true)}
                    disabled={games.length === 0}
                    title={t("library.convertShownHint")}
                    data-testid="convert-shown"
                  >
                    <Replace aria-hidden />
                    {t("library.convertShown")}
                  </DropdownMenuItem>
                </DropdownMenuContent>
              </DropdownMenu>
            )}
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
          {/* The header names the device already; the path is a detail. */}
          {!simple && (
            <span className="truncate font-mono" title={root}>
              {root}
            </span>
          )}
          {loader.device && <DeviceModeLine device={loader.device} className="text-xs" />}
          {loader.active && <LoaderProfileLine loader={loader.active} className="text-xs" />}
          {data && (
            <>
              <span data-testid="library-count">
                {t("library.count", { shown: games.length, total: data.games.length })}
              </span>
              <span>{formatBytes(data.games.reduce((sum, g) => sum + g.size, 0))}</span>
              {data.skipped.length > 0 && (
                <button
                  type="button"
                  className="rounded-sm underline-offset-2 outline-none hover:underline focus-visible:ring-3 focus-visible:ring-ring/50"
                  onClick={() => setSkippedOpen(true)}
                >
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
          <div
            className="flex flex-wrap items-center gap-3 rounded-md border border-destructive/40 bg-destructive/10 p-3 text-sm text-destructive"
            role="alert"
          >
            <span className="min-w-0 flex-1 wrap-anywhere">{t("library.scanFailed", { error })}</span>
            <Button size="sm" variant="outline" onClick={rescan} disabled={isFetching}>
              <RefreshCw aria-hidden />
              {t("library.retryScan")}
            </Button>
          </div>
        )}
        {loader.device && <SmbFolderHint device={loader.device} />}
        {loader.device && <LoaderProfileHint device={loader.device} />}
        {loaderNotices.length > 0 && <NoticeList notices={loaderNotices} />}
        {data && data.warnings.length > 0 && <NoticeList notices={data.warnings} />}
        {missingFolders.length > 0 && smbNext === null && (
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
      {data && <ArtFetchDialog root={root} games={data.games} open={artOpen} onOpenChange={setArtOpen} quick={simple} />}
      <ArtImportDialog root={root} open={importOpen} onOpenChange={setImportOpen} />
      {data && <VerifyDialog root={root} games={data.games} open={verifyOpen} onOpenChange={setVerifyOpen} />}
      <ConvertDialog root={root} games={games} open={convertOpen} onOpenChange={setConvertOpen} />

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
      {/* The first scan: the shape of the grid, not an empty page that jumps. */}
      {isPending && !data && <GridSkeleton />}
      {games.length > 0 && (shownView === "grid" ? <GameGrid games={games} /> : <GameTable games={games} />)}

      {data && (
        <Dialog open={skippedOpen} onOpenChange={setSkippedOpen}>
          <DialogContent className="flex max-h-[85dvh] flex-col sm:max-w-xl">
            <DialogHeader>
              <DialogTitle>{t("library.skippedTitle")}</DialogTitle>
              <DialogDescription>{t("library.skippedDescription")}</DialogDescription>
            </DialogHeader>
            <ul className="relative min-h-0 flex-1 overflow-auto text-sm">
              {data.skipped.map((skipped) => (
                <li key={skipped.path} className="border-b py-2 last:border-0">
                  <div className="truncate font-mono text-xs" title={skipped.path}>
                    {skipped.path}
                  </div>
                  <NoticeList notices={[skipped.reason]} />
                </li>
              ))}
            </ul>
          </DialogContent>
        </Dialog>
      )}
    </div>
  );
}

/** Counter filters, sorting and the view: advanced mode only (UI-4). */
function AdvancedFilters({
  filters,
  setFilters,
  withWarnings,
  withoutCover,
  view,
  setView,
}: {
  filters: LibraryFilters;
  setFilters: (patch: Partial<LibraryFilters>) => void;
  withWarnings: number;
  withoutCover: number;
  view: ViewMode;
  setView: (view: ViewMode) => void;
}) {
  const { t } = useTranslation();
  return (
    <>
      <Toggle
        variant="outline"
        size="sm"
        pressed={filters.warnings}
        onPressedChange={(warnings) => setFilters({ warnings })}
        title={t("library.onlyWarnings")}
      >
        <AlertTriangle aria-hidden />
        {t("library.warnings", { count: withWarnings })}
      </Toggle>
      <Toggle
        variant="outline"
        size="sm"
        pressed={filters.noCover}
        onPressedChange={(noCover) => setFilters({ noCover })}
        title={t("library.onlyNoCover")}
        data-testid="filter-no-cover"
      >
        <ImageOff aria-hidden />
        {t("library.noCover", { count: withoutCover })}
      </Toggle>
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
    </>
  );
}

/** Placeholder cards while the first scan runs; hidden from screen readers, the progress speaks. */
function GridSkeleton() {
  return (
    <div
      className="grid min-h-0 flex-1 content-start overflow-hidden px-6 py-4"
      style={{ gridTemplateColumns: `repeat(auto-fill, minmax(${GRID_MIN_COLUMN}px, 1fr))`, gap: GRID_GAP }}
      aria-hidden
      data-testid="grid-skeleton"
    >
      {Array.from({ length: 12 }, (_, index) => (
        <div key={index} className="flex flex-col gap-2 p-2">
          <Skeleton className="aspect-[7/10] w-full rounded-lg" />
          <Skeleton className="h-4 w-4/5" />
          <Skeleton className="h-3 w-1/2" />
        </div>
      ))}
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
      <Progress value={value ?? 0} className={cn(value === null && "animate-pulse")} aria-label={t("library.scanning")} />
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

/** The grid's layout for the width of `ref`'s content box, kept up to date. */
function useGridLayout(ref: React.RefObject<HTMLElement | null>): GridLayout {
  const [layout, setLayout] = useState<GridLayout>(() => gridLayout(4 * (GRID_MIN_COLUMN + GRID_GAP)));
  useLayoutEffect(() => {
    const element = ref.current;
    if (!element) return;
    const update = (width: number) => {
      // Rem-based card parts follow the root font size the user may have changed.
      const rem = Number.parseFloat(getComputedStyle(document.documentElement).fontSize) || 16;
      const next = gridLayout(width, rem);
      setLayout((current) =>
        current.columns === next.columns && current.rowHeight === next.rowHeight ? current : next,
      );
    };
    update(element.clientWidth - Number.parseFloat(getComputedStyle(element).paddingLeft) * 2);
    const observer = new ResizeObserver(([entry]) => {
      if (entry) update(entry.contentRect.width);
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, [ref]);
  return layout;
}

function GameGrid({ games }: { games: Game[] }) {
  const { t } = useTranslation();
  const scrollRef = useRef<HTMLDivElement>(null);
  const { columns, rowHeight } = useGridLayout(scrollRef);
  const rows = Math.ceil(games.length / columns);
  const virtualizer = useVirtualizer({
    count: rows,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => rowHeight,
    overscan: 2,
  });
  const onKeyDown = useRovingFocus(games.length, columns, (index) =>
    virtualizer.scrollToIndex(Math.floor(index / columns), { align: "auto" }),
  );

  // A new width changes the row height and the games per row: drop the old
  // sizes and keep the first game on screen where it was.
  const firstShown = useRef(0);
  const shownLayout = useRef({ columns, rowHeight });
  useLayoutEffect(() => {
    const before = shownLayout.current;
    if (before.columns === columns && before.rowHeight === rowHeight) return;
    shownLayout.current = { columns, rowHeight };
    virtualizer.measure();
    if ((scrollRef.current?.scrollTop ?? 0) > 0) {
      virtualizer.scrollToIndex(Math.floor(firstShown.current / columns), { align: "start" });
    }
  }, [columns, rowHeight, virtualizer]);
  const items = virtualizer.getVirtualItems();
  useEffect(() => {
    const scroller = scrollRef.current;
    if (!scroller) return;
    const onScroll = () => {
      firstShown.current = Math.floor(scroller.scrollTop / shownLayout.current.rowHeight) * shownLayout.current.columns;
    };
    scroller.addEventListener("scroll", onScroll, { passive: true });
    return () => scroller.removeEventListener("scroll", onScroll);
  }, []);

  return (
    <div ref={scrollRef} className="relative min-h-0 flex-1 overflow-auto px-6 py-4" data-testid="game-grid">
      <div
        role="list"
        aria-label={t("library.games")}
        className="relative w-full"
        style={{ height: virtualizer.getTotalSize() }}
        onKeyDown={onKeyDown}
      >
        {items.map((row) => (
          <div
            key={row.key}
            className="absolute top-0 left-0 grid w-full"
            style={{
              transform: `translateY(${row.start}px)`,
              height: row.size - GRID_GAP,
              gridTemplateColumns: `repeat(${columns}, minmax(0, 1fr))`,
              columnGap: GRID_GAP,
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
  return (
    <Link
      to={`/library/game/${encodeURIComponent(gameKey(game))}`}
      role="listitem"
      data-index={index}
      className="group flex flex-col gap-2 rounded-lg p-2 outline-none transition-colors hover:bg-muted focus-visible:ring-3 focus-visible:ring-ring/50"
      aria-label={`${game.title}, ${game.startup}`}
    >
      <GameCover game={game} className="shrink-0 transition-transform group-hover:scale-[1.02]" />
      {/* A fixed height, part of the row height in `gridLayout`: text never spills into the next row.
          Two lines: the title, then the ID OPL shows with the marks; the size is in the list view. */}
      <div className="h-9 min-w-0 overflow-hidden">
        <div className="truncate text-sm font-medium" title={game.title}>
          {game.title}
        </div>
        <div className="flex items-center justify-between gap-2 text-xs text-muted-foreground">
          <span className="truncate font-mono">{game.startup}</span>
          <span className="flex shrink-0 items-center gap-1">
            {game.warnings.length > 0 && <WarningMark count={game.warnings.length} />}
            {game.dump && <DumpMark game={game} />}
          </span>
        </div>
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
    <div ref={scrollRef} className="relative min-h-0 flex-1 overflow-auto px-6 py-4" data-testid="game-list">
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
                  <Badge variant="outline" className="shrink-0 gap-1 text-amber-700 dark:text-amber-400">
                    <AlertTriangle aria-hidden />
                    {game.warnings.length}
                  </Badge>
                )}
                {game.dump && <DumpMark game={game} />}
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

/** Warnings as an icon; the game card lists them. */
function WarningMark({ count }: { count: number }) {
  const { t } = useTranslation();
  const label = t("library.warningCount", { count });
  return (
    <span className="flex items-center text-amber-700 dark:text-amber-400" title={label} data-testid="warning-mark">
      <AlertTriangle className="size-3.5" aria-hidden />
      <span className="sr-only">{label}</span>
    </span>
  );
}

/** The Redump verdict as an icon; the name says which. */
function DumpMark({ game }: { game: Game }) {
  const { t } = useTranslation();
  if (!game.dump) return null;
  const label = t(`dump.status.${game.dump.status}`);
  return (
    <span title={game.dump.redump ? `${label}: ${game.dump.redump}` : label} data-testid="dump-mark" data-status={game.dump.status}>
      <DumpIcon status={game.dump.status} className="size-3.5" />
      <span className="sr-only">{label}</span>
    </span>
  );
}
