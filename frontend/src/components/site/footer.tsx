"use client";

import { Cpu, HardDrive, Network, ShieldCheck } from "lucide-react";
import { useMemo } from "react";
import { HiveLogo } from "./hive-logo";
import { useAppStore } from "@/store/app-store";
import { useI18n } from "@/store/i18n-store";
import { getSiteDefinition } from "@/lib/hivemind-site-data.mjs";

const icons = [Network, Cpu, HardDrive, ShieldCheck];

export function Footer() {
  const navigate = useAppStore((state) => state.navigate);
  const { locale } = useI18n();
  const site = useMemo(() => getSiteDefinition(locale), [locale]);

  return (
    <footer className="mt-auto border-t border-border/60 bg-background/60">
      <div className="mx-auto max-w-7xl px-4 py-14 sm:px-6">
        <div className="grid gap-10 lg:grid-cols-[1.4fr_1fr_1fr]">
          <div className="space-y-4">
            <HiveLogo withText />
            <p className="max-w-md text-sm leading-relaxed text-muted-foreground">
              {locale === "zh"
                ? "Hivemind 讓團隊把繁重工作交給分散式算力處理，從提交、追蹤到取回結果，都留在同一個清楚的產品體驗裡。"
                : "Hivemind helps teams send heavy work to distributed compute, then track progress and collect results through one clear product experience."}
            </p>
          </div>

          <div>
            <h4 className="mb-4 text-xs font-semibold uppercase tracking-wider text-muted-foreground">
              {locale === "zh" ? "網站導覽" : "Navigation"}
            </h4>
            <ul className="space-y-2.5">
              {site.routes.map((route) => (
                <li key={route.id}>
                  <button
                    type="button"
                    onClick={() => navigate(route.id as import("@/store/app-store").Route)}
                    className="text-sm text-muted-foreground transition-colors hover:text-foreground"
                  >
                    {route.label}
                  </button>
                </li>
              ))}
            </ul>
          </div>

          <div>
            <h4 className="mb-4 text-xs font-semibold uppercase tracking-wider text-muted-foreground">
              {locale === "zh" ? "平台重點" : "Platform Highlights"}
            </h4>
            <div className="grid gap-3">
              {site.sections.features.map((feature, index) => {
                const Icon = icons[index % icons.length];
                return (
                  <div key={feature.title} className="flex items-start gap-3 rounded-xl border border-border/60 bg-card/40 p-3">
                    <span className="mt-0.5 flex size-8 items-center justify-center rounded-lg bg-honey/10 text-honey">
                      <Icon className="size-4" />
                    </span>
                    <div>
                      <div className="text-sm font-medium">{feature.title}</div>
                      <div className="text-xs leading-relaxed text-muted-foreground">{feature.body}</div>
                    </div>
                  </div>
                );
              })}
            </div>
          </div>
        </div>

        <div className="mt-12 flex flex-col items-start justify-between gap-4 border-t border-border/60 pt-6 text-xs text-muted-foreground sm:flex-row sm:items-center">
          <p>
            © {new Date().getFullYear()} Hivemind. {locale === "zh" ? "分散式算力工作平台。" : "Distributed compute work platform."}
          </p>
          <div className="flex items-center gap-4">
            <span className="inline-flex items-center gap-1.5">
              <span className="relative flex size-2">
                <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-foreground opacity-75" />
                <span className="relative inline-flex size-2 rounded-full bg-foreground" />
              </span>
              {locale === "zh" ? "官方網站" : "Official site"}
            </span>
            <span className="font-mono-tech">Hivemind</span>
          </div>
        </div>
      </div>
    </footer>
  );
}
