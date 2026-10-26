import { cn } from "cn";
import type { ComponentProps } from "react";

/**
 * Facts on one line, each but the last followed by a middle dot (UI-4; the
 * owner, 2026-10-27): "Model: X · Serial: Y". The dot is drawn, not text,
 * so screen readers do not hear it, and it stays with the fact before it
 * when the line wraps. Every child is a fact; a hidden child would leave a
 * stray dot, so what only screen readers get goes outside.
 */
export function Facts({ className, ...props }: ComponentProps<"div">) {
  return (
    <div
      className={cn(
        "flex flex-wrap items-center gap-y-1 [&>*:not(:last-child)]:after:mx-1.5 [&>*:not(:last-child)]:after:text-muted-foreground [&>*:not(:last-child)]:after:content-['·']",
        className,
      )}
      {...props}
    />
  );
}
