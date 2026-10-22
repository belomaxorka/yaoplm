import { cn } from "cn";
import { HardDrive, Library, ListChecks, Settings } from "lucide-react";
import { useTranslation } from "react-i18next";
import { NavLink, Outlet, useLocation } from "react-router";

import { ConnectionStatus } from "@/components/ConnectionStatus";
import { LanguageSwitch } from "@/components/LanguageSwitch";
import { ThemeToggle } from "@/components/ThemeToggle";

const navItems = [
  { to: "/", label: "nav.library", icon: Library },
  { to: "/devices", label: "nav.devices", icon: HardDrive },
  { to: "/jobs", label: "nav.jobs", icon: ListChecks },
  { to: "/settings", label: "nav.settings", icon: Settings },
] as const;

export function AppShell() {
  const { t } = useTranslation();
  const { pathname } = useLocation();
  const current = navItems.find((item) => item.to === pathname) ?? navItems[0];

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
          {navItems.map(({ to, label, icon: Icon }) => (
            <NavLink
              key={to}
              to={to}
              end
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
              {t(label)}
            </NavLink>
          ))}
        </nav>

        <div className="border-t px-4 py-3">
          <ConnectionStatus />
        </div>
      </aside>

      <div className="flex min-w-0 flex-1 flex-col">
        <header className="flex h-14 shrink-0 items-center gap-2 border-b px-6">
          <h1 className="flex-1 truncate text-lg font-semibold">{t(current.label)}</h1>
          <LanguageSwitch />
          <ThemeToggle />
        </header>
        <main id="main" tabIndex={-1} className="flex-1 overflow-auto p-6 outline-none">
          <Outlet />
        </main>
      </div>
    </div>
  );
}
