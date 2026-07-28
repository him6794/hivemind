"use client";

import { useEffect, useMemo, useState } from "react";
import { CornerDownLeft, FileText, Home, LogIn, Shield, User, UserPlus } from "lucide-react";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog";
import { useAppStore, type Route } from "@/store/app-store";
import { useI18n } from "@/store/i18n-store";
import { getSiteDefinition } from "@/lib/hivemind-site-data.mjs";
import { cn } from "@/lib/utils";

const routeIcons: Record<Route, React.ElementType> = {
  home: Home,
  login: LogIn,
  register: UserPlus,
  account: User,
  security: Shield,
  docs: FileText,
};

export function CommandPalette() {
  const open = useAppStore((state) => state.commandOpen);
  const setOpen = useAppStore((state) => state.setCommandOpen);
  const navigate = useAppStore((state) => state.navigate);
  const { locale } = useI18n();
  const site = useMemo(() => getSiteDefinition(locale), [locale]);
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        setOpen(!open);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, setOpen]);

  const filtered = useMemo(() => {
    if (!query.trim()) return site.routes;
    const normalized = query.trim().toLowerCase();
    return site.routes.filter((route) => route.label.toLowerCase().includes(normalized) || route.id.includes(normalized));
  }, [query, site.routes]);

  useEffect(() => {
    if (!open) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "ArrowDown") {
        event.preventDefault();
        setActive((index) => Math.min(index + 1, filtered.length - 1));
      } else if (event.key === "ArrowUp") {
        event.preventDefault();
        setActive((index) => Math.max(index - 1, 0));
      } else if (event.key === "Enter") {
        event.preventDefault();
        const route = filtered[active];
        if (route) {
          navigate(route.id as Route);
          setOpen(false);
        }
      } else if (event.key === "Escape") {
        setOpen(false);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [active, filtered, navigate, open, setOpen]);

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogContent className="top-[15%] max-w-xl translate-y-0 gap-0 overflow-hidden rounded-2xl border-border/60 bg-card/95 p-0 backdrop-blur-2xl">
        <DialogTitle className="sr-only">Command palette</DialogTitle>
        <DialogDescription className="sr-only">Quick navigation across the Hivemind site.</DialogDescription>

        <div className="flex items-center gap-3 border-b border-border/60 px-4">
          <input
            autoFocus
            value={query}
            onChange={(event) => {
              setQuery(event.target.value);
              setActive(0);
            }}
            placeholder={locale === "zh" ? "搜尋頁面或操作..." : "Search pages or actions..."}
            className="h-14 flex-1 bg-transparent text-sm outline-none placeholder:text-muted-foreground"
          />
          <kbd className="rounded-md border border-border bg-background/60 px-1.5 py-0.5 font-mono-tech text-[10px] text-muted-foreground">
            ESC
          </kbd>
        </div>

        <div className="max-h-[60vh] overflow-y-auto scroll-tech p-2">
          {filtered.map((route, index) => {
            const Icon = routeIcons[route.id];
            const isActive = index === active;
            return (
              <button
                key={route.id}
                type="button"
                onMouseEnter={() => setActive(index)}
                onClick={() => {
                  navigate(route.id as Route);
                  setOpen(false);
                }}
                className={cn(
                  "flex w-full items-center gap-3 rounded-xl px-3 py-2.5 text-left transition-colors",
                  isActive ? "bg-honey/10 text-honey" : "hover:bg-accent/40"
                )}
              >
                <span className={cn("flex size-8 items-center justify-center rounded-lg", isActive ? "bg-honey/15" : "bg-accent")}>
                  <Icon className="size-4" />
                </span>
                <div className="flex-1 min-w-0">
                  <div className="text-sm font-medium">{route.label}</div>
                  <div className="text-[11px] text-muted-foreground">{route.id}</div>
                </div>
                {isActive ? <CornerDownLeft className="size-3.5 text-honey" /> : null}
              </button>
            );
          })}
        </div>
      </DialogContent>
    </Dialog>
  );
}
