"use client";

import { useEffect, useMemo, useState } from "react";
import { ArrowRight, CreditCard, Download, ShieldCheck, Wallet } from "lucide-react";
import { Button } from "@/components/ui/button";
import { getBalance } from "@/lib/hivemind-api";
import { parseAccountBalance } from "@/lib/account-policy.mjs";
import { getSiteDefinition } from "@/lib/hivemind-site-data.mjs";
import { useI18n } from "@/store/i18n-store";
import { useAppStore } from "@/store/app-store";
import { KeyValue, PageSection, Surface } from "./page-primitives";

export function AccountPage() {
  const { locale } = useI18n();
  const site = useMemo(() => getSiteDefinition(locale), [locale]);
  const user = useAppStore((state) => state.user);
  const token = useAppStore((state) => state.token);
  const navigate = useAppStore((state) => state.navigate);
  const [balance, setBalance] = useState<number | null>(null);
  const [status, setStatus] = useState("");

  useEffect(() => {
    if (!token) {
      setBalance(null);
      setStatus(locale === "zh" ? "請先登入以查看帳號資訊。" : "Sign in to review account details.");
      return;
    }

    let cancelled = false;

    const load = async () => {
      setStatus(locale === "zh" ? "讀取帳號資料中..." : "Loading account details...");
      try {
        const data = await getBalance(token) as { balance?: number; cpt_balance?: number };
        if (cancelled) return;
        const nextBalance = parseAccountBalance(data);
        setBalance(nextBalance);
        setStatus("");
      } catch (error) {
        if (cancelled) return;
        setStatus(error instanceof Error ? error.message : "Failed to load account details.");
      }
    };

    load();
    return () => {
      cancelled = true;
    };
  }, [locale, token]);

  const panels = site.sections.account.panels;

  return (
    <PageSection
      eyebrow={locale === "zh" ? "帳號中心" : "Account Center"}
      title={locale === "zh" ? "你的帳號與餘額都在這裡。" : "Your account and balance, in one place."}
      body={site.sections.account.summary}
      className="pt-32"
    >
      <div className="mt-10 grid gap-4 lg:grid-cols-[1.1fr_0.9fr]">
        <Surface className="border-border/80 bg-card/70">
          <div className="flex items-start justify-between gap-4">
            <div>
              <div className="inline-flex items-center gap-2 rounded-full bg-honey/10 px-3 py-1 text-xs font-medium text-honey">
                <Wallet className="size-3.5" />
                {locale === "zh" ? "帳號摘要" : "Account summary"}
              </div>
              <h3 className="mt-4 text-2xl font-semibold">
                {user ? user.username : (locale === "zh" ? "尚未登入" : "Not signed in")}
              </h3>
              <p className="mt-2 text-sm text-muted-foreground">
                {locale === "zh"
                  ? "工作從你的 Master 送出，再由排程器交給符合條件的網路 worker。"
                  : "Work leaves through your Master, then the scheduler assigns it to an eligible network worker."}
              </p>
            </div>
            <Button variant="outline" onClick={() => navigate(token ? "docs" : "login")}>
              {token ? (locale === "zh" ? "查看文件" : "Open docs") : (locale === "zh" ? "前往登入" : "Sign in")}
            </Button>
          </div>

          <div className="mt-8 grid gap-4 sm:grid-cols-3">
            <KeyValue
              label={locale === "zh" ? "CPT 餘額" : "CPT balance"}
              value={<span className="font-mono-tech text-3xl">{balance === null ? "..." : balance.toFixed(2)}</span>}
            />
            <KeyValue
              label={locale === "zh" ? "工作在哪跑" : "Where work runs"}
              value={locale === "zh" ? "符合條件的網路 worker" : "An eligible network worker"}
            />
            <KeyValue
              label={locale === "zh" ? "下一步" : "Next step"}
              value={locale === "zh" ? "部署 Master 或 Worker" : "Deploy a Master or Worker"}
            />
          </div>

          {status ? <div aria-live="polite" className="mt-6 rounded-xl border border-border/60 bg-background/40 p-4 text-sm text-muted-foreground">{status}</div> : null}
        </Surface>

        <div className="grid gap-4">
          {panels.map((panel, index) => {
            const icons = [CreditCard, Download, ShieldCheck];
            const Icon = icons[index % icons.length];
            return (
              <Surface key={panel.title} className="bg-card/50">
                <div className="inline-flex size-10 items-center justify-center rounded-xl bg-honey/10 text-honey">
                  <Icon className="size-4.5" />
                </div>
                <h3 className="mt-4 text-lg font-semibold">{panel.title}</h3>
                <p className="mt-2 text-sm leading-relaxed text-muted-foreground">{panel.body}</p>
              </Surface>
            );
          })}
        </div>
      </div>

      <Surface className="mt-8 border-border/80 bg-card/50">
        <div className="flex flex-col gap-4 md:flex-row md:items-center md:justify-between">
          <div>
            <h3 className="text-lg font-semibold">
              {locale === "zh" ? "要送出工作，還是貢獻算力？" : "Send work, or contribute compute?"}
            </h3>
            <p className="mt-2 text-sm text-muted-foreground">
              {locale === "zh"
                ? "送工作部署 Master，貢獻算力部署 Worker。"
                : "Deploy a Master node to send work. Deploy a Worker node to contribute compute."}
            </p>
          </div>
          <Button onClick={() => navigate("docs")} className="bg-honey text-honey-foreground hover:bg-honey/90">
            {locale === "zh" ? "前往部署文件" : "Go to deployment docs"}
            <ArrowRight className="size-4" />
          </Button>
        </div>
      </Surface>
    </PageSection>
  );
}
