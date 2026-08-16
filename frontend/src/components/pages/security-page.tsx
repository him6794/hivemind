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
      title={locale === "zh" ? "worker 說它用了多少，不算數。" : "What the worker says it used does not count."}
      body={locale === "zh"
        ? "計費之前，網路端會自己驗證一份密碼學證明，並確認它來自被釘選的那份程式。驗不過的工作就直接失敗。"
        : "Before anything is billed, the network verifies a cryptographic proof on its own side and confirms it came from the exact program it pinned. A job that fails that check fails outright."}
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
