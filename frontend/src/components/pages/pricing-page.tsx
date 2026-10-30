"use client";

import { ArrowRight } from "lucide-react";
import { Button } from "@/components/ui/button";
import { useAppStore } from "@/store/app-store";
import { useI18n } from "@/store/i18n-store";
import { getSiteDefinition } from "@/lib/hivemind-site-data.mjs";
import { Bullets, Receipt, type ReceiptRow } from "./site-blocks";
import { PageSection, Surface } from "./page-primitives";

export function PricingPage() {
  const navigate = useAppStore((state) => state.navigate);
  const { locale } = useI18n();
  const site = getSiteDefinition(locale);
  const copy = site.sections.pricing;

  return (
    <PageSection eyebrow={locale === "zh" ? "價格" : "Pricing"} title={copy.title} body={copy.body}>
      <div className="mt-12 grid gap-12 lg:grid-cols-2 lg:items-start">
        <div>
          <Bullets items={copy.points} />
          <p className="mt-8 text-sm leading-relaxed text-muted-foreground">{copy.creditNote}</p>
        </div>
        <Receipt title={copy.example.title} note={copy.example.note} rows={copy.example.rows as ReceiptRow[]} />
      </div>

      <section className="mt-20">
        <h2 className="text-2xl font-semibold tracking-tight sm:text-3xl">{site.sections.featuresTitle}</h2>
        <div className="mt-8 grid gap-4 md:grid-cols-2">
          {site.sections.features.map((feature: { title: string; body: string }) => (
            <Surface key={feature.title}>
              <h3 className="text-base font-semibold">{feature.title}</h3>
              <p className="mt-2 text-sm leading-relaxed text-muted-foreground">{feature.body}</p>
            </Surface>
          ))}
        </div>
      </section>

      <div className="mt-16 flex flex-wrap gap-3">
        <Button size="lg" className="h-11" onClick={() => navigate("register")}>
          {site.hero.primaryCta}
          <ArrowRight aria-hidden="true" />
        </Button>
        <Button size="lg" variant="outline" className="h-11" onClick={() => navigate("docs")}>
          {site.sections.finalCta.secondaryCta}
        </Button>
      </div>
    </PageSection>
  );
}
