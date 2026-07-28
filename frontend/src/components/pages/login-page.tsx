"use client";

import { FormEvent, useState } from "react";
import { ArrowRight, Shield } from "lucide-react";
import { Button } from "@/components/ui/button";
import { useAppStore } from "@/store/app-store";
import { useI18n } from "@/store/i18n-store";
import { loginUser } from "@/lib/hivemind-api";
import { Surface } from "./page-primitives";

export function LoginPage() {
  const navigate = useAppStore((state) => state.navigate);
  const setAuth = useAppStore((state) => state.setAuth);
  const { locale } = useI18n();
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [status, setStatus] = useState("");
  const [loading, setLoading] = useState(false);

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setLoading(true);
    setStatus(locale === "zh" ? "登入中..." : "Signing in...");
    try {
      const data = await loginUser(username, password) as { success?: boolean; token?: string; message?: string };
      if (!data.success || !data.token) {
        throw new Error(data.message || "Login failed.");
      }
      setAuth({ username }, data.token);
      setStatus(locale === "zh" ? "登入成功。" : "Signed in.");
      navigate("account");
    } catch (error) {
      setStatus(error instanceof Error ? error.message : "Login failed.");
    } finally {
      setLoading(false);
    }
  }

  return (
    <section className="mx-auto grid min-h-screen max-w-7xl items-center gap-8 px-4 py-28 sm:px-6 lg:grid-cols-[1.1fr_0.9fr]">
      <div>
        <div className="mb-4 inline-flex items-center gap-2 rounded-full glass px-3 py-1 text-xs font-medium text-muted-foreground">
          <Shield className="size-3.5 text-honey" />
          {locale === "zh" ? "安全登入" : "Secure sign in"}
        </div>
        <h1 className="text-balance text-4xl font-semibold tracking-tight sm:text-6xl">
          {locale === "zh" ? "回到你的 Hivemind 帳號中心。" : "Return to your Hivemind account center."}
        </h1>
        <p className="mt-4 max-w-xl text-muted-foreground">
          {locale === "zh"
            ? "登入後可查看帳號資訊、CPT 餘額，以及部署 Master 或 Worker 節點的下一步指引。"
            : "After signing in, you can review account state, CPT balance, and the next steps for deploying a Master or Worker node."}
        </p>
      </div>

      <Surface className="border-border/80 bg-card/70 p-8">
        <form className="space-y-4" onSubmit={handleSubmit}>
          <div>
            <label className="mb-2 block text-sm font-medium">{locale === "zh" ? "使用者名稱" : "Username"}</label>
            <input
              className="w-full rounded-xl border border-border bg-background/70 px-4 py-3 outline-none transition focus:border-honey/40"
              value={username}
              onChange={(event) => setUsername(event.target.value)}
              placeholder="team-ops"
            />
          </div>
          <div>
            <label className="mb-2 block text-sm font-medium">{locale === "zh" ? "密碼" : "Password"}</label>
            <input
              type="password"
              className="w-full rounded-xl border border-border bg-background/70 px-4 py-3 outline-none transition focus:border-honey/40"
              value={password}
              onChange={(event) => setPassword(event.target.value)}
              placeholder={locale === "zh" ? "至少 8 個字元" : "At least 8 characters"}
            />
          </div>

          <Button type="submit" disabled={loading} className="h-12 w-full rounded-xl bg-honey text-honey-foreground hover:bg-honey/90">
            {loading ? (locale === "zh" ? "處理中..." : "Working...") : (locale === "zh" ? "登入" : "Sign in")}
            <ArrowRight className="size-4" />
          </Button>
        </form>

        <div className="mt-4 rounded-xl border border-border/60 bg-background/40 p-4 text-sm text-muted-foreground">
          {status || (locale === "zh" ? "登入後可進入帳號中心。" : "Sign in to enter the account center.")}
        </div>

        <button type="button" onClick={() => navigate("register")} className="mt-4 text-sm text-muted-foreground transition-colors hover:text-foreground">
          {locale === "zh" ? "還沒有帳號？立即建立" : "Need an account? Create one"}
        </button>
      </Surface>
    </section>
  );
}
