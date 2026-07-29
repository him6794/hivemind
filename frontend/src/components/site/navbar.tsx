"use client";

import { Menu, X, ArrowRight } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { HiveLogo } from "./hive-logo";
import { ThemeToggle } from "./theme-toggle";
import { LocaleToggle } from "./locale-toggle";
import { Button } from "@/components/ui/button";
import { useAppStore, type Route } from "@/store/app-store";
import { useI18n } from "@/store/i18n-store";
import { getSiteDefinition } from "@/lib/hivemind-site-data.mjs";
import { cn } from "@/lib/utils";

const primaryRoutes: Route[] = ["home", "account", "security", "docs"];

export function Navbar() {
  const route = useAppStore((state) => state.route);
  const navigate = useAppStore((state) => state.navigate);
  const user = useAppStore((state) => state.user);
  const logout = useAppStore((state) => state.logout);
  const { locale } = useI18n();
  const site = useMemo(() => getSiteDefinition(locale), [locale]);
  const [mobileOpen, setMobileOpen] = useState(false);
  const [scrolled, setScrolled] = useState(false);

  useEffect(() => {
    const onScroll = () => setScrolled(window.scrollY > 12);
    onScroll();
    window.addEventListener("scroll", onScroll, { passive: true });
    return () => window.removeEventListener("scroll", onScroll);
  }, []);

  const navItems = site.routes.filter((item) => primaryRoutes.includes(item.id as Route));

  return (
    <header className={cn("fixed inset-x-0 top-0 z-50 transition-all duration-500", scrolled ? "py-2.5" : "py-4")}>
      <div className="mx-auto max-w-7xl px-4 sm:px-6">
        <nav className={cn(
          "relative flex h-14 items-center justify-between rounded-2xl px-3 transition-all duration-500 sm:px-4",
          scrolled ? "glass-strong shadow-2xl shadow-black/30" : "border border-transparent"
        )}>
          <button
            type="button"
            onClick={() => {
              navigate("home");
              setMobileOpen(false);
            }}
            className="group flex items-center gap-2 rounded-xl px-1.5 transition-transform hover:scale-[1.02]"
            aria-label="Hivemind home"
          >
            <HiveLogo withText />
          </button>

          <div className="hidden items-center gap-1 md:flex">
            {navItems.map((item) => (
              <button
                key={item.id}
                type="button"
                onClick={() => navigate(item.id as Route)}
                className="relative rounded-lg px-3.5 py-2 text-sm font-medium text-muted-foreground transition-colors hover:text-foreground"
              >
                {item.label}
                {route === item.id ? (
                  <motion.span
                    layoutId="site-nav-active"
                    className="absolute inset-0 -z-10 rounded-lg bg-honey/10 ring-1 ring-honey/20"
                    transition={{ type: "spring", stiffness: 400, damping: 32 }}
                  />
                ) : null}
              </button>
            ))}
          </div>

          <div className="flex items-center gap-2">
            <ThemeToggle />
            <LocaleToggle />
            <div className="hidden items-center gap-1.5 sm:flex">
              {user ? (
                <>
                  <Button variant="ghost" size="sm" onClick={() => navigate("account")}>
                    {site.routes.find((entry) => entry.id === "account")?.label}
                  </Button>
                  <Button variant="ghost" size="sm" onClick={logout}>
                    {locale === "zh" ? "登出" : "Sign out"}
                  </Button>
                  <Button size="sm" onClick={() => navigate("docs")} className="group bg-honey text-honey-foreground hover:bg-honey/90">
                    {site.routes.find((entry) => entry.id === "docs")?.label}
                    <ArrowRight className="size-3.5 transition-transform group-hover:translate-x-0.5" />
                  </Button>
                </>
              ) : (
                <>
                  <Button variant="ghost" size="sm" onClick={() => navigate("login")}>
                    {site.routes.find((entry) => entry.id === "login")?.label}
                  </Button>
                  <Button size="sm" onClick={() => navigate("register")} className="group bg-honey text-honey-foreground hover:bg-honey/90">
                    {site.routes.find((entry) => entry.id === "register")?.label}
                    <ArrowRight className="size-3.5 transition-transform group-hover:translate-x-0.5" />
                  </Button>
                </>
              )}
            </div>

            <button
              type="button"
              className="inline-flex size-9 items-center justify-center rounded-lg text-muted-foreground hover:bg-accent hover:text-foreground md:hidden"
              onClick={() => setMobileOpen((value) => !value)}
              aria-label="Toggle menu"
            >
              {mobileOpen ? <X className="size-5" /> : <Menu className="size-5" />}
            </button>
          </div>
        </nav>

        <AnimatePresence>
          {mobileOpen ? (
            <motion.div
              initial={{ opacity: 0, y: -8, height: 0 }}
              animate={{ opacity: 1, y: 0, height: "auto" }}
              exit={{ opacity: 0, y: -8, height: 0 }}
              transition={{ duration: 0.25 }}
              className="mt-2 overflow-hidden rounded-2xl glass-strong p-2 md:hidden"
            >
              {site.routes.map((item) => (
                <button
                  key={item.id}
                  type="button"
                  onClick={() => {
                    navigate(item.id as Route);
                    setMobileOpen(false);
                  }}
                  className="block w-full rounded-lg px-4 py-3 text-left text-sm font-medium text-muted-foreground hover:bg-accent hover:text-foreground"
                >
                  {item.label}
                </button>
              ))}
              {user ? (
                <button
                  type="button"
                  onClick={() => {
                    logout();
                    setMobileOpen(false);
                  }}
                  className="block w-full rounded-lg px-4 py-3 text-left text-sm font-medium text-muted-foreground hover:bg-accent hover:text-foreground"
                >
                  {locale === "zh" ? "登出" : "Sign out"}
                </button>
              ) : null}
            </motion.div>
          ) : null}
        </AnimatePresence>
      </div>
    </header>
  );
}
