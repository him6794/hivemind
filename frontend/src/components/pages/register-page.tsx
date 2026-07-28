"use client";

import { FormEvent, useState } from "react";
import { ArrowRight, UserPlus } from "lucide-react";
import { Button } from "@/components/ui/button";
import { useAppStore } from "@/store/app-store";
import { useI18n } from "@/store/i18n-store";
import { loginUser, registerUser } from "@/lib/hivemind-api";
import { Surface } from "./page-primitives";

export function RegisterPage() {
  const navigate = useAppStore((state) => state.navigate);
  const setAuth = useAppStore((state) => state.setAuth);
  const { locale } = useI18n();
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [status, setStatus] = useState("");
  const [loading, setLoading] = useState(false);

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (password !== confirm) {
      setStatus(locale === "zh" ? "兩次輸入的密碼不一致。" : "Passwords do not match.");
      return;
    }
    setLoading(true);
    setStatus(locale === "zh" ? "建立帳號中..." : "Creating account...");
    try {
      const registered = await registerUser(username, password) as { success?: boolean; message?: string };
      if (!registered.success) {
        throw new Error(registered.message || "Registration failed.");
      }
      const login = await loginUser(username, password) as { success?: boolean; token?: string; message?: string };
      if (!login.success || !login.token) {
        throw new Error(login.message || "Login failed.");
      }
      setAuth({ username }, login.token);
      setStatus(locale === "zh" ? "帳號建立完成。" : "Account created.");
      navigate("account");
    } catch (error) {
      setStatus(error instanceof Error ? error.message : "Registration failed.");
    } finally {
      setLoading(false);
    }
  }

  return (
    <section className="mx-auto grid min-h-screen max-w-7xl items-center gap-8 px-4 py-28 sm:px-6 lg:grid-cols-[1.1fr_0.9fr]">
      <div>
        <div className="mb-4 inline-flex items-center gap-2 rounded-full glass px-3 py-1 text-xs font-medium text-muted-foreground">
          <UserPlus className="size-3.5 text-honey" />
          {locale === "zh" ? "建立官方帳號" : "Create an official account"}
        </div>
        <h1 className="text-balance text-4xl font-semibold tracking-tight sm:text-6xl">
          {locale === "zh" ? "建立你的 Hivemind 帳號中心。" : "Create your Hivemind account center."}
        </h1>
        <p className="mt-4 max-w-xl text-muted-foreground">
          {locale === "zh"
            ? "官方網站提供註冊、登入與餘額可見性；真正的工作提交與節點控制會在你部署的 Master 或 Worker 節點上進行。"
            : "The official site handles registration, sign-in, and balance visibility. Real task and worker operations happen on the Master or Worker nodes you deploy."}
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
            />
          </div>
          <div>
            <label className="mb-2 block text-sm font-medium">{locale === "zh" ? "確認密碼" : "Confirm password"}</label>
            <input
              type="password"
              className="w-full rounded-xl border border-border bg-background/70 px-4 py-3 outline-none transition focus:border-honey/40"
              value={confirm}
              onChange={(event) => setConfirm(event.target.value)}
            />
          </div>

          <Button type="submit" disabled={loading} className="h-12 w-full rounded-xl bg-honey text-honey-foreground hover:bg-honey/90">
            {loading ? (locale === "zh" ? "處理中..." : "Working...") : (locale === "zh" ? "建立帳號" : "Create account")}
            <ArrowRight className="size-4" />
          </Button>
        </form>

        <div className="mt-4 rounded-xl border border-border/60 bg-background/40 p-4 text-sm text-muted-foreground">
          {status || (locale === "zh" ? "建立成功後會自動登入並進入帳號中心。" : "After creation, you will sign in automatically and enter the account center.")}
        </div>
      </Surface>
    </section>
  );
}
