"use client";

import { useEffect, useState } from "react";
import { ArrowRight, RefreshCw, Wallet } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { getBalance } from "@/lib/hivemind-api";
import { parseAccountBalance } from "@/lib/account-policy.mjs";
import { useI18n } from "@/store/i18n-store";
import { useAppStore } from "@/store/app-store";

export function AccountPage() {
  const { locale } = useI18n();
  const user = useAppStore((state) => state.user);
  const token = useAppStore((state) => state.token);
  const navigate = useAppStore((state) => state.navigate);
  const [balance, setBalance] = useState<number | null>(null);
  const [status, setStatus] = useState("");
  const [loading, setLoading] = useState(false);
  const [refresh, setRefresh] = useState(0);

  useEffect(() => {
    if (!token) {
      setBalance(null);
      setLoading(false);
      setStatus("");
      return;
    }
    const authToken = token;
    let cancelled = false;
    setLoading(true);
    setStatus("");
    async function load() {
      try {
        const data = await getBalance(authToken) as { balance?: number; cpt_balance?: number };
        if (!cancelled) setBalance(parseAccountBalance(data));
      } catch (error) {
        if (!cancelled) setStatus(error instanceof Error ? error.message : "Failed to load account details.");
      } finally {
        if (!cancelled) setLoading(false);
      }
    }
    void load();
    return () => { cancelled = true; };
  }, [locale, token, refresh]);

  return (
    <section className="mx-auto w-full max-w-5xl px-4 pb-16 pt-28 sm:px-6">
      <div className="mb-8 flex flex-wrap items-center justify-between gap-4">
        <h1 className="text-3xl font-semibold tracking-tight">{locale === "zh" ? "帳號中心" : "Account Center"}</h1>
        <Button variant="outline" onClick={() => navigate(token ? "docs" : "login")}>{token ? (locale === "zh" ? "查看文件" : "Open docs") : (locale === "zh" ? "登入" : "Sign in")}</Button>
      </div>
      <div className="grid gap-4 md:grid-cols-2">
        <Card>
          <CardHeader className="flex flex-row items-start justify-between gap-4">
            <div className="space-y-2"><CardTitle className="text-base">{locale === "zh" ? "CPT 餘額" : "CPT balance"}</CardTitle><CardDescription>{user?.username || (locale === "zh" ? "尚未登入" : "Not signed in")}</CardDescription></div>
            <Wallet aria-hidden="true" className="size-5 text-muted-foreground" />
          </CardHeader>
          <CardContent aria-busy={loading}>
            {loading && balance === null ? <Skeleton className="h-10 w-32" aria-label={locale === "zh" ? "讀取餘額" : "Loading balance"} /> : <p className="text-4xl font-semibold tracking-tight">{balance === null ? "—" : balance.toFixed(2)}</p>}
            <p role="status" aria-live="polite" className="mt-4 break-words text-sm text-muted-foreground">{status}</p>
            {token && <Button className="mt-4" variant="outline" disabled={loading} onClick={() => setRefresh((value) => value + 1)}><RefreshCw aria-hidden="true" className={loading ? "animate-spin" : ""} />{locale === "zh" ? "重新整理" : "Refresh"}</Button>}
          </CardContent>
        </Card>
        <Card>
          <CardHeader><CardTitle className="text-base">{locale === "zh" ? "客戶端" : "Clients"}</CardTitle><CardDescription>{locale === "zh" ? "使用 Master 送出工作，使用 Worker 分享電腦。" : "Use Master to submit work and Worker to share your computer."}</CardDescription></CardHeader>
          <CardContent><Button onClick={() => navigate("docs")}>{locale === "zh" ? "下載與安裝" : "Download and setup"}<ArrowRight aria-hidden="true" /></Button></CardContent>
        </Card>
      </div>
    </section>
  );
}
