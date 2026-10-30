"use client";

import { useEffect } from "react";
import { useI18n } from "@/store/i18n-store";

// The root layout is rendered on the server, before the stored locale is known.
// Keep the document language and title in step with the locale after mount.
export function DocumentLocale() {
  const { locale } = useI18n();

  useEffect(() => {
    document.documentElement.lang = locale === "zh" ? "zh-Hant" : "en";
    document.title = locale === "zh" ? "Hivemind 官方網站" : "Hivemind | Official site";
  }, [locale]);

  return null;
}
