"use client";

import { useEffect, useState } from "react";
import { ArrowRight, LogOut, RefreshCw, Wallet } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { AccountApiError, getAccount } from "@/lib/hivemind-api";
import { accountErrorMessage, parseAccountBalance, welcomeGrantMessage } from "@/lib/account-policy.mjs";
import { useI18n } from "@/store/i18n-store";
import { useAppStore } from "@/store/app-store";
import { BindEmailForm, ChangePasswordForm, EmailVerificationForm, TransferForm } from "@/components/pages/account-forms";

interface AccountSummary {
  username: string;
  email: string;
  emailVerified: boolean;
  balance: number;
  welcomeGrantStatus: string;
}

function errorCode(error: unknown) {
  return error instanceof AccountApiError ? error.code : "request_failed";
}

// Codes that mean the server no longer accepts the session.
const SESSION_ENDED_CODES = ["session_expired", "authentication_required"];

// Reads the account payload. A malformed balance throws, which the page reports as a failure.
function summarize(data: Record<string, unknown>): AccountSummary {
  return {
    username: typeof data.username === "string" ? data.username : "",
    email: typeof data.email === "string" ? data.email : "",
    emailVerified: data.emailVerified === true,
    balance: parseAccountBalance(data),
    welcomeGrantStatus: typeof data.welcomeGrantStatus === "string" ? data.welcomeGrantStatus : "unknown",
  };
}

