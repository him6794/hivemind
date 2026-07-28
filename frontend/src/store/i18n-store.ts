"use client";

import { create } from "zustand";
import { persist } from "zustand/middleware";

export type Locale = "en" | "zh";

interface I18nState {
  locale: Locale;
  setLocale: (locale: Locale) => void;
}

export const useI18n = create<I18nState>()(
  persist(
    (set) => ({
      locale: "zh",
      setLocale: (locale) => set({ locale }),
    }),
    {
      name: "hivemind-site-locale",
      partialize: (state) => ({ locale: state.locale }),
    }
  )
);
