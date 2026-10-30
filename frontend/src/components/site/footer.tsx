"use client";

import { useMemo } from "react";
import { HiveLogo } from "./hive-logo";
import { useAppStore, type Route } from "@/store/app-store";
import { useI18n } from "@/store/i18n-store";
import { getSiteDefinition } from "@/lib/hivemind-site-data.mjs";

export function Footer() {
  const navigate = useAppStore((state) => state.navigate);
  const { locale } = useI18n();
  const zh = locale === "zh";
  const site = useMemo(() => getSiteDefinition(locale), [locale]);

  return (
    <footer className="mt-auto border-t bg-background">
      <div className="mx-auto max-w-7xl px-4 py-14 sm:px-6">
        <div className="grid gap-10 md:grid-cols-[1.4fr_repeat(4,1fr)]">
          <div className="space-y-4">
            <HiveLogo withText />
            <p className="max-w-xs text-sm leading-relaxed text-muted-foreground">{site.brand.tagline}</p>
          </div>
          {site.footer.map((column: { title: string; links: { label: string; route: string; slug?: string }[] }) => (
            <div key={column.title}>
              <h2 className="mb-4 text-xs font-semibold uppercase tracking-wider text-muted-foreground">{column.title}</h2>
              <ul className="space-y-2.5">
                {column.links.map((link) => (
                  <li key={link.label}>
                    <button
                      type="button"
                      onClick={() => navigate(link.route as Route, link.slug)}
                      className="text-sm text-muted-foreground transition-colors hover:text-foreground"
                    >
                      {link.label}
                    </button>
                  </li>
                ))}
              </ul>
            </div>
          ))}
        </div>
        <div className="mt-12 flex flex-col items-start justify-between gap-4 border-t pt-6 text-xs text-muted-foreground sm:flex-row sm:items-center">
          <p>© {new Date().getFullYear()} Hivemind</p>
          <p className="font-mono-tech">{zh ? "首次公開版本" : "First public version"}</p>
        </div>
      </div>
    </footer>
  );
}
