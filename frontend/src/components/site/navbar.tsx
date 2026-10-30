"use client";

import { Menu } from "lucide-react";
import { useMemo, useRef, useState } from "react";
import { HiveLogo } from "./hive-logo";
import { ThemeToggle } from "./theme-toggle";
import { LocaleToggle } from "./locale-toggle";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog";
import { useAppStore, type Route } from "@/store/app-store";
import { useI18n } from "@/store/i18n-store";
import { getSiteDefinition } from "@/lib/hivemind-site-data.mjs";

export function Navbar() {
  const route = useAppStore((state) => state.route);
  const navigate = useAppStore((state) => state.navigate);
  const user = useAppStore((state) => state.user);
  const logout = useAppStore((state) => state.logout);
  const { locale } = useI18n();
  const zh = locale === "zh";
  const site = useMemo(() => getSiteDefinition(locale), [locale]);
  const [mobileOpen, setMobileOpen] = useState(false);
  const menuButton = useRef<HTMLButtonElement>(null);

  const label = (id: string) => site.routes.find((entry: { id: string }) => entry.id === id)?.label ?? id;
  const go = (target: Route) => {
    navigate(target);
    setMobileOpen(false);
  };

  return (
    <header className="fixed inset-x-0 top-0 z-40 border-b bg-background/95 backdrop-blur-sm">
      <nav aria-label={zh ? "主要導覽" : "Main navigation"} className="mx-auto flex h-18 max-w-7xl items-center justify-between gap-2 px-4 sm:px-6">
        <Button variant="ghost" onClick={() => navigate("home")} className="shrink-0 px-0 hover:bg-transparent" aria-label={zh ? "回到首頁" : "Hivemind home"}>
          <HiveLogo withText />
        </Button>
        <div className="hidden items-center gap-1 xl:flex">
          {site.nav.map((item: { id: string; label: string; route: string }) => {
            const active = item.route === route;
            return (
              <Button key={item.id} variant={active ? "secondary" : "ghost"} onClick={() => go(item.route as Route)} aria-current={active ? "page" : undefined}>
                {item.label}
              </Button>
            );
          })}
        </div>
        <div className="flex items-center gap-1">
          <ThemeToggle />
          <LocaleToggle />
          <div className="hidden items-center gap-2 lg:flex">
            {user ? (
              <>
                <Button variant="ghost" onClick={() => go("account")}>{label("account")}</Button>
                <Button variant="ghost" onClick={() => logout()}>{zh ? "登出" : "Sign out"}</Button>
              </>
            ) : (
              <>
                <Button variant="ghost" onClick={() => go("login")}>{label("login")}</Button>
                <Button onClick={() => go("register")}>{label("register")}</Button>
              </>
            )}
          </div>
          <Button
            type="button"
            variant="ghost"
            size="icon"
            className="size-11 xl:hidden"
            ref={menuButton}
            onClick={() => setMobileOpen(true)}
            aria-label={zh ? "開啟選單" : "Toggle menu"}
          >
            <Menu aria-hidden="true" className="size-5" />
          </Button>
        </div>
      </nav>
      <Dialog open={mobileOpen} onOpenChange={setMobileOpen}>
        <DialogContent onCloseAutoFocus={(event) => { event.preventDefault(); menuButton.current?.focus(); }} className="max-h-[80dvh] max-w-sm overflow-y-auto">
          <DialogTitle>{zh ? "導覽" : "Navigation"}</DialogTitle>
          <DialogDescription className="sr-only">{zh ? "Hivemind 網站導覽" : "Hivemind site navigation"}</DialogDescription>
          <nav className="grid gap-1">
            {site.nav.map((item: { id: string; label: string; route: string }) => (
              <Button key={item.id} className="h-11 justify-start" variant={item.route === route ? "secondary" : "ghost"} onClick={() => go(item.route as Route)}>
                {item.label}
              </Button>
            ))}
            <div className="my-2 border-t" />
            {user ? (
              <>
                <Button variant="ghost" className="h-11 justify-start" onClick={() => go("account")}>{label("account")}</Button>
                <Button variant="ghost" className="h-11 justify-start" onClick={() => { logout(); setMobileOpen(false); }}>{zh ? "登出" : "Sign out"}</Button>
              </>
            ) : (
              <>
                <Button variant="ghost" className="h-11 justify-start" onClick={() => go("login")}>{label("login")}</Button>
                <Button className="h-11 justify-start" onClick={() => go("register")}>{label("register")}</Button>
              </>
            )}
          </nav>
        </DialogContent>
      </Dialog>
    </header>
  );
}
