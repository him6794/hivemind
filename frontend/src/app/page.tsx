"use client";

import { useEffect, useRef } from "react";
import { AnimatePresence, motion, useReducedMotion } from "framer-motion";
import { Navbar } from "@/components/site/navbar";
import { Footer } from "@/components/site/footer";
import { CommandPalette } from "@/components/site/command-palette";
import { LandingPage } from "@/components/pages/landing-page";
import { LoginPage } from "@/components/pages/login-page";
import { RegisterPage } from "@/components/pages/register-page";
import { AccountPage } from "@/components/pages/account-page";
import { DownloadPage } from "@/components/pages/download-page";
import { FaqPage } from "@/components/pages/faq-page";
import { PricingPage } from "@/components/pages/pricing-page";
import { SharePage } from "@/components/pages/share-page";
import { SecurityPage } from "@/components/pages/security-page";
import { DocsPage } from "@/components/pages/docs-page";
import { TermsPage } from "@/components/pages/terms-page";
import { useAppStore, type Route } from "@/store/app-store";
import { DEFAULT_DOCS_SLUG, docsSlugFrom } from "@/lib/docs/structure.mjs";

const fullscreenRoutes: Route[] = ["login", "register"];

const routes: Route[] = [
  "home",
  "login",
  "register",
  "account",
  "download",
  "pricing",
  "share",
  "security",
  "faq",
  "docs",
  "terms",
];

type Destination = { route: Route; docsSlug: string };

// "#/docs/loops" opens the loops page of the documentation. For other routes,
// anything after the first path segment is ignored.
function parseHash(): Destination | null {
  const raw = window.location.hash.replace(/^#\/?/, "");
  if (!raw) {
    return { route: "home", docsSlug: DEFAULT_DOCS_SLUG };
  }
  const [head, detail] = raw.split("/");
  const route = routes.find((entry) => entry === head);
  if (!route) {
    return null;
  }
  return { route, docsSlug: route === "docs" ? docsSlugFrom(detail) : DEFAULT_DOCS_SLUG };
}

export default function Home() {
  const route = useAppStore((state) => state.route);
  const docsSlug = useAppStore((state) => state.docsSlug);
  const navigate = useAppStore((state) => state.navigate);
  const hashSynced = useRef(false);
  const reducedMotion = useReducedMotion();
  const isFullscreen = fullscreenRoutes.includes(route);

  useEffect(() => {
    const isCurrent = (next: Destination) => {
      const state = useAppStore.getState();
      return next.route === state.route && (next.route !== "docs" || next.docsSlug === state.docsSlug);
    };

    const syncRoute = () => {
      const next = parseHash();
      if (next !== null && !isCurrent(next)) {
        navigate(next.route, next.docsSlug);
      }
    };

    if (!hashSynced.current) {
      hashSynced.current = true;
      const next = parseHash() ?? { route: "home" as Route, docsSlug: DEFAULT_DOCS_SLUG };
      if (!isCurrent(next)) {
        navigate(next.route, next.docsSlug);
        return;
      }
    }

    const nextHash = route === "home" ? "#/" : route === "docs" ? `#/docs/${docsSlug}` : `#/${route}`;
    if (window.location.hash !== nextHash) {
      window.history.replaceState(null, "", nextHash);
    }
    window.addEventListener("hashchange", syncRoute);
    return () => window.removeEventListener("hashchange", syncRoute);
  }, [navigate, route, docsSlug]);

  return (
    <div className="relative flex min-h-screen flex-col overflow-x-hidden bg-background">
      {!isFullscreen ? <Navbar /> : null}
      <main className="flex-1">
        <AnimatePresence mode="wait">
          <motion.div
            key={route}
            initial={{ opacity: 0, y: 6 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: reducedMotion ? 0 : -4 }}
            transition={{ duration: reducedMotion ? 0 : 0.18, ease: [0.16, 1, 0.3, 1] }}
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
    case "download":
      return <DownloadPage />;
    case "pricing":
      return <PricingPage />;
    case "share":
      return <SharePage />;
    case "faq":
      return <FaqPage />;
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
