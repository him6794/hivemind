"use client";

import { create } from "zustand";
import { clearLegacyAuthStorage } from "@/lib/auth-storage-policy.mjs";
import { DEFAULT_DOCS_SLUG, docsSlugFrom } from "@/lib/docs/structure.mjs";

export type Route =
  | "home"
  | "login"
  | "register"
  | "account"
  | "download"
  | "pricing"
  | "share"
  | "security"
  | "faq"
  | "docs"
  | "terms";

export interface AuthUser {
  username: string;
}

interface AppState {
  route: Route;
  docsSlug: string;
  user: AuthUser | null;
  token: string | null;
  commandOpen: boolean;
  navigate: (route: Route, docsSlug?: string) => void;
  setAuth: (user: AuthUser, token: string) => void;
  logout: () => void;
  setCommandOpen: (open: boolean) => void;
}

function clearLegacyBrowserAuthStorage() {
  if (typeof window === "undefined") return;
  try {
    clearLegacyAuthStorage(window.localStorage);
  } catch {
    // Browser storage can be unavailable under strict privacy policies.
  }
}

clearLegacyBrowserAuthStorage();

export const useAppStore = create<AppState>()((set, get) => ({
  route: "home",
  docsSlug: DEFAULT_DOCS_SLUG,
  user: null,
  token: null,
  commandOpen: false,
  navigate: (route, docsSlug) => {
    set({ route, docsSlug: route === "docs" ? docsSlugFrom(docsSlug) : get().docsSlug });
    if (typeof window !== "undefined") {
      window.scrollTo({ top: 0, behavior: "smooth" });
    }
  },
  setAuth: (user, token) => set({ user, token }),
  logout: () => {
    clearLegacyBrowserAuthStorage();
    set({ user: null, token: null, route: "home" });
  },
  setCommandOpen: (commandOpen) => set({ commandOpen }),
}));
