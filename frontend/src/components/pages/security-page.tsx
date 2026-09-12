"use client";

import { useI18n } from "@/store/i18n-store";
import { getSiteDefinition } from "@/lib/hivemind-site-data.mjs";
import { PageSection, Surface } from "./page-primitives";

export function SecurityPage() {
  const { locale } = useI18n();
  const site = getSiteDefinition(locale);
  const security = site.sections.security;

  return (
    <PageSection
      eyebrow={locale === "zh" ? "信任與安全" : "Trust & Safety"}
      title={locale === "zh" ? "單一電腦的回報不能直接扣款。" : "One computer cannot charge your account by itself."}
      body={locale === "zh"
        ? "扣款之前，幾台參與工作的電腦會回報結果，網路會比較它們並確認是否達成共識。無法確認的工作就直接失敗。"
        : "Before anything is charged, several participating computers report their results and the network checks whether they agree. A task the network cannot confirm fails outright."}
    >
      <div className="mt-10 grid gap-4 lg:grid-cols-2">
        {security.items.map((item: string) => (
          <Surface key={item}>
            <p className="text-sm leading-relaxed text-muted-foreground">{item}</p>
          </Surface>
        ))}
      </div>

      <h2 className="mt-16 text-balance text-2xl font-semibold tracking-tight sm:text-3xl">
        {security.pipelineTitle}
      </h2>
      <div className="mt-6 grid gap-4 md:grid-cols-2 xl:grid-cols-4">
        {security.pipeline.map((stage: { step: string; title: string; body: string }) => (
          <Surface key={stage.step}>
            <div className="font-mono-tech text-xs text-honey">STEP {stage.step}</div>
            <h3 className="mt-2 text-base font-semibold">{stage.title}</h3>
            <p className="mt-2 text-sm leading-relaxed text-muted-foreground">{stage.body}</p>
          </Surface>
        ))}
      </div>

      <Surface className="mt-10 border-honey/25 bg-honey/[0.04]">
        <h2 className="text-lg font-semibold">{security.caveatsTitle}</h2>
        <ul className="mt-4 space-y-2.5">
          {security.caveats.map((caveat: string) => (
            <li key={caveat} className="flex gap-2.5 text-sm leading-relaxed text-muted-foreground">
              <span aria-hidden="true" className="mt-2 size-1 shrink-0 rounded-full bg-honey" />
              <span>{caveat}</span>
            </li>
          ))}
        </ul>
      </Surface>
    </PageSection>
  );
}
