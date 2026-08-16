"use client";

import { useEffect, useState } from "react";
import { motion, useReducedMotion } from "framer-motion";
import { ArrowRight } from "lucide-react";
import { Button } from "@/components/ui/button";
import { useAppStore } from "@/store/app-store";
import { useI18n } from "@/store/i18n-store";
import { getSiteDefinition } from "@/lib/hivemind-site-data.mjs";

type Copy = { zh: string; en: string };
const t = (locale: string, copy: Copy) => (locale === "zh" ? copy.zh : copy.en);

// Every figure below is the real output of the example the docs publish. It is
// pinned by published_doc_example.rs in executor-rs, so this panel cannot drift
// into being a mockup of numbers that never happened.
const RECEIPT = {
  jobId: "row-score-001",
  runtime: "managed-function-v0",
  meter: [
    { key: "executed_ops", value: 80 },
    { key: "usage_units", value: 80 },
    { key: "function_calls", value: 2 },
    { key: "loop_iterations", value: 2 },
    { key: "output_bytes", value: 13 },
  ],
  charges: [
    { label: { zh: "基本呼叫費", en: "base invocation" }, amount: "1 CPT" },
    { label: { zh: "用量 80 × 1 CPT", en: "usage 80 × 1 CPT" }, amount: "80 CPT" },
  ],
  total: "81 CPT",
};

function useCountUp(target: number, run: boolean, durationMs = 900) {
  const [value, setValue] = useState(run ? 0 : target);

  useEffect(() => {
    if (!run) {
      setValue(target);
      return;
    }
    let frame = 0;
    const started = performance.now();
    const tick = (now: number) => {
      const progress = Math.min(1, (now - started) / durationMs);
      // Ease out so the meter settles rather than stopping dead.
      setValue(Math.round(target * (1 - Math.pow(1 - progress, 3))));
      if (progress < 1) frame = requestAnimationFrame(tick);
    };
    frame = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(frame);
  }, [target, run, durationMs]);

  return value;
}

/** The signature element: a real execution receipt, the artifact this product
 *  is actually about. The bill is derived from it and proven before it settles. */
function ReceiptPanel({ locale }: { locale: string }) {
  const reduceMotion = useReducedMotion();
  const animate = !reduceMotion;
  const ops = useCountUp(80, animate);

  return (
    <motion.figure
      initial={animate ? { opacity: 0, y: 18 } : false}
      animate={{ opacity: 1, y: 0 }}
      transition={{ delay: 0.25, duration: 0.6, ease: [0.22, 1, 0.36, 1] }}
      className="relative rounded-xl border border-border bg-card/70 p-6 sm:p-7"
    >
      <figcaption className="flex items-baseline justify-between gap-4 border-b border-border pb-4">
        <span className="eyebrow">{t(locale, { zh: "執行回執", en: "Execution receipt" })}</span>
        <span className="tabular text-xs text-muted-foreground">{RECEIPT.runtime}</span>
      </figcaption>

      <dl className="mt-5 space-y-2.5">
        <div className="flex items-baseline justify-between gap-4">
          <dt className="text-xs text-muted-foreground">{t(locale, { zh: "工作", en: "job" })}</dt>
          <dd className="tabular text-xs">{RECEIPT.jobId}</dd>
        </div>
        {RECEIPT.meter.map((row) => (
          <div key={row.key} className="flex items-baseline justify-between gap-4">
            <dt className="tabular text-xs text-muted-foreground">{row.key}</dt>
            <dd className="tabular text-sm">{row.key === "executed_ops" ? ops : row.value}</dd>
          </div>
        ))}
      </dl>

      <div className="mt-5 space-y-2.5 border-t border-border pt-5">
        {RECEIPT.charges.map((charge) => (
          <div key={charge.amount} className="flex items-baseline justify-between gap-4">
            <dt className="text-xs text-muted-foreground">{t(locale, charge.label)}</dt>
            <dd className="tabular text-sm">{charge.amount}</dd>
          </div>
        ))}
      </div>

      {/* The stamp lands on the total, because the total is the claim being
          verified. Kept in flow so it can never collide with the caption. */}
      <div className="mt-5 flex flex-wrap items-center justify-between gap-3 border-t border-border pt-5">
        <span className="eyebrow">{t(locale, { zh: "應付", en: "total" })}</span>
        <div className="flex items-center gap-4">
          <motion.span
            initial={animate ? { opacity: 0, scale: 1.35, rotate: -8 } : false}
            animate={{ opacity: 1, scale: 1, rotate: -6 }}
            transition={{ delay: 1.15, duration: 0.4, ease: "backOut" }}
            className="stamp rounded px-2.5 py-1 font-mono-tech text-xs uppercase"
          >
            {t(locale, { zh: "已驗證", en: "verified" })}
          </motion.span>
          <span className="tabular text-2xl font-semibold">{RECEIPT.total}</span>
        </div>
      </div>

      <p className="mt-6 text-xs leading-relaxed text-muted-foreground">
        {t(locale, {
          zh: "這是文件裡那個範例的實際回執。帳單從回執推導，並在結算前經過證明驗證。",
          en: "The real receipt from the example in the docs. The bill is derived from it, and verified by proof before it settles.",
        })}
      </p>
    </motion.figure>
  );
}

