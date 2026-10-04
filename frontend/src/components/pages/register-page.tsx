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
import { loginUser, registerUser } from "@/lib/hivemind-api";
import { validateRegistrationInput } from "@/lib/auth-policy.mjs";

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
    if (loading) return;
    const validation = validateRegistrationInput(username, password, confirm);
    if (!validation.ok) {
      const messages = {
        username_too_short: locale === "zh" ? "使用者名稱至少需要 3 個字元。" : "Username must be at least 3 characters.",
        password_too_short: locale === "zh" ? "密碼至少需要 8 個字元。" : "Password must be at least 8 characters.",
        password_mismatch: locale === "zh" ? "兩次輸入的密碼不一致。" : "Passwords do not match.",
      };
      setStatus(messages[validation.code]);
      return;
    }
    setLoading(true);
    setStatus(locale === "zh" ? "建立帳號中..." : "Creating account...");
    try {
      const registered = await registerUser(validation.username, password) as { success?: boolean; message?: string };
      if (!registered.success) throw new Error(registered.message || "Registration failed.");
      const login = await loginUser(validation.username, password) as { success?: boolean; token?: string; message?: string };
      if (!login.success || !login.token) throw new Error(login.message || "Login failed.");
      setUsername(validation.username);
      setAuth({ username: validation.username }, login.token);
      setStatus("");
      navigate("account");
    } catch (error) {
      setStatus(error instanceof Error ? error.message : "Registration failed.");
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
        <CardHeader><h1 className="text-2xl font-semibold tracking-tight">{locale === "zh" ? "建立 Hivemind 帳號，開始使用。" : "Create a Hivemind account and get started."}</h1></CardHeader>
        <CardContent>
          <form className="space-y-5" onSubmit={handleSubmit} aria-busy={loading}>
            <div className="space-y-2">
              <Label htmlFor="register-username">{locale === "zh" ? "使用者名稱" : "Username"}</Label>
              <Input id="register-username" className="h-11" value={username} onChange={(event) => setUsername(event.target.value)} minLength={3} autoComplete="username" disabled={loading} required />
            </div>
            <div className="space-y-2">
              <Label htmlFor="register-password">{locale === "zh" ? "密碼" : "Password"}</Label>
              <Input id="register-password" className="h-11" type="password" value={password} onChange={(event) => setPassword(event.target.value)} minLength={8} autoComplete="new-password" disabled={loading} required />
            </div>
            <div className="space-y-2">
              <Label htmlFor="register-confirm-password">{locale === "zh" ? "確認密碼" : "Confirm password"}</Label>
              <Input id="register-confirm-password" className="h-11" type="password" value={confirm} onChange={(event) => setConfirm(event.target.value)} minLength={8} autoComplete="new-password" disabled={loading} required />
            </div>
            <Button type="submit" disabled={loading} className="h-11 w-full">
              {loading ? (locale === "zh" ? "建立中..." : "Creating account...") : (locale === "zh" ? "建立帳號" : "Create account")}
              <ArrowRight aria-hidden="true" className="size-4" />
            </Button>
            <p aria-live="polite" className="text-sm text-muted-foreground">{status}</p>
          </form>
          <Button variant="link" className="mt-3 h-auto p-0 text-muted-foreground" onClick={() => navigate("login")}>{locale === "zh" ? "已有帳號？登入" : "Already have an account? Sign in"}</Button>
        </CardContent>
      </Card>
    </section>
  );
}
