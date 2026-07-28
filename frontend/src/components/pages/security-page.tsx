"use client";

import { useI18n } from "@/store/i18n-store";
import { getSiteDefinition } from "@/lib/hivemind-site-data.mjs";
import { PageSection, Surface } from "./page-primitives";

export function SecurityPage() {
  const { locale } = useI18n();
  const site = getSiteDefinition(locale);

  return (
    <PageSection
      eyebrow={locale === "zh" ? "信任與安全" : "Trust & Safety"}
      title={locale === "zh" ? "給大眾的是簡單入口，給團隊的是可檢查、可追蹤的執行流程。" : "A simple public entry point, backed by a process teams can inspect and trust."}
      body={locale === "zh"
        ? "Hivemind 的重點不是把技術細節丟到首頁，而是讓使用者知道：帳號、工作、算力與結果都有清楚邊界，重要紀錄也能被追蹤。"
        : "Hivemind keeps the public experience clear while preserving the controls serious teams need: boundaries, traceability, and accountable execution."}
    >
      <div className="mt-10 grid gap-4 lg:grid-cols-2">
        {site.sections.security.map((item) => (
          <Surface key={item}>
            <p className="text-sm leading-relaxed text-muted-foreground">{item}</p>
          </Surface>
        ))}
      </div>
    </PageSection>
  );
}
