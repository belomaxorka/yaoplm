import { cn } from "cn";
import { HardDrive, Library, ListChecks, Settings } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Link, NavLink, Outlet, useLocation } from "react-router";

import { ConnectionStatus } from "@/components/ConnectionStatus";
import { LanguageSwitch } from "@/components/LanguageSwitch";
import { ThemeToggle } from "@/components/ThemeToggle";
import { Badge } from "@/components/ui/badge";
import { JobsDock } from "@/features/jobs/JobsDock";
import { JobToasts } from "@/features/jobs/JobToasts";
import { LibraryKeeper } from "@/features/library/useLibrary";
import { isUserJob, useJobs } from "@/jobs/context";
import { useSettings } from "@/settings/context";

const navItems = [
  { to: "/library", label: "nav.library", icon: Library, advanced: false },
  { to: "/devices", label: "nav.devices", icon: HardDrive, advanced: false },
  // Simple mode follows writes in the dock and notifications instead.
  { to: "/jobs", label: "nav.jobs", icon: ListChecks, advanced: true },
  { to: "/settings", label: "nav.settings", icon: Settings, advanced: false },
] as const;

export function AppShell() {
  const { t } = useTranslation();
  const { pathname } = useLocation();
  const active = useJobs().active.filter(isUserJob);
  const { settings } = useSettings();
  const simple = settings.simpleMode;
  const current = navItems.find((item) => pathname.startsWith(item.to)) ?? navItems[0];
  const title =
    pathname === "/library/add" ? t("add.heading") : pathname.startsWith("/library/game/") ? t("game.heading") : t(current.label);

  return (
    <div className="flex h-dvh bg-background text-foreground">
      <a
        href="#main"
        className="sr-only z-50 rounded-md bg-primary px-3 py-2 text-sm text-primary-foreground focus:not-sr-only focus:absolute focus:top-2 focus:left-2"
      >
        {t("app.skipToContent")}
      </a>

      <aside className="flex w-56 shrink-0 flex-col border-r bg-sidebar text-sidebar-foreground">
        <div className="flex h-14 shrink-0 items-center gap-2.5 border-b px-4">
          <img src="/favicon.svg" alt="" className="size-7" />
          <div className="leading-tight">
            <div className="text-sm font-semibold">{t("app.name")}</div>
            <div className="text-xs text-muted-foreground">{t("app.tagline")}</div>
          </div>
        </div>

        <nav aria-label={t("nav.label")} className="flex flex-1 flex-col gap-0.5 px-2 py-2">
          {navItems
            .filter((item) => !(simple && item.advanced))
            .map(({ to, label, icon: Icon }) => (
              <NavLink
                key={to}
                to={to}
                className={({ isActive }) =>
                  cn(
                    "flex items-center gap-2.5 rounded-md px-3 py-2 text-sm outline-none transition-colors focus-visible:ring-3 focus-visible:ring-ring/50",
                    isActive
                      ? "bg-sidebar-accent font-medium text-sidebar-accent-foreground"
                      : "text-muted-foreground hover:bg-sidebar-accent/60 hover:text-sidebar-accent-foreground",
                  )
                }
              >
                <Icon className="size-4" aria-hidden />
                <span className="flex-1">{t(label)}</span>
                {to === "/jobs" && active.length > 0 && (
                  <Badge className="h-5 min-w-5 justify-center px-1.5 tabular-nums" aria-label={t("jobs.active", { count: active.length })}>
                    {active.length}
                  </Badge>
                )}
              </NavLink>
            ))}
        </nav>

        <div className="border-t px-4 py-3">
          <ConnectionStatus />
        </div>
      </aside>

      <div className="flex min-w-0 flex-1 flex-col">
        <header className="flex h-14 shrink-0 items-center gap-3 border-b px-6">
          <h1 className="shrink-0 text-lg font-semibold">{title}</h1>
          {settings.root && (
            <Link
              to="/devices"
              className="hidden min-w-0 max-w-xs items-center gap-1.5 truncate rounded-md border px-2 py-1 font-mono text-xs text-muted-foreground outline-none hover:text-foreground focus-visible:ring-3 focus-visible:ring-ring/50 md:flex"
              title={t("devices.changeDevice")}
              data-testid="current-root"
            >
              <HardDrive className="size-3.5 shrink-0" aria-hidden />
              <span className="truncate">{settings.root}</span>
            </Link>
          )}
          <div className="ml-auto flex items-center gap-2">
            <LanguageSwitch />
            <ThemeToggle />
          </div>
        </header>
        <main id="main" tabIndex={-1} className="flex min-h-0 flex-1 flex-col overflow-hidden outline-none">
          <Outlet />
        </main>
      </div>
      <JobsDock />
      <JobToasts />
      <LibraryKeeper />
    </div>
  );
}
