"use client";

import { motion, useReducedMotion } from "framer-motion";
import { ArrowRight, Check, FileText, Laptop, Lock, ShieldCheck, Users, Workflow } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { useAppStore } from "@/store/app-store";
import { useI18n } from "@/store/i18n-store";
import { getSiteDefinition } from "@/lib/hivemind-site-data.mjs";
import { Bullets, Receipt, type ReceiptRow } from "./site-blocks";

const hubIcons = [Workflow, Laptop, FileText];
const trustIcons = [Users, Check, ShieldCheck, Lock];

export function LandingPage() {
  const navigate = useAppStore((state) => state.navigate);
  const { locale } = useI18n();
  const site = getSiteDefinition(locale);
  const reducedMotion = useReducedMotion();
  // Same initial state on the server and the client; reduced motion only shortens the transition.
  const entrance = { opacity: 0, y: 12 };
  const zh = locale === "zh";

  return (
    <div>
      <section className="mx-auto grid max-w-7xl items-center gap-12 px-4 pb-16 pt-32 sm:px-6 sm:pt-40 lg:grid-cols-[1.1fr_0.9fr]">
        <motion.div initial={entrance} animate={{ opacity: 1, y: 0 }} transition={{ duration: reducedMotion ? 0 : 0.3, ease: [0.16, 1, 0.3, 1] }}>
          <p className="font-mono-tech text-xs uppercase tracking-[0.2em] text-honey">{site.hero.badge}</p>
          <h1 className="mt-4 max-w-2xl text-balance text-4xl font-semibold leading-[1.1] tracking-tight sm:text-5xl lg:text-6xl">
            {site.hero.title}
          </h1>
          <p className="mt-6 max-w-xl text-pretty text-lg leading-relaxed text-muted-foreground">{site.hero.body}</p>
          <div className="mt-8 flex flex-wrap gap-3">
            <Button size="lg" className="h-11" onClick={() => navigate("register")}>
              {site.hero.primaryCta}
              <ArrowRight aria-hidden="true" />
            </Button>
            <Button size="lg" variant="outline" className="h-11" onClick={() => navigate("share")}>
              {site.hero.secondaryCta}
            </Button>
          </div>
          <div className="mt-10">
            <Bullets items={site.hero.bullets} />
          </div>
        </motion.div>
        <motion.div initial={entrance} animate={{ opacity: 1, y: 0 }} transition={{ duration: reducedMotion ? 0 : 0.3, delay: reducedMotion ? 0 : 0.05 }}>
          <Receipt title={site.hero.mockTitle} note={site.hero.mockNote} rows={site.hero.mockRows as ReceiptRow[]} />
        </motion.div>
      </section>

      <section className="border-y bg-muted/30">
        <div className="mx-auto grid max-w-7xl gap-8 px-4 py-10 sm:grid-cols-3 sm:px-6">
          {site.sections.stats.map((item: { value: string; label: string }) => (
            <div key={item.label}>
              <p className="font-mono-tech text-3xl font-semibold text-honey">{item.value}</p>
              <p className="mt-2 text-sm text-muted-foreground">{item.label}</p>
            </div>
          ))}
        </div>
      </section>

      <section className="mx-auto max-w-7xl px-4 py-20 sm:px-6 sm:py-28">
        <h2 className="text-2xl font-semibold tracking-tight sm:text-3xl">{site.sections.hub.title}</h2>
        <div className="mt-10 grid gap-4 md:grid-cols-3">
          {site.sections.hub.items.map((item: { id: string; title: string; body: string; cta: string; route: string }, index: number) => {
            const Icon = hubIcons[index % hubIcons.length];
            return (
              <Card key={item.id} className="flex flex-col shadow-none">
                <CardHeader>
                  <Icon aria-hidden="true" className="mb-2 size-6 text-muted-foreground" />
                  <CardTitle className="text-lg">{item.title}</CardTitle>
                  <CardDescription className="leading-relaxed">{item.body}</CardDescription>
                </CardHeader>
                <CardContent className="mt-auto pt-2">
                  <Button variant="outline" onClick={() => navigate(item.route as Parameters<typeof navigate>[0])}>
                    {item.cta}
                    <ArrowRight aria-hidden="true" />
                  </Button>
                </CardContent>
              </Card>
            );
          })}
        </div>
      </section>

      <section className="border-y bg-muted/30">
        <div className="mx-auto max-w-7xl px-4 py-20 sm:px-6 sm:py-28">
          <h2 className="text-balance text-3xl font-semibold tracking-tight sm:text-4xl">{site.sections.workflowTitle}</h2>
          <p className="mt-4 max-w-2xl leading-relaxed text-muted-foreground">{site.sections.workflowBody}</p>
          <ol className="mt-12 grid gap-6 md:grid-cols-3">
            {site.sections.workflow.map((step: { step: string; title: string; body: string }) => (
              <li key={step.step} className="rounded-2xl border bg-background p-6">
                <p className="font-mono-tech text-sm text-honey">{step.step}</p>
                <h3 className="mt-4 text-lg font-semibold">{step.title}</h3>
                <p className="mt-3 text-sm leading-relaxed text-muted-foreground">{step.body}</p>
              </li>
            ))}
          </ol>
        </div>
      </section>

      <section className="mx-auto grid max-w-7xl gap-10 px-4 py-20 sm:px-6 sm:py-28 lg:grid-cols-[1fr_auto] lg:items-end">
        <div className="max-w-2xl">
          <p className="font-mono-tech text-xs uppercase tracking-[0.2em] text-honey">{zh ? "價格" : "Pricing"}</p>
          <h2 className="mt-3 text-balance text-3xl font-semibold tracking-tight sm:text-4xl">{site.sections.pricing.title}</h2>
          <div className="mt-6">
            <Bullets items={site.sections.pricing.points.slice(0, 2)} />
          </div>
        </div>
        <Button size="lg" variant="outline" className="h-11" onClick={() => navigate("pricing")}>
          {zh ? "查看價格" : "See pricing"}
          <ArrowRight aria-hidden="true" />
        </Button>
      </section>

      <section className="border-t bg-muted/30">
        <div className="mx-auto max-w-7xl px-4 py-20 sm:px-6 sm:py-28">
          <p className="font-mono-tech text-xs uppercase tracking-[0.2em] text-honey">{zh ? "信任" : "Trust"}</p>
          <h2 className="mt-3 text-balance text-3xl font-semibold tracking-tight sm:text-4xl">{site.sections.trust.title}</h2>
          <div className="mt-10 grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
            {site.sections.trust.cards.map((card: { title: string; body: string }, index: number) => {
              const Icon = trustIcons[index % trustIcons.length];
              return (
                <Card key={card.title} className="shadow-none">
                  <CardHeader>
                    <Icon aria-hidden="true" className="mb-2 size-5 text-honey" />
                    <CardTitle className="text-base">{card.title}</CardTitle>
                  </CardHeader>
                  <CardContent>
                    <p className="text-sm leading-relaxed text-muted-foreground">{card.body}</p>
                  </CardContent>
                </Card>
              );
            })}
          </div>
          <Button variant="link" className="mt-8 h-auto p-0" onClick={() => navigate("security")}>
            {site.sections.trust.linkLabel}
            <ArrowRight aria-hidden="true" />
          </Button>
        </div>
      </section>

      <section className="border-t">
        <div className="mx-auto flex max-w-7xl flex-col items-start gap-6 px-4 py-20 sm:flex-row sm:items-center sm:justify-between sm:px-6 sm:py-24">
          <div className="max-w-2xl">
            <h2 className="text-3xl font-semibold tracking-tight sm:text-4xl">{site.sections.finalCta.title}</h2>
            <p className="mt-4 leading-relaxed text-muted-foreground">{site.sections.finalCta.body}</p>
          </div>
          <div className="flex flex-wrap gap-3">
            <Button size="lg" className="h-11" onClick={() => navigate("register")}>
              {site.sections.finalCta.primaryCta}
              <ArrowRight aria-hidden="true" />
            </Button>
            <Button size="lg" variant="outline" className="h-11" onClick={() => navigate("docs")}>
              {site.sections.finalCta.secondaryCta}
            </Button>
          </div>
        </div>
      </section>
    </div>
  );
}
