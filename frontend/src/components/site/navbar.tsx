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

const primaryRoutes: Route[] = ["home", "account", "security", "docs"];

export function Navbar() {
  const route = useAppStore((state) => state.route);
  const navigate = useAppStore((state) => state.navigate);
  const user = useAppStore((state) => state.user);
  const logout = useAppStore((state) => state.logout);
  const { locale } = useI18n();
  const site = useMemo(() => getSiteDefinition(locale), [locale]);
  const [mobileOpen, setMobileOpen] = useState(false);
  const menuButton = useRef<HTMLButtonElement>(null);
  const navItems = site.routes.filter((item) => primaryRoutes.includes(item.id as Route));

  return (
    <header className="fixed inset-x-0 top-0 z-40 border-b bg-background/95 backdrop-blur-sm">
      <nav aria-label="Main navigation" className="mx-auto flex h-18 max-w-7xl items-center justify-between gap-2 px-4 sm:px-6">
        <Button variant="ghost" onClick={() => navigate("home")} className="shrink-0 px-0 hover:bg-transparent" aria-label="Hivemind home"><HiveLogo withText /></Button>
        <div className="hidden items-center gap-1 md:flex">
          {navItems.map((item) => (
            <Button key={item.id} variant={route === item.id ? "secondary" : "ghost"} onClick={() => navigate(item.id as Route)} aria-current={route === item.id ? "page" : undefined}>{item.label}</Button>
          ))}
        </div>
        <div className="flex items-center gap-1">
          <ThemeToggle /><LocaleToggle />
          <div className="hidden items-center gap-2 lg:flex">
            {user ? <Button variant="ghost" onClick={() => logout()}>{locale === "zh" ? "登出" : "Sign out"}</Button> : <>
              <Button variant="ghost" onClick={() => navigate("login")}>{site.routes.find((entry) => entry.id === "login")?.label}</Button>
              <Button onClick={() => navigate("register")}>{site.routes.find((entry) => entry.id === "register")?.label}</Button>
            </>}
          </div>
          <Button type="button" variant="ghost" size="icon" className="size-11 lg:hidden" ref={menuButton} onClick={() => setMobileOpen(true)} aria-label="Toggle menu"><Menu aria-hidden="true" className="size-5" /></Button>
        </div>
      </nav>
      <Dialog open={mobileOpen} onOpenChange={setMobileOpen}>
        <DialogContent onCloseAutoFocus={(event) => { event.preventDefault(); menuButton.current?.focus(); }} className="max-h-[80dvh] max-w-sm overflow-y-auto">
          <DialogTitle>{locale === "zh" ? "導覽" : "Navigation"}</DialogTitle>
          <DialogDescription className="sr-only">Hivemind navigation</DialogDescription>
          <nav className="grid gap-1">
            {site.routes.map((item) => <Button key={item.id} className="h-11 justify-start" variant={route === item.id ? "secondary" : "ghost"} onClick={() => { navigate(item.id as Route); setMobileOpen(false); }}>{item.label}</Button>)}
            {user && <Button variant="ghost" className="h-11 justify-start" onClick={() => { logout(); setMobileOpen(false); }}>{locale === "zh" ? "登出" : "Sign out"}</Button>}
          </nav>
        </DialogContent>
      </Dialog>
    </header>
  );
}