export function LandingPage() {
  const navigate = useAppStore((state) => state.navigate);
  const { locale } = useI18n();
  const site = getSiteDefinition(locale);
  const reduceMotion = useReducedMotion();
  const rise = reduceMotion
    ? {}
    : { initial: { opacity: 0, y: 14 }, animate: { opacity: 1, y: 0 } };

  return (
    <div className="relative">
      <section className="relative overflow-hidden pb-16 pt-32 sm:pt-40">
        <div className="mx-auto max-w-7xl px-4 sm:px-6">
          <div className="grid items-start gap-12 lg:grid-cols-12 lg:gap-16">
            <div className="lg:col-span-7">
              <motion.div {...rise} transition={{ duration: 0.5 }} className="eyebrow">
                {site.hero.badge}
              </motion.div>

              <motion.h1
                {...rise}
                transition={{ delay: 0.06, duration: 0.6, ease: [0.22, 1, 0.36, 1] }}
                className="font-display mt-5 text-balance text-4xl leading-[1.08] sm:text-5xl lg:text-6xl"
              >
                {site.hero.title}
              </motion.h1>

              <motion.p
                {...rise}
                transition={{ delay: 0.12, duration: 0.6 }}
                className="mt-6 max-w-xl text-pretty leading-relaxed text-muted-foreground"
              >
                {site.hero.body}
              </motion.p>

              <motion.div
                {...rise}
                transition={{ delay: 0.18, duration: 0.6 }}
                className="mt-9 flex flex-col gap-3 sm:flex-row"
              >
                <Button
                  size="lg"
                  onClick={() => navigate("register")}
                  className="group h-12 rounded-lg bg-honey px-7 text-honey-foreground hover:bg-honey/90"
                >
                  {site.hero.primaryCta}
                  <ArrowRight className="size-4 transition-transform group-hover:translate-x-0.5" />
                </Button>
                <Button
                  size="lg"
                  variant="outline"
                  onClick={() => navigate("docs")}
                  className="h-12 rounded-lg px-7"
                >
                  {site.hero.secondaryCta}
                </Button>
              </motion.div>

              <motion.ul
                {...rise}
                transition={{ delay: 0.24, duration: 0.6 }}
                className="mt-10 max-w-xl divide-y divide-border border-y border-border"
              >
                {site.hero.bullets.map((bullet: string) => (
                  <li key={bullet} className="py-3 text-sm text-muted-foreground">
                    {bullet}
                  </li>
                ))}
              </motion.ul>
            </div>

            <div className="lg:col-span-5">
              <ReceiptPanel locale={locale} />
            </div>
          </div>
        </div>
      </section>

      {/* The four facts read as one meter strip, not four identical cards. */}
      <section className="border-y border-border">
        <div className="mx-auto max-w-7xl px-4 sm:px-6">
          <dl className="grid divide-y divide-border sm:grid-cols-2 sm:divide-y-0 lg:grid-cols-4 lg:divide-x">
            {site.sections.stats.map((stat: { value: string; label: string }, index: number) => (
              <div
                key={stat.label}
                className={`py-8 lg:px-8 ${index === 0 ? "lg:pl-0" : ""} ${
                  index === site.sections.stats.length - 1 ? "lg:pr-0" : ""
                } ${index % 2 === 1 ? "sm:border-l sm:border-border lg:border-l-0" : ""}`}
              >
                <dt className="tabular text-3xl font-semibold">{stat.value}</dt>
                <dd className="mt-2 text-sm leading-relaxed text-muted-foreground">{stat.label}</dd>
              </div>
            ))}
          </dl>
        </div>
      </section>

      <section className="py-24 sm:py-28">
        <div className="mx-auto max-w-7xl px-4 sm:px-6">
          <div className="eyebrow">{locale === "zh" ? "為什麼是 Hivemind" : "Why Hivemind"}</div>
          <h2 className="font-display mt-4 max-w-2xl text-balance text-2xl leading-tight sm:text-4xl">
            {locale === "zh"
              ? "每一筆帳單都能追回到一份證明。"
              : "Every bill traces back to a proof."}
          </h2>

          <div className="mt-14 grid gap-x-12 gap-y-10 md:grid-cols-2">
            {site.sections.features.map((feature: { title: string; body: string }) => (
              <motion.div
                key={feature.title}
                initial={reduceMotion ? false : { opacity: 0, y: 16 }}
                whileInView={{ opacity: 1, y: 0 }}
                viewport={{ once: true, amount: 0.3 }}
                transition={{ duration: 0.5 }}
                className="border-t border-border pt-6"
              >
                <h3 className="text-lg font-semibold">{feature.title}</h3>
                <p className="mt-3 max-w-md leading-relaxed text-muted-foreground">{feature.body}</p>
              </motion.div>
            ))}
          </div>
        </div>
      </section>

      {/* Numbered because this genuinely is a sequence: each step is only
          available once the one before it is done. */}
      <section id="workflow" className="border-t border-border py-24 sm:py-28">
        <div className="mx-auto max-w-7xl px-4 sm:px-6">
          <div className="eyebrow">{locale === "zh" ? "運作方式" : "How it works"}</div>
          <h2 className="font-display mt-4 max-w-2xl text-balance text-2xl leading-tight sm:text-4xl">
            {locale === "zh"
              ? "從帳號到第一份結果，四個步驟。"
              : "Account to first result, in four steps."}
          </h2>

          <ol className="mt-14 grid gap-px overflow-hidden rounded-xl border border-border bg-border md:grid-cols-2 lg:grid-cols-4">
            {site.sections.workflow.map((step: { step: string; title: string; body: string }) => (
              <li key={step.step} className="bg-background p-7">
                <span className="tabular text-xs text-honey">{step.step}</span>
                <h3 className="mt-4 text-base font-semibold">{step.title}</h3>
                <p className="mt-2.5 text-sm leading-relaxed text-muted-foreground">{step.body}</p>
              </li>
            ))}
          </ol>
        </div>
      </section>
    </div>
  );
}
