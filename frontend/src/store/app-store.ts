"use client";

import { create } from "zustand";
import { clearLegacyAuthStorage } from "@/lib/auth-storage-policy.mjs";

export type Route =
  | "home"
  | "login"
  | "register"
  | "account"
  | "security"
  | "docs"
  | "terms";

export interface AuthUser {
  username: string;
}

interface AppState {
  route: Route;
  user: AuthUser | null;
  token: string | null;
  commandOpen: boolean;
  navigate: (route: Route) => void;
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

export const useAppStore = create<AppState>()((set) => ({
  route: "home",
  user: null,
  token: null,
  commandOpen: false,
  navigate: (route) => {
    set({ route });
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
