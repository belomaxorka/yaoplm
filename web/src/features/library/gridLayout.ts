/** About OPL's own 140 px cover plus the card's padding. */
export const GRID_MIN_COLUMN = 150;
/** Between cards; their own padding adds to it, so covers end up 24 px apart. */
export const GRID_GAP = 8;
/** The card around the cover: `p-2` on each side and `gap-2` under the cover, in rem. */
const CARD_PADDING_REM = 0.5;
const CARD_GAP_REM = 0.5;
/** The title, then the ID with the warning and dump marks: `h-9`. */
const CARD_TEXT_REM = 2.25;
/** OPL's COV proportions (140×200), as `GameCover` draws them. */
const COVER_RATIO = 10 / 7;

export interface GridLayout {
  columns: number;
  /** A card plus the gap under it: every row is the same height. */
  rowHeight: number;
}

/**
 * Columns and row height for a grid `width` px wide. Cards stretch to fill
 * the row, so the cover, and with it the row, grows with the column width.
 */
export function gridLayout(width: number, rem = 16): GridLayout {
  const columns = Math.max(1, Math.floor((width + GRID_GAP) / (GRID_MIN_COLUMN + GRID_GAP)));
  const column = Math.max(0, (width - GRID_GAP * (columns - 1)) / columns);
  const cover = Math.max(0, column - 2 * CARD_PADDING_REM * rem) * COVER_RATIO;
  const card = (2 * CARD_PADDING_REM + CARD_GAP_REM + CARD_TEXT_REM) * rem + cover;
  return { columns, rowHeight: Math.ceil(card) + GRID_GAP };
}
