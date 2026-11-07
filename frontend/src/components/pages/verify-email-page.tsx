"use client";

import { useEffect, useState } from "react";
import { ArrowRight } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader } from "@/components/ui/card";
import { HiveLogo } from "@/components/site/hive-logo";
import { ThemeToggle } from "@/components/site/theme-toggle";
import { LocaleToggle } from "@/components/site/locale-toggle";
import { useAppStore } from "@/store/app-store";
import { useI18n } from "@/store/i18n-store";
import { AccountApiError, verifyEmail, type WelcomeGrantStatus } from "@/lib/hivemind-api";
import { accountErrorMessage, welcomeGrantMessage } from "@/lib/account-policy.mjs";

type Outcome =
  | { state: "pending" }
  | { state: "verified"; grant: WelcomeGrantStatus }
  | { state: "failed"; code: string };

function errorCode(error: unknown) {
  return error instanceof AccountApiError ? error.code : "request_failed";
}

function headlineFor(outcome: Outcome, zh: boolean) {
  if (outcome.state === "pending") return zh ? "正在驗證電子郵件…" : "Verifying your email…";
  if (outcome.state === "verified") return zh ? "電子郵件已驗證" : "Email verified";
  return zh ? "無法完成驗證" : "Verification did not complete";
}

// Share pending requests and retain successful single-use results across remounts.
// Failed requests are removed so a temporary outage does not make a valid link unusable.
const verifications = new Map<string, Promise<WelcomeGrantStatus>>();

function verificationFor(token: string) {
  let request = verifications.get(token);
  if (!request) {
    request = verifyEmail(token).catch((error) => {
      verifications.delete(token);
      throw error;
    });
    verifications.set(token, request);
  }
  return request;
}

export function VerifyEmailPage() {
  const navigate = useAppStore((state) => state.navigate);
  const token = useAppStore((state) => state.routeToken);
  const { locale } = useI18n();
  const zh = locale === "zh";
  const [outcome, setOutcome] = useState<Outcome>(() =>
    token ? { state: "pending" } : { state: "failed", code: "verification_failed" }
  );

  useEffect(() => {
    if (!token) {
      setOutcome({ state: "failed", code: "verification_failed" });
      return;
    }
    // A response for an earlier token must not overwrite the result for the current one.
    let current = true;
    setOutcome({ state: "pending" });
    verificationFor(token)
      .then((grant) => {
        if (current) setOutcome({ state: "verified", grant });
      })
      .catch((error) => {
        if (current) setOutcome({ state: "failed", code: errorCode(error) });
      });
    return () => {
      current = false;
    };
  }, [token]);

  const headline = headlineFor(outcome, zh);
  // Empty for a grant status the page does not recognise. The verified note still shows.
  const grantText = outcome.state === "verified" ? welcomeGrantMessage(outcome.grant, true, locale) : "";

  return (
    <section className="mx-auto flex min-h-screen w-full max-w-md flex-col justify-center gap-6 px-4 py-12">
      <div className="flex items-center justify-between gap-3">
        <Button variant="ghost" className="px-0 hover:bg-transparent" aria-label={zh ? "回到首頁" : "Hivemind home"} onClick={() => navigate("home")}><HiveLogo withText /></Button>
        <div className="flex items-center gap-1"><ThemeToggle /><LocaleToggle /></div>
      </div>
      <Card>
        <CardHeader>
          <h1 className="text-2xl font-semibold tracking-tight">{headline}</h1>
        </CardHeader>
        <CardContent className="space-y-5">
          {outcome.state === "verified" ? (
            <p role="status" className="text-sm text-muted-foreground">
              {grantText || (zh ? "你的帳號已完成電子郵件驗證。" : "Your account email is verified.")}
            </p>
          ) : null}
          {outcome.state === "failed" ? (
            <p role="alert" className="text-sm text-muted-foreground">{accountErrorMessage(outcome.code, locale)}</p>
          ) : null}
          {outcome.state === "pending" ? null : (
            <Button className="h-11 w-full" onClick={() => navigate(outcome.state === "verified" ? "account" : "login")}>
              {outcome.state === "verified" ? (zh ? "前往帳號中心" : "Go to Account Center") : (zh ? "前往登入" : "Go to sign in")}
              <ArrowRight aria-hidden="true" className="size-4" />
            </Button>
          )}
        </CardContent>
      </Card>
    </section>
  );
}