export function AccountPage() {
  const { locale } = useI18n();
  const zh = locale === "zh";
  const user = useAppStore((state) => state.user);
  const navigate = useAppStore((state) => state.navigate);
  const logout = useAppStore((state) => state.logout);
  const signedIn = user !== null;
  const [account, setAccount] = useState<AccountSummary | null>(null);
  const [status, setStatus] = useState("");
  const [loading, setLoading] = useState(false);
  const [refresh, setRefresh] = useState(0);

  useEffect(() => {
    if (!signedIn) {
      setAccount(null);
      setLoading(false);
      return;
    }
    let cancelled = false;
    setLoading(true);
    setStatus("");
    getAccount()
      .then((data) => {
        if (!cancelled) setAccount(summarize(data));
      })
      .catch((error) => {
        if (cancelled) return;
        if (SESSION_ENDED_CODES.includes(errorCode(error))) {
          // The session has ended. Clear it, then send the visitor to sign in and back here.
          void logout().then(() => navigate("account"));
          return;
        }
        setStatus(accountErrorMessage(errorCode(error), locale));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [locale, signedIn, refresh, logout, navigate]);

  const grantNote = account ? welcomeGrantMessage(account.welcomeGrantStatus, account.emailVerified, locale) : "";
  const reload = () => setRefresh((value) => value + 1);

  return (
    <section className="mx-auto w-full max-w-5xl px-4 pb-16 pt-28 sm:px-6">
      <div className="mb-6 flex flex-wrap items-center justify-between gap-4">
        <div className="space-y-1">
          <h1 className="text-3xl font-semibold tracking-tight">{zh ? "帳號中心" : "Account Center"}</h1>
          {user ? (
            <p className="text-sm text-muted-foreground">{zh ? `已登入為 ${user.username}` : `Signed in as ${user.username}`}</p>
          ) : null}
        </div>
        {user ? (
          <Button variant="outline" onClick={() => void logout()}>
            <LogOut aria-hidden="true" className="size-4" />
            {zh ? "登出" : "Sign out"}
          </Button>
        ) : null}
      </div>

      {account && !account.emailVerified ? (
        <div role="status" className="mb-6 rounded-lg border border-amber-500/40 bg-amber-500/10 p-4 text-sm">
          {zh
            ? "你的電子郵件還沒有驗證。驗證後才會發放歡迎 CPT，也才能轉帳。"
            : "Your email is not verified yet. The welcome CPT is paid, and transfers open, after you verify it."}
        </div>
      ) : null}

      <div className="grid gap-4 md:grid-cols-2">
        <Card>
          <CardHeader className="flex flex-row items-start justify-between gap-4">
            <div className="space-y-2">
              <CardTitle className="text-base">{zh ? "CPT 餘額" : "CPT balance"}</CardTitle>
              <CardDescription>{account?.username || user?.username || (zh ? "尚未登入" : "Not signed in")}</CardDescription>
            </div>
            <Wallet aria-hidden="true" className="size-5 text-muted-foreground" />
          </CardHeader>
          <CardContent aria-busy={loading}>
            {loading && account === null ? <Skeleton className="h-10 w-32" aria-label={zh ? "讀取餘額" : "Loading balance"} /> : <p className="text-4xl font-semibold tracking-tight">{account === null ? "—" : account.balance.toFixed(2)}</p>}
            {grantNote ? <p className="mt-3 text-sm text-muted-foreground">{grantNote}</p> : null}
            <p role="status" aria-live="polite" className="mt-4 break-words text-sm text-muted-foreground">{status}</p>
            {signedIn && <Button className="mt-4" variant="outline" disabled={loading} onClick={reload}><RefreshCw aria-hidden="true" className={loading ? "animate-spin" : ""} />{zh ? "重新整理" : "Refresh"}</Button>}
          </CardContent>
        </Card>
        <Card>
          <CardHeader>
            <CardTitle className="text-base">{zh ? "電子郵件與驗證" : "Email and verification"}</CardTitle>
            <CardDescription>{account ? (account.email || (zh ? "尚未設定電子郵件" : "No email on file")) : (zh ? "登入後顯示" : "Shown after sign-in")}</CardDescription>
          </CardHeader>
          <CardContent className="space-y-4">
            {account ? (
              <>
                <p className="text-sm font-medium">
                  {account.emailVerified ? (zh ? "已驗證" : "Verified") : (zh ? "尚未驗證" : "Not verified")}
                </p>
                {account.email ? <EmailVerificationForm emailVerified={account.emailVerified} /> : <BindEmailForm onBound={reload} />}
              </>
            ) : null}
          </CardContent>
        </Card>
        <Card>
          <CardHeader>
            <CardTitle className="text-base">{zh ? "轉帳 CPT" : "Transfer CPT"}</CardTitle>
            <CardDescription>{zh ? "轉給另一位使用者。" : "Send CPT to another user."}</CardDescription>
          </CardHeader>
          <CardContent>
            {account ? (
              <TransferForm emailVerified={account.emailVerified} onTransferred={reload} />
            ) : (
              <p className="text-sm text-muted-foreground">{zh ? "登入後即可轉帳。" : "Sign in to transfer CPT."}</p>
            )}
          </CardContent>
        </Card>
        <Card>
          <CardHeader>
            <CardTitle className="text-base">{zh ? "變更密碼" : "Change password"}</CardTitle>
            <CardDescription>{zh ? "需要輸入目前的密碼。" : "Your current password is required."}</CardDescription>
          </CardHeader>
          <CardContent>
            {signedIn ? <ChangePasswordForm /> : <p className="text-sm text-muted-foreground">{zh ? "登入後即可變更密碼。" : "Sign in to change your password."}</p>}
          </CardContent>
        </Card>
        <Card>
          <CardHeader>
            <CardTitle className="text-base">{zh ? "Master 與 Worker" : "Master and Worker"}</CardTitle>
            <CardDescription>{zh ? "用 Master 送出工作，用 Worker 分享你的電腦。" : "Use Master to send tasks and Worker to share your computer."}</CardDescription>
          </CardHeader>
          <CardContent><Button onClick={() => navigate("download")}>{zh ? "下載與安裝" : "Download and install"}<ArrowRight aria-hidden="true" /></Button></CardContent>
        </Card>
      </div>
    </section>
  );
}
