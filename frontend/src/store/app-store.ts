"use client";

import { create } from "zustand";
import { persist } from "zustand/middleware";

export type Route =
  | "home"
  | "login"
  | "register"
  | "account"
  | "security"
  | "docs";

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

export const useAppStore = create<AppState>()(
  persist(
    (set) => ({
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
      logout: () => set({ user: null, token: null, route: "home" }),
      setCommandOpen: (commandOpen) => set({ commandOpen }),
    }),
    {
      name: "hivemind-site-auth",
      partialize: (state) => ({
        route: state.route,
        user: state.user,
        token: state.token,
      }),
    }
  )
);
