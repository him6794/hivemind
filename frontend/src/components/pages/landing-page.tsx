"use client";

import { motion, useReducedMotion } from "framer-motion";
import { ArrowRight, Laptop, Workflow } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { useAppStore } from "@/store/app-store";
import { useI18n } from "@/store/i18n-store";
import { getSiteDefinition } from "@/lib/hivemind-site-data.mjs";

export function LandingPage() {
  const navigate = useAppStore((state) => state.navigate);
  const { locale } = useI18n();
  const site = getSiteDefinition(locale);
  const reducedMotion = useReducedMotion();
  const entrance = reducedMotion ? false : { opacity: 0, y: 12 };

  return (
    <div>
      <section className="mx-auto grid max-w-7xl items-center gap-12 px-4 pb-20 pt-32 sm:px-6 sm:pt-40 lg:grid-cols-[1.2fr_1fr]">
        <motion.div initial={entrance} animate={{ opacity: 1, y: 0 }} transition={{ duration: reducedMotion ? 0 : 0.3, ease: [0.16, 1, 0.3, 1] }}>
          <h1 className="max-w-2xl text-balance text-4xl font-semibold leading-tight tracking-tight sm:text-5xl lg:text-6xl">{site.hero.title}</h1>
          <p className="mt-6 max-w-xl text-pretty leading-relaxed text-muted-foreground">{locale === "zh" ? "執行工作，或分享你的電腦。設定可接受的額度上限，其餘交給 Hivemind。" : "Run work or share your computer. Set the credit limit you accept and let Hivemind handle the rest."}</p>
          <div className="mt-8 flex flex-wrap gap-3">
            <Button size="lg" className="h-11" onClick={() => navigate("register")}>{site.hero.primaryCta}<ArrowRight aria-hidden="true" /></Button>
            <Button size="lg" className="h-11" variant="outline" onClick={() => navigate("docs")}>{site.hero.secondaryCta}</Button>
          </div>
        </motion.div>
        <motion.div className="grid gap-4" initial={entrance} animate={{ opacity: 1, y: 0 }} transition={{ duration: reducedMotion ? 0 : 0.3, delay: reducedMotion ? 0 : 0.05 }}>
          <Card>
            <CardHeader>
              <Workflow aria-hidden="true" className="mb-2 size-6 text-muted-foreground" />
              <CardTitle>Master</CardTitle>
              <CardDescription>{locale === "zh" ? "透過自己的客戶端送出工作、查看進度與結果。" : "Submit work and review progress and results in your own client."}</CardDescription>
            </CardHeader>
            <CardContent><Button variant="outline" onClick={() => navigate("docs")}>{locale === "zh" ? "安裝說明" : "Setup guide"}<ArrowRight aria-hidden="true" /></Button></CardContent>
          </Card>
          <Card>
            <CardHeader>
              <Laptop aria-hidden="true" className="mb-2 size-6 text-muted-foreground" />
              <CardTitle>Worker</CardTitle>
              <CardDescription>{locale === "zh" ? "將電腦的可用運算資源分享給網路。" : "Share your computer’s available capacity with the network."}</CardDescription>
            </CardHeader>
            <CardContent><Button variant="outline" onClick={() => navigate("docs")}>{locale === "zh" ? "安裝說明" : "Setup guide"}<ArrowRight aria-hidden="true" /></Button></CardContent>
          </Card>
        </motion.div>
      </section>
      <section className="border-y bg-muted/30">
        <div className="mx-auto grid max-w-7xl gap-4 px-4 py-12 sm:px-6 md:grid-cols-2">
          {site.sections.features.map((feature: { title: string; body: string }) => (
            <Card key={feature.title} className="shadow-none">
              <CardHeader><CardTitle className="text-base">{feature.title}</CardTitle></CardHeader>
              <CardContent><p className="text-sm leading-relaxed text-muted-foreground">{feature.body}</p></CardContent>
            </Card>
          ))}
        </div>
      </section>
      <section id="workflow" className="mx-auto max-w-7xl px-4 py-16 sm:px-6">
        <h2 className="text-2xl font-semibold tracking-tight">{locale === "zh" ? "開始使用" : "Getting started"}</h2>
        <ol className="mt-8 grid gap-6 md:grid-cols-2 lg:grid-cols-4">
          {site.sections.workflow.map((step: { step: string; title: string; body: string }) => (
            <li key={step.step} className="border-t pt-5">
              <h3 className="font-medium">{step.title}</h3>
              <p className="mt-2 text-sm leading-relaxed text-muted-foreground">{step.body}</p>
            </li>
          ))}
        </ol>
      </section>
    </div>
  );
}
