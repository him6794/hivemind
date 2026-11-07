"use client";

import { useI18n } from "@/store/i18n-store";
import { LEGAL_COPY } from "@/lib/legal-copy.mjs";
import { PageSection, Surface } from "./page-primitives";

export function LegalPage({ kind }: { kind: "terms" | "privacy" }) {
  const { locale } = useI18n();
  const copy = LEGAL_COPY[locale === "zh" ? "zh" : "en"][kind];

  return (
    <PageSection eyebrow={locale === "zh" ? "法律文件" : "Legal"} title={copy.title} body={copy.intro}>
      <Surface className="mt-10">
        <p className="text-sm text-muted-foreground">{copy.updated}</p>
        <div className="mt-8 divide-y divide-border/60">
          {copy.sections.map((section: { heading: string; paragraphs: string[] }) => (
            <section key={section.heading} className="space-y-4 py-8 first:pt-0 last:pb-0">
              <h2 className="text-lg font-semibold">{section.heading}</h2>
              <div className="space-y-3 text-sm leading-relaxed text-muted-foreground">
                {section.paragraphs.map((paragraph) => (
                  <p key={paragraph}>{paragraph}</p>
                ))}
              </div>
            </section>
          ))}
        </div>
      </Surface>
    </PageSection>
  );
}
