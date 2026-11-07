"use client";

import { create } from "zustand";
import { clearLegacyAuthStorage } from "@/lib/auth-storage-policy.mjs";
import { DEFAULT_DOCS_SLUG, docsSlugFrom } from "@/lib/docs/structure.mjs";
import { TOKEN_ROUTE_IDS } from "@/lib/route-hash-policy.mjs";
import { getAccount, logoutUser } from "@/lib/hivemind-api";

export type Route =
  | "home"
  | "login"
  | "register"
  | "account"
  | "forgot-password"
  | "forgot-username"
  | "verify-email"
  | "reset-password"
  | "download"
  | "pricing"
  | "share"
  | "security"
  | "faq"
  | "docs"
  | "terms"
  | "terms-of-service"
  | "privacy";

export interface AuthUser {
  username: string;
}

// Pages that need a signed-in account. A visitor who is signed out goes to the sign-in page first.
export const PROTECTED_ROUTES: Route[] = ["account"];

interface AppState {
  route: Route;
  docsSlug: string;
  // The token from an email link. Only the token routes use it.
  routeToken: string;
  // The signed-in account, or null. The session itself is an HttpOnly cookie the page cannot read.
  user: AuthUser | null;
  // False until the page has asked the server whether a session exists. Protected pages wait for the answer.
  sessionChecked: boolean;
  // The protected page the visitor asked for before signing in. Signing in opens it.
  returnRoute: Route | null;
  commandOpen: boolean;
  // The detail segment is the documentation slug for docs and the email token for the
  // token routes. Other routes ignore it.
  navigate: (route: Route, detail?: string) => void;
  setAuth: (user: AuthUser) => void;
  restoreSession: () => Promise<void>;
  logout: () => Promise<void>;
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

function scrollToTop() {
  if (typeof window !== "undefined") {
    window.scrollTo({ top: 0, behavior: "smooth" });
  }
}

function needsSignIn(route: Route, state: { sessionChecked: boolean; user: AuthUser | null }) {
  return PROTECTED_ROUTES.includes(route) && state.sessionChecked && state.user === null;
}

let authRevision = 0;
let restoreSequence = 0;

export const useAppStore = create<AppState>()((set, get) => ({
  route: "home",
  docsSlug: DEFAULT_DOCS_SLUG,
  routeToken: "",
  user: null,
  sessionChecked: false,
  returnRoute: null,
  commandOpen: false,
  navigate: (route, detail) => {
    const state = get();
    if (needsSignIn(route, state)) {
      set({ route: "login", routeToken: "", returnRoute: route });
      scrollToTop();
      return;
    }
    set({
      route,
      docsSlug: route === "docs" ? docsSlugFrom(detail) : state.docsSlug,
      routeToken: TOKEN_ROUTE_IDS.includes(route) ? (detail ?? "") : "",
      returnRoute: route === "login" ? state.returnRoute : null,
    });
    scrollToTop();
  },
  setAuth: (user) => {
    authRevision += 1;
    set({ user, sessionChecked: true });
  },
  restoreSession: async () => {
    const revision = authRevision;
    const sequence = ++restoreSequence;
    let user: AuthUser | null = null;
    try {
      const data = (await getAccount()) as { username?: unknown };
      if (typeof data.username === "string" && data.username !== "") {
        user = { username: data.username };
      }
    } catch {
      // No session, or the session has expired. Either way the visitor signs in again.
    }
    // A newer sign-in, sign-out, or restore owns the current authentication state.
    if (revision !== authRevision || sequence !== restoreSequence) return;
    set({ user, sessionChecked: true });
    const state = get();
    if (needsSignIn(state.route, state)) {
      set({ route: "login", routeToken: "", returnRoute: state.route });
    }
  },
  logout: async () => {
    const revision = ++authRevision;
    try {
      await logoutUser();
    } catch {
      // The local state is cleared either way. If the request failed, the cookie expires on its own.
    }
    if (revision !== authRevision) return;
    authRevision += 1;
    clearLegacyBrowserAuthStorage();
    set({ user: null, sessionChecked: true, route: "home", routeToken: "", returnRoute: null });
    scrollToTop();
  },
  setCommandOpen: (commandOpen) => set({ commandOpen }),
}));
