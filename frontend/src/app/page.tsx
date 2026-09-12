"use client";

import { useEffect, useRef } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { Navbar } from "@/components/site/navbar";
import { Footer } from "@/components/site/footer";
import { CommandPalette } from "@/components/site/command-palette";
import { LandingPage } from "@/components/pages/landing-page";
import { LoginPage } from "@/components/pages/login-page";
import { RegisterPage } from "@/components/pages/register-page";
import { AccountPage } from "@/components/pages/account-page";
import { SecurityPage } from "@/components/pages/security-page";
import { DocsPage } from "@/components/pages/docs-page";
import { TermsPage } from "@/components/pages/terms-page";
import { useAppStore, type Route } from "@/store/app-store";

const fullscreenRoutes: Route[] = ["login", "register"];

function routeFromHash(): Route | null {
  const raw = window.location.hash.replace(/^#\/?/, "");
  if (!raw) {
    return "home";
  }
  return (["home", "login", "register", "account", "security", "docs", "terms"] as Route[])
    .find((entry) => entry === raw) || null;
}

export default function Home() {
  const route = useAppStore((state) => state.route);
  const navigate = useAppStore((state) => state.navigate);
  const hashSynced = useRef(false);
  const isFullscreen = fullscreenRoutes.includes(route);

  useEffect(() => {
    const syncRoute = () => {
      const next = routeFromHash();
      if (next !== null && next !== useAppStore.getState().route) {
        navigate(next);
      }
    };

    if (!hashSynced.current) {
      hashSynced.current = true;
      const next = routeFromHash() ?? "home";
      if (next !== route) {
        navigate(next);
        return;
      }
    }

    const nextHash = route === "home" ? "#/" : `#/${route}`;
    if (window.location.hash !== nextHash) {
      window.history.replaceState(null, "", nextHash);
    }
    window.addEventListener("hashchange", syncRoute);
    return () => window.removeEventListener("hashchange", syncRoute);
  }, [navigate, route]);

  return (
    <div className="relative flex min-h-screen flex-col overflow-x-hidden bg-background">
      {!isFullscreen ? <Navbar /> : null}
      <main className="flex-1">
        <AnimatePresence mode="wait">
          <motion.div
            key={route}
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            transition={{ duration: 0.25, ease: "easeOut" }}
            className="flex min-h-screen flex-col"
          >
            {renderRoute(route)}
          </motion.div>
        </AnimatePresence>
      </main>
      {!isFullscreen ? <Footer /> : null}
      <CommandPalette />
    </div>
  );
}

function renderRoute(route: Route) {
  switch (route) {
    case "home":
      return <LandingPage />;
    case "login":
      return <LoginPage />;
    case "register":
      return <RegisterPage />;
    case "account":
      return <AccountPage />;
    case "security":
      return <SecurityPage />;
    case "docs":
      return <DocsPage />;
    case "terms":
      return <TermsPage />;
    default:
      return <LandingPage />;
  }
}
