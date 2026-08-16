"use client";

import { useEffect } from "react";
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

export default function Home() {
  const route = useAppStore((state) => state.route);
  const navigate = useAppStore((state) => state.navigate);
  const isFullscreen = fullscreenRoutes.includes(route);

  useEffect(() => {
    const fromHash = () => {
      const raw = window.location.hash.replace(/^#\/?/, "");
      const next = (["home", "login", "register", "account", "security", "docs", "terms"] as Route[])
        .find((entry) => entry === raw) || "home";
      navigate(next);
    };
    fromHash();
    window.addEventListener("hashchange", fromHash);
    return () => window.removeEventListener("hashchange", fromHash);
  }, [navigate]);

  useEffect(() => {
    window.history.replaceState(null, "", route === "home" ? "#/" : `#/${route}`);
  }, [route]);

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
