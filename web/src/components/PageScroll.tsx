import { cn } from "cn";
import type { ReactNode } from "react";

/**
 * The scrolling body of a page; the shell's <main> itself does not scroll.
 * `relative` makes it the containing block of absolutely positioned
 * descendants (screen-reader labels, Radix's hidden inputs): without it
 * they are placed against the document and give the window a second
 * scrollbar on a long page.
 */
export function PageScroll({ children, className }: { children: ReactNode; className?: string }) {
  return <div className={cn("relative min-h-0 flex-1 overflow-auto p-6", className)}>{children}</div>;
}
