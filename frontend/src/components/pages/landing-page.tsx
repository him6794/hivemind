"use client";

import { motion } from "framer-motion";
import { ArrowRight, Check, Compass, ShieldCheck, UserRound, Waypoints } from "lucide-react";
import { Button } from "@/components/ui/button";
import { useAppStore } from "@/store/app-store";
import { useI18n } from "@/store/i18n-store";
import { getSiteDefinition } from "@/lib/hivemind-site-data.mjs";
import { NetworkBackground, AmbientBlobs } from "@/components/site/network-background";

export function LandingPage() {
  const navigate = useAppStore((state) => state.navigate);
  const { locale } = useI18n();
  const site = getSiteDefinition(locale);

  return (
    <div className="relative">
      <section className="relative overflow-hidden pb-20 pt-32 sm:pb-28 sm:pt-40">
        <AmbientBlobs />
        <div className="absolute inset-0 bg-grid bg-grid-fade" aria-hidden="true" />
        <NetworkBackground />

        <div className="relative mx-auto max-w-7xl px-4 sm:px-6">
          <div className="mx-auto max-w-4xl text-center">
            <motion.div
              initial={{ opacity: 0, y: 16 }}
              animate={{ opacity: 1, y: 0 }}
              className="mx-auto mb-6 inline-flex items-center gap-2 rounded-full glass px-3.5 py-1.5 text-xs font-medium text-muted-foreground"
            >
              <span className="relative flex size-1.5">
                <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-honey opacity-75" />
                <span className="relative inline-flex size-1.5 rounded-full bg-honey" />
              </span>
              {site.hero.badge}
              <ArrowRight className="size-3" />
            </motion.div>

            <motion.h1
              initial={{ opacity: 0, y: 24 }}
              animate={{ opacity: 1, y: 0 }}
              transition={{ delay: 0.08 }}
              className="text-balance text-4xl font-semibold leading-[1.05] tracking-tight sm:text-6xl lg:text-7xl"
            >
              {site.hero.title}
            </motion.h1>

            <motion.p
              initial={{ opacity: 0, y: 24 }}
              animate={{ opacity: 1, y: 0 }}
              transition={{ delay: 0.16 }}
              className="mx-auto mt-6 max-w-3xl text-pretty text-base leading-relaxed text-muted-foreground sm:text-lg"
            >
              {site.hero.body}
            </motion.p>

            <motion.div
              initial={{ opacity: 0, y: 24 }}
              animate={{ opacity: 1, y: 0 }}
              transition={{ delay: 0.24 }}
              className="mt-9 flex flex-col items-center justify-center gap-3 sm:flex-row"
            >
              <Button
                size="lg"
                onClick={() => navigate("register")}
                className="group h-12 rounded-xl bg-honey px-7 text-honey-foreground hover:bg-honey/90 glow-honey-sm"
              >
                {site.hero.primaryCta}
                <ArrowRight className="size-4 transition-transform group-hover:translate-x-0.5" />
              </Button>
              <Button
                size="lg"
                variant="outline"
                onClick={() => navigate("docs")}
                className="h-12 rounded-xl px-7"
              >
                {site.hero.secondaryCta}
              </Button>
            </motion.div>

            <div className="mt-8 flex flex-wrap items-center justify-center gap-x-6 gap-y-2 text-xs text-muted-foreground">
              {site.hero.bullets.map((bullet) => (
                <span key={bullet} className="inline-flex items-center gap-1.5">
                  <Check className="size-3.5 text-honey" />
                  {bullet}
                </span>
              ))}
            </div>
          </div>

          <div className="mx-auto mt-16 grid max-w-6xl gap-4 md:grid-cols-4">
            {site.sections.stats.map((stat, index) => (
              <motion.div
                key={stat.label}
                initial={{ opacity: 0, y: 16 }}
                whileInView={{ opacity: 1, y: 0 }}
                viewport={{ once: true }}
                transition={{ delay: index * 0.08 }}
                className="rounded-2xl border border-border/60 bg-card/40 p-6 glass"
              >
                <div className="font-mono-tech text-2xl font-semibold text-gradient-honey">{stat.value}</div>
                <div className="mt-2 text-sm text-muted-foreground">{stat.label}</div>
              </motion.div>
            ))}
          </div>
        </div>
      </section>

      <section className="relative border-y border-border/60 bg-card/20 py-24 sm:py-32">
        <div className="mx-auto max-w-7xl px-4 sm:px-6">
          <div className="mx-auto max-w-2xl text-center">
            <div className="mb-3 font-mono-tech text-xs uppercase tracking-widest text-honey">
              {locale === "zh" ? "官方定位" : "Official Role"}
            </div>
            <h2 className="text-balance text-3xl font-semibold tracking-tight sm:text-4xl">
              {locale === "zh"
                ? "首頁負責說明產品、帳號與部署邊界，不負責操作任務。"
                : "The homepage explains product value, account access, and deployment boundaries."}
            </h2>
          </div>

          <div className="mt-14 grid gap-4 md:grid-cols-2 xl:grid-cols-4">
            {site.sections.features.map((feature, index) => {
              const icons = [Compass, UserRound, Waypoints, ShieldCheck];
              const Icon = icons[index % icons.length];
              return (
                <motion.div
                  key={feature.title}
                  initial={{ opacity: 0, y: 20 }}
                  whileInView={{ opacity: 1, y: 0 }}
                  viewport={{ once: true }}
                  transition={{ delay: index * 0.08 }}
                  whileHover={{ y: -4 }}
                  className="group relative overflow-hidden rounded-2xl border border-border/60 bg-card/40 p-6 transition-colors hover:border-honey/30"
                >
                  <div className="mb-4 inline-flex size-11 items-center justify-center rounded-xl bg-honey/10 ring-1 ring-honey/20">
                    <Icon className="size-5 text-honey" />
                  </div>
                  <h3 className="mb-2 text-base font-semibold">{feature.title}</h3>
                  <p className="text-sm leading-relaxed text-muted-foreground">{feature.body}</p>
                </motion.div>
              );
            })}
          </div>
        </div>
      </section>

      <section id="workflow" className="relative py-24 sm:py-32">
        <div className="mx-auto max-w-7xl px-4 sm:px-6">
          <div className="mx-auto max-w-2xl text-center">
            <div className="mb-3 font-mono-tech text-xs uppercase tracking-widest text-honey">
              {locale === "zh" ? "使用流程" : "Flow"}
            </div>
            <h2 className="text-balance text-3xl font-semibold tracking-tight sm:text-4xl">
              {locale === "zh"
                ? "從建立帳號到部署節點，責任邊界一路保持清楚。"
                : "From account creation to node deployment, the boundary stays clear."}
            </h2>
          </div>
          <div className="relative mt-16 grid gap-6 md:grid-cols-4">
            {site.sections.workflow.map((step, index) => (
              <motion.div
                key={step.step}
                initial={{ opacity: 0, y: 24 }}
                whileInView={{ opacity: 1, y: 0 }}
                viewport={{ once: true }}
                transition={{ delay: index * 0.1 }}
                className="rounded-2xl border border-border/60 bg-card/40 p-6"
              >
                <div className="mb-2 font-mono-tech text-xs text-honey">STEP {step.step}</div>
                <h3 className="mb-2 text-lg font-semibold">{step.title}</h3>
                <p className="text-sm leading-relaxed text-muted-foreground">{step.body}</p>
              </motion.div>
            ))}
          </div>
        </div>
      </section>
    </div>
  );
}
