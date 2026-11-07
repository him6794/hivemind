"use client";

import { FormEvent, useRef, useState } from "react";
import { ArrowRight, Eye, EyeOff } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Card, CardContent, CardHeader } from "@/components/ui/card";
import { HiveLogo } from "@/components/site/hive-logo";
import { ThemeToggle } from "@/components/site/theme-toggle";
import { LocaleToggle } from "@/components/site/locale-toggle";
import { TurnstileWidget, type TurnstileHandle } from "@/components/site/turnstile-widget";
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
  const [showPassword, setShowPassword] = useState(false);
  const [captchaToken, setCaptchaToken] = useState("");
  const [status, setStatus] = useState("");
  const [loading, setLoading] = useState(false);
  const captcha = useRef<TurnstileHandle>(null);

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (loading) return;
    const validation = validateLoginInput(username, password);
    if (!validation.ok) {
      setStatus(accountErrorMessage(validation.code, locale));
      return;
    }
    if (!captchaToken) {
      setStatus(accountErrorMessage("captcha_required", locale));
      return;
    }
    setLoading(true);
    setStatus(zh ? "登入中..." : "Signing in...");
    try {
      const data = (await loginUser(validation.username, password, captchaToken)) as { success?: boolean };
      if (!data.success) throw new AccountApiError("Login failed.", "login_failed");
      setAuth({ username: validation.username });
      setStatus("");
      // The session cookie is set. Open the page that asked for sign-in, or the account page.
      navigate(useAppStore.getState().returnRoute ?? "account");
    } catch (error) {
      setStatus(accountErrorMessage(errorCode(error), locale));
      // A captcha token works once, so the challenge is reset after every failed attempt.
      captcha.current?.reset();
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
        <CardHeader className="space-y-2">
          <h1 className="text-2xl font-semibold tracking-tight">{zh ? "登入 Hivemind" : "Sign in to Hivemind"}</h1>
          <p className="text-sm text-muted-foreground">
            {zh ? "登入後可以查看帳號、轉帳與管理密碼。" : "Sign in to view your account, transfer CPT, and manage your password."}
          </p>
        </CardHeader>
        <CardContent>
          <form className="space-y-5" onSubmit={handleSubmit} aria-busy={loading}>
            <div className="space-y-2">
              <Label htmlFor="login-username">{zh ? "使用者名稱" : "Username"}</Label>
              <Input id="login-username" className="h-11" value={username} onChange={(event) => setUsername(event.target.value)} autoComplete="username" disabled={loading} required />
            </div>
            <div className="space-y-2">
              <div className="flex items-center justify-between gap-3">
                <Label htmlFor="login-password">{zh ? "密碼" : "Password"}</Label>
                <Button type="button" variant="link" onClick={() => navigate("forgot-password")} className="h-auto p-0 text-sm text-muted-foreground">
                  {zh ? "忘記密碼？" : "Forgot password?"}
                </Button>
              </div>
              <div className="relative">
                <Input id="login-password" className="h-11 pr-11" type={showPassword ? "text" : "password"} value={password} onChange={(event) => setPassword(event.target.value)} autoComplete="current-password" disabled={loading} required />
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  className="absolute right-1 top-1/2 size-9 -translate-y-1/2"
                  aria-label={showPassword ? (zh ? "隱藏密碼" : "Hide password") : (zh ? "顯示密碼" : "Show password")}
                  onClick={() => setShowPassword((shown) => !shown)}
                >
                  {showPassword ? <EyeOff aria-hidden="true" className="size-4" /> : <Eye aria-hidden="true" className="size-4" />}
                </Button>
              </div>
            </div>
            <TurnstileWidget ref={captcha} onToken={setCaptchaToken} />
            <Button type="submit" disabled={loading} className="h-11 w-full">
              {loading ? (zh ? "登入中..." : "Signing in...") : (zh ? "登入" : "Sign in")}
              <ArrowRight aria-hidden="true" className="size-4" />
            </Button>
            <p aria-live="polite" className="text-sm text-muted-foreground">{status}</p>
          </form>
          <div className="mt-6 space-y-2 border-t pt-5 text-sm">
            <p className="text-muted-foreground">
              {zh ? "還沒有帳號？" : "New here?"}{" "}
              <Button type="button" variant="link" onClick={() => navigate("register")} className="h-auto p-0">
                {zh ? "建立帳號" : "Create an account"}
              </Button>
            </p>
            <p className="text-muted-foreground">
              {zh ? "忘記使用者名稱？" : "Forgot your username?"}{" "}
              <Button type="button" variant="link" onClick={() => navigate("forgot-username")} className="h-auto p-0">
                {zh ? "查詢使用者名稱" : "Look it up"}
              </Button>
            </p>
          </div>
        </CardContent>
      </Card>
    </section>
  );
}
