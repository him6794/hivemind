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
import { AccountApiError, resetPassword } from "@/lib/hivemind-api";
import { validateResetPasswordInput } from "@/lib/auth-policy.mjs";
import { accountErrorMessage } from "@/lib/account-policy.mjs";

function errorCode(error: unknown) {
  return error instanceof AccountApiError ? error.code : "request_failed";
}

export function ResetPasswordPage() {
  const navigate = useAppStore((state) => state.navigate);
  const token = useAppStore((state) => state.routeToken);
  const { locale } = useI18n();
  const zh = locale === "zh";
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [status, setStatus] = useState("");
  const [loading, setLoading] = useState(false);
  const [done, setDone] = useState(false);

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (loading || !token) return;
    const validation = validateResetPasswordInput(password, confirm);
    if (!validation.ok) {
      setStatus(accountErrorMessage(validation.code, locale));
      return;
    }
    setLoading(true);
    setStatus(zh ? "更新密碼中..." : "Updating password...");
    try {
      await resetPassword(token, password);
      setPassword("");
      setConfirm("");
      setStatus("");
      setDone(true);
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
          <h1 className="text-2xl font-semibold tracking-tight">{zh ? "設定新密碼" : "Set a new password"}</h1>
        </CardHeader>
        <CardContent className="space-y-5">
          {!token ? (
            <>
              <p role="alert" className="text-sm text-muted-foreground">{accountErrorMessage("reset_failed", locale)}</p>
              <Button className="h-11 w-full" onClick={() => navigate("forgot-password")}>
                {zh ? "重新要求重設連結" : "Request a new reset link"}
                <ArrowRight aria-hidden="true" className="size-4" />
              </Button>
            </>
          ) : done ? (
            <>
              <p role="status" className="text-sm text-muted-foreground">
                {zh ? "密碼已重設。請使用新密碼登入。" : "Your password has been reset. Sign in with the new password."}
              </p>
              <Button className="h-11 w-full" onClick={() => navigate("login")}>
                {zh ? "前往登入" : "Go to sign in"}
                <ArrowRight aria-hidden="true" className="size-4" />
              </Button>
            </>
          ) : (
            <form className="space-y-5" onSubmit={handleSubmit} aria-busy={loading}>
              <div className="space-y-2">
                <Label htmlFor="reset-password-new">{zh ? "新密碼" : "New password"}</Label>
                <Input id="reset-password-new" className="h-11" type="password" value={password} onChange={(event) => setPassword(event.target.value)} autoComplete="new-password" disabled={loading} required />
              </div>
              <div className="space-y-2">
                <Label htmlFor="reset-password-confirm">{zh ? "確認新密碼" : "Confirm new password"}</Label>
                <Input id="reset-password-confirm" className="h-11" type="password" value={confirm} onChange={(event) => setConfirm(event.target.value)} autoComplete="new-password" disabled={loading} required />
              </div>
              <Button type="submit" disabled={loading} className="h-11 w-full">
                {zh ? "設定新密碼" : "Set new password"}
                <ArrowRight aria-hidden="true" className="size-4" />
              </Button>
              <p aria-live="polite" className="text-sm text-muted-foreground">{status}</p>
            </form>
          )}
        </CardContent>
      </Card>
    </section>
  );
}
