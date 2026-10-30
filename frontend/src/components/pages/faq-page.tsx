"use client";

import { useI18n } from "@/store/i18n-store";
import { getSiteDefinition } from "@/lib/hivemind-site-data.mjs";
import { PageSection } from "./page-primitives";

export function FaqPage() {
  const { locale } = useI18n();
  const site = getSiteDefinition(locale);
  const status = site.sections.status;
  const faq = site.sections.faq;

  return (
    <PageSection eyebrow={locale === "zh" ? "常見問題" : "FAQ"} title={faq.pageTitle}>
      <section className="mt-12 grid gap-8 lg:grid-cols-[0.9fr_1.1fr]">
        <div>
          <h2 className="text-2xl font-semibold tracking-tight sm:text-3xl">{status.title}</h2>
          <p className="mt-4 leading-relaxed text-muted-foreground">{status.body}</p>
        </div>
        <dl className="divide-y rounded-2xl border">
          {status.rows.map((row: { label: string; value: string }) => (
            <div key={row.label} className="grid gap-1 px-6 py-4 sm:grid-cols-2 sm:gap-6">
              <dt className="text-sm text-muted-foreground">{row.label}</dt>
              <dd className="text-sm font-medium">{row.value}</dd>
            </div>
          ))}
        </dl>
      </section>

      <section className="mt-20 max-w-3xl">
        <h2 className="text-2xl font-semibold tracking-tight sm:text-3xl">{faq.title}</h2>
        <div className="mt-8 divide-y rounded-2xl border bg-background">
          {faq.items.map((item: { question: string; answer: string }) => (
            <details key={item.question} className="group px-6 py-5">
              <summary className="flex cursor-pointer list-none items-center justify-between gap-6 text-base font-medium">
                {item.question}
                <span aria-hidden="true" className="text-xl leading-none text-muted-foreground transition-transform group-open:rotate-45">
                  +
                </span>
              </summary>
              <p className="mt-3 text-sm leading-relaxed text-muted-foreground">{item.answer}</p>
            </details>
          ))}
        </div>
      </section>
    </PageSection>
  );
}
