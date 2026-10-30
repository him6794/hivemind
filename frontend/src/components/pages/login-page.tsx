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
import { AccountApiError, loginUser } from "@/lib/hivemind-api";
import { validateLoginInput } from "@/lib/auth-policy.mjs";
import { accountErrorMessage } from "@/lib/account-policy.mjs";

function errorCode(error: unknown) {
  return error instanceof AccountApiError ? error.code : "request_failed";
}

export function LoginPage() {
  const navigate = useAppStore((state) => state.navigate);
  const setAuth = useAppStore((state) => state.setAuth);
  const { locale } = useI18n();
  const zh = locale === "zh";
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [status, setStatus] = useState("");
  const [loading, setLoading] = useState(false);

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (loading) return;
    const validation = validateLoginInput(username, password);
    if (!validation.ok) {
      setStatus(accountErrorMessage(validation.code, locale));
      return;
    }
    setLoading(true);
    setStatus(zh ? "登入中..." : "Signing in...");
    try {
      const data = await loginUser(validation.username, password) as { success?: boolean; token?: string };
      if (!data.success || !data.token) throw new AccountApiError("Login failed.", "login_failed");
      setUsername(validation.username);
      setAuth({ username: validation.username }, data.token);
      setStatus("");
      navigate("account");
    } catch (error) {
      setStatus(accountErrorMessage(errorCode(error), locale));
    } finally {
      setLoading(false);
    }
  }

  return (
    <section className="mx-auto flex min-h-screen w-full max-w-md flex-col justify-center gap-6 px-4 py-12">
      <div className="flex items-center justify-between gap-3">
        <Button variant="ghost" className="px-0 hover:bg-transparent" aria-label={zh ? "回到首頁" : "Hivemind home"} onClick={() => navigate("home")}><HiveLogo withText /></Button>
        <div className="flex items-center gap-1"><ThemeToggle /><LocaleToggle /></div>
      </div>
      <Card>
        <CardHeader>
          <h1 className="text-2xl font-semibold tracking-tight">{zh ? "登入 Hivemind" : "Sign in to Hivemind"}</h1>
        </CardHeader>
        <CardContent>
          <form className="space-y-5" onSubmit={handleSubmit} aria-busy={loading}>
            <div className="space-y-2">
              <Label htmlFor="login-username">{zh ? "使用者名稱" : "Username"}</Label>
              <Input id="login-username" className="h-11" value={username} onChange={(event) => setUsername(event.target.value)} autoComplete="username" disabled={loading} required />
            </div>
            <div className="space-y-2">
              <Label htmlFor="login-password">{zh ? "密碼" : "Password"}</Label>
              <Input id="login-password" className="h-11" type="password" value={password} onChange={(event) => setPassword(event.target.value)} autoComplete="current-password" disabled={loading} required />
            </div>
            <Button type="submit" disabled={loading} className="h-11 w-full">
              {loading ? (zh ? "登入中..." : "Signing in...") : (zh ? "登入" : "Sign in")}
              <ArrowRight aria-hidden="true" className="size-4" />
            </Button>
            <p aria-live="polite" className="text-sm text-muted-foreground">{status}</p>
          </form>
          <Button type="button" variant="link" onClick={() => navigate("register")} className="mt-3 h-auto p-0 text-muted-foreground">
            {zh ? "還沒有帳號？建立帳號" : "New here? Create an account"}
          </Button>
        </CardContent>
      </Card>
    </section>
  );
}
