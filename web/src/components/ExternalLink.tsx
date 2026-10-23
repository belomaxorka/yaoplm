import { isTauri } from "@tauri-apps/api/core";
import { cn } from "cn";
import { ExternalLink as ExternalIcon } from "lucide-react";
import type { ReactNode } from "react";

/**
 * A link to a web page outside the app. In the desktop app the WebView does
 * not open new windows, so the system browser opens it (Tauri's opener
 * plugin); the module loads only there.
 */
export function ExternalLink({ href, children, className }: { href: string; children?: ReactNode; className?: string }) {
  return (
    <a
      href={href}
      target="_blank"
      rel="noreferrer"
      className={cn(
        "inline-flex items-center gap-1 rounded-sm text-primary underline-offset-2 outline-none hover:underline focus-visible:ring-3 focus-visible:ring-ring/50",
        className,
      )}
      onClick={(event) => {
        if (!isTauri()) return;
        event.preventDefault();
        void import("@tauri-apps/plugin-opener").then(({ openUrl }) => openUrl(href));
      }}
    >
      {children ?? href.replace(/^https?:\/\//, "")}
      <ExternalIcon className="size-3" aria-hidden />
    </a>
  );
}
