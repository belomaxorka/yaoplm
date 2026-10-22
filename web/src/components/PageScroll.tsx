import { cn } from "cn";
import type { ReactNode } from "react";

/** The scrolling body of a page; the shell's <main> itself does not scroll. */
export function PageScroll({ children, className }: { children: ReactNode; className?: string }) {
  return <div className={cn("min-h-0 flex-1 overflow-auto p-6", className)}>{children}</div>;
}
