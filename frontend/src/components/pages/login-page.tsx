"use client";

import { FormEvent, useState } from "react";
import { ArrowRight } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Card, CardContent, CardHeader } from "@/components/ui/card";
import { HiveLogo } from "@/components/site/hive-logo";
import { ThemeToggle } from "@/components/site/theme-toggle";
import { LocaleToggle } from "@/components/site/locale-toggle";
import { useAppStore } from "@/store/app-store";
import { useI18n } from "@/store/i18n-store";
import { loginUser } from "@/lib/hivemind-api";
import { validateLoginInput } from "@/lib/auth-policy.mjs";

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
    if (loading) return;
    const validation = validateLoginInput(username, password);
    if (!validation.ok) {
      setStatus(locale === "zh" ? "請輸入使用者名稱與密碼。" : "Enter your username and password.");
      return;
    }
    setLoading(true);
    setStatus(locale === "zh" ? "登入中..." : "Signing in...");
    try {
      const data = await loginUser(validation.username, password) as { success?: boolean; token?: string; message?: string };
      if (!data.success || !data.token) throw new Error(data.message || "Login failed.");
      setUsername(validation.username);
      setAuth({ username: validation.username }, data.token);
      setStatus("");
      navigate("account");
    } catch (error) {
      setStatus(error instanceof Error ? error.message : "Login failed.");
    } finally {
      setLoading(false);
    }
  }

  return (
    <section className="mx-auto flex min-h-screen w-full max-w-md flex-col justify-center gap-6 px-4 py-12">
      <div className="flex items-center justify-between gap-3">
        <Button variant="ghost" className="px-0 hover:bg-transparent" aria-label="Hivemind home" onClick={() => navigate("home")}><HiveLogo withText /></Button>
        <div className="flex items-center gap-1"><ThemeToggle /><LocaleToggle /></div>
      </div>
      <Card>
        <CardHeader>
          <h1 className="text-2xl font-semibold tracking-tight">{locale === "zh" ? "登入 Hivemind，開始使用你的額度。" : "Sign in to Hivemind and use your credits."}</h1>
        </CardHeader>
        <CardContent>
          <form className="space-y-5" onSubmit={handleSubmit} aria-busy={loading}>
            <div className="space-y-2">
              <Label htmlFor="login-username">{locale === "zh" ? "使用者名稱" : "Username"}</Label>
              <Input id="login-username" className="h-11" value={username} onChange={(event) => setUsername(event.target.value)} autoComplete="username" disabled={loading} required />
            </div>
            <div className="space-y-2">
              <Label htmlFor="login-password">{locale === "zh" ? "密碼" : "Password"}</Label>
              <Input id="login-password" className="h-11" type="password" value={password} onChange={(event) => setPassword(event.target.value)} autoComplete="current-password" disabled={loading} required />
            </div>
            <Button type="submit" disabled={loading} className="h-11 w-full">
              {loading ? (locale === "zh" ? "登入中..." : "Signing in...") : (locale === "zh" ? "登入" : "Sign in")}
              <ArrowRight aria-hidden="true" className="size-4" />
            </Button>
            <p aria-live="polite" className="text-sm text-muted-foreground">{status}</p>
          </form>
          <Button type="button" variant="link" onClick={() => navigate("register")} className="mt-3 h-auto p-0 text-muted-foreground">
            {locale === "zh" ? "還沒有帳號？立即建立" : "Need an account? Create one"}
          </Button>
        </CardContent>
      </Card>
    </section>
  );
}
