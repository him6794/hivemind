"use client";

import { FormEvent, useRef, useState } from "react";
import { ArrowRight } from "lucide-react";
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
import { AccountApiError, requestPasswordReset, requestUsernameReminder } from "@/lib/hivemind-api";
import { validateEmail } from "@/lib/account-input-policy.mjs";
import { accountErrorMessage } from "@/lib/account-policy.mjs";

type RecoveryKind = "password" | "username";

// The sent message is the same whether or not an account uses the address, so it does not
// reveal which addresses have accounts.
const COPY = {
  password: {
    en: {
      title: "Reset your password",
      body: "Enter the email address on your account. If an account uses it, we will send a link to reset the password.",
      action: "Send reset link",
      sent: "If an account uses that email address, a password reset link has been sent. Check your inbox and spam folder.",
    },
    zh: {
      title: "重設密碼",
      body: "請輸入帳號使用的電子郵件。若有帳號使用此信箱，我們會寄出重設密碼的連結。",
      action: "寄出重設連結",
      sent: "若有帳號使用此電子郵件，我們已寄出重設密碼的連結。請檢查收件匣與垃圾郵件匣。",
    },
  },
  username: {
    en: {
      title: "Find your username",
      body: "Enter the verified email address on your account. If a verified account uses it, we will send the username to that address.",
      action: "Send username",
      sent: "If a verified account uses that email address, the username has been sent to it. Check your inbox and spam folder.",
    },
    zh: {
      title: "找回使用者名稱",
      body: "請輸入已驗證的帳號電子郵件。若有已驗證的帳號使用此信箱，我們會把使用者名稱寄到該信箱。",
      action: "寄出使用者名稱",
      sent: "若有已驗證的帳號使用此電子郵件，我們已將使用者名稱寄到該信箱。請檢查收件匣與垃圾郵件匣。",
    },
  },
} as const;

function errorCode(error: unknown) {
  return error instanceof AccountApiError ? error.code : "request_failed";
}

function RecoveryPage({ kind }: { kind: RecoveryKind }) {
  const navigate = useAppStore((state) => state.navigate);
  const { locale } = useI18n();
  const zh = locale === "zh";
  const copy = COPY[kind][zh ? "zh" : "en"];
  const [email, setEmail] = useState("");
  const [captchaToken, setCaptchaToken] = useState("");
  const [status, setStatus] = useState("");
  const [loading, setLoading] = useState(false);
  const [sent, setSent] = useState(false);
  const captcha = useRef<TurnstileHandle>(null);

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (loading) return;
    const address = validateEmail(email);
    if (!address.ok) {
      setStatus(accountErrorMessage(address.code, locale));
      return;
    }
    if (!captchaToken) {
      setStatus(accountErrorMessage("captcha_required", locale));
      return;
    }
    setLoading(true);
    setStatus(zh ? "寄送中..." : "Sending...");
    try {
      if (kind === "password") {
        await requestPasswordReset(address.email, captchaToken);
      } else {
        await requestUsernameReminder(address.email, captchaToken);
      }
      setStatus("");
      setSent(true);
    } catch (error) {
      setStatus(accountErrorMessage(errorCode(error), locale));
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
        <CardHeader>
          <h1 className="text-2xl font-semibold tracking-tight">{copy.title}</h1>
        </CardHeader>
        <CardContent>
          {sent ? (
            <div className="space-y-5">
              <p role="status" className="text-sm text-muted-foreground">{copy.sent}</p>
              <Button className="h-11 w-full" onClick={() => navigate("login")}>
                {zh ? "返回登入" : "Back to sign in"}
                <ArrowRight aria-hidden="true" className="size-4" />
              </Button>
            </div>
          ) : (
            <form className="space-y-5" onSubmit={handleSubmit} aria-busy={loading}>
              <p className="text-sm text-muted-foreground">{copy.body}</p>
              <div className="space-y-2">
                <Label htmlFor={`${kind}-recovery-email`}>{zh ? "電子郵件" : "Email"}</Label>
                <Input id={`${kind}-recovery-email`} className="h-11" type="email" value={email} onChange={(event) => setEmail(event.target.value)} autoComplete="email" maxLength={254} disabled={loading} required />
              </div>
              <TurnstileWidget ref={captcha} onToken={setCaptchaToken} />
              <Button type="submit" disabled={loading} className="h-11 w-full">
                {copy.action}
                <ArrowRight aria-hidden="true" className="size-4" />
              </Button>
              <p aria-live="polite" className="text-sm text-muted-foreground">{status}</p>
            </form>
          )}
          {sent ? null : (
            <Button variant="link" className="mt-3 h-auto p-0 text-muted-foreground" onClick={() => navigate("login")}>{zh ? "返回登入" : "Back to sign in"}</Button>
          )}
        </CardContent>
      </Card>
    </section>
  );
}

export function ForgotPasswordPage() {
  return <RecoveryPage kind="password" />;
}

export function ForgotUsernamePage() {
  return <RecoveryPage kind="username" />;
}
