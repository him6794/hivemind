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
import { ForgotPasswordPage, ForgotUsernamePage } from "@/components/pages/account-recovery-page";
import { ResetPasswordPage } from "@/components/pages/reset-password-page";
import { VerifyEmailPage } from "@/components/pages/verify-email-page";
import { DownloadPage } from "@/components/pages/download-page";
import { FaqPage } from "@/components/pages/faq-page";
import { PricingPage } from "@/components/pages/pricing-page";
import { SharePage } from "@/components/pages/share-page";
import { SecurityPage } from "@/components/pages/security-page";
import { DocsPage } from "@/components/pages/docs-page";
import { TermsPage } from "@/components/pages/terms-page";
import { LegalPage } from "@/components/pages/legal-page";
import { useAppStore, type Route } from "@/store/app-store";
import { useI18n } from "@/store/i18n-store";
import { DEFAULT_DOCS_SLUG } from "@/lib/docs/structure.mjs";
import { parseRouteHash, routeHash, TOKEN_ROUTE_IDS } from "@/lib/route-hash-policy.mjs";

const fullscreenRoutes: Route[] = [
  "login",
  "register",
  "forgot-password",
  "forgot-username",
  "verify-email",
  "reset-password",
];

const routes: Route[] = [
  "home",
  "login",
  "register",
  "account",
  "forgot-password",
  "forgot-username",
  "verify-email",
  "reset-password",
  "download",
  "pricing",
  "share",
  "security",
  "faq",
  "docs",
  "terms",
  "terms-of-service",
  "privacy",
];

type Destination = { route: Route; docsSlug: string; token: string };

// "#/docs/loops" opens the loops page of the documentation. "#/verify-email/<token>" and
// "#/reset-password/<token>" open the pages for email links. For other routes, anything
// after the first path segment is ignored.
function parseHash(hash: string = window.location.hash): Destination | null {
  const parsed = parseRouteHash(hash, routes);
  return parsed === null ? null : { ...parsed, route: parsed.route as Route };
}

// The detail segment a destination needs when it is navigated to.
function detailOf(next: Destination): string | undefined {
  if (next.route === "docs") return next.docsSlug;
  if (TOKEN_ROUTE_IDS.includes(next.route)) return next.token;
  return undefined;
}

// True when the store already shows the destination, so the hash needs no navigation.
function isCurrentDestination(next: Destination) {
  const state = useAppStore.getState();
  return (
    next.route === state.route &&
    (next.route !== "docs" || next.docsSlug === state.docsSlug) &&
    (!TOKEN_ROUTE_IDS.includes(next.route) || next.token === state.routeToken)
  );
}

// Writes the store's route into the address bar when the two differ. replaceState goes through Next.js's
// patched history methods, so Next.js learns the new URL as well.
function syncHashToStore() {
  const state = useAppStore.getState();
  const nextHash = routeHash(state.route, { docsSlug: state.docsSlug, token: state.routeToken });
  if (window.location.hash !== nextHash) {
    window.history.replaceState(null, "", nextHash);
  }
}

export default function Home() {
  const route = useAppStore((state) => state.route);
  const docsSlug = useAppStore((state) => state.docsSlug);
  const routeToken = useAppStore((state) => state.routeToken);
  const navigate = useAppStore((state) => state.navigate);
  const sessionChecked = useAppStore((state) => state.sessionChecked);
  const restoreSession = useAppStore((state) => state.restoreSession);
  const hashSynced = useRef(false);

  // A reload keeps the visitor signed in: the server reads the session cookie and says who it belongs to.
  useEffect(() => {
    void restoreSession();
  }, [restoreSession]);
  const reducedMotion = useReducedMotion();
  const isFullscreen = fullscreenRoutes.includes(route);

  // Attached once. Re-attaching it on every route change left a gap in which a hash change was lost.
  // The destination is read from the event, not from window.location: Next.js can write its older copy of
  // the URL back over a hash change before this listener runs, and the listener would then see the old hash.
  useEffect(() => {
    const syncRoute = (event: HashChangeEvent) => {
      const next = parseHash(new URL(event.newURL).hash);
      if (next !== null && !isCurrentDestination(next)) {
        navigate(next.route, detailOf(next));
      }
      syncHashToStore();
    };
    window.addEventListener("hashchange", syncRoute);
    return () => window.removeEventListener("hashchange", syncRoute);
  }, [navigate]);

  useEffect(() => {
    if (!hashSynced.current) {
      hashSynced.current = true;
      const next: Destination = parseHash() ?? { route: "home", docsSlug: DEFAULT_DOCS_SLUG, token: "" };
      if (!isCurrentDestination(next)) {
        navigate(next.route, detailOf(next));
      }
    }
    // The store is read here, not this render's values. In StrictMode the second run of this effect would
    // otherwise write the home hash over the route the first run has just set.
    syncHashToStore();
  }, [navigate, route, docsSlug, routeToken]);

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
            {renderRoute(route, sessionChecked)}
          </motion.div>
        </AnimatePresence>
      </main>
      {!isFullscreen ? <Footer /> : null}
      <CommandPalette />
    </div>
  );
}

function renderRoute(route: Route, sessionChecked: boolean) {
  switch (route) {
    case "home":
      return <LandingPage />;
    case "login":
      return <LoginPage />;
    case "register":
      return <RegisterPage />;
    case "account":
      return sessionChecked ? <AccountPage /> : <SessionPending />;
    case "forgot-password":
      return <ForgotPasswordPage />;
    case "forgot-username":
      return <ForgotUsernamePage />;
    case "verify-email":
      return <VerifyEmailPage />;
    case "reset-password":
      return <ResetPasswordPage />;
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
    case "terms-of-service":
      return <LegalPage kind="terms" />;
    case "privacy":
      return <LegalPage kind="privacy" />;
    default:
      return <LandingPage />;
  }
}

// Shown while the page asks the server whether the visitor is signed in, so a protected page does not flash.
function SessionPending() {
  const { locale } = useI18n();
  return (
    <section role="status" aria-live="polite" className="mx-auto flex min-h-[60vh] w-full max-w-md items-center justify-center px-4 text-sm text-muted-foreground">
      {locale === "zh" ? "正在確認登入狀態…" : "Checking your sign-in…"}
    </section>
  );
}
