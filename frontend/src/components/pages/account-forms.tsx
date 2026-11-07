"use client";

import { FormEvent, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { TurnstileWidget, type TurnstileHandle } from "@/components/site/turnstile-widget";
import { AccountApiError, bindEmail, changePassword, resendVerification, transferCpt } from "@/lib/hivemind-api";
import { validateChangePasswordInput, validateTransferInput } from "@/lib/auth-policy.mjs";
import { accountErrorMessage } from "@/lib/account-policy.mjs";
import { newIdempotencyKey, validateEmail } from "@/lib/account-input-policy.mjs";
import { useI18n } from "@/store/i18n-store";

function errorCode(error: unknown) {
  return error instanceof AccountApiError ? error.code : "request_failed";
}

export function EmailVerificationForm({ emailVerified }: { emailVerified: boolean }) {
  const { locale } = useI18n();
  const zh = locale === "zh";
  const [captchaToken, setCaptchaToken] = useState("");
  const [status, setStatus] = useState("");
  const [sending, setSending] = useState(false);
  const captcha = useRef<TurnstileHandle>(null);

  async function resend() {
    if (sending) return;
    if (!captchaToken) {
      setStatus(accountErrorMessage("captcha_required", locale));
      return;
    }
    setSending(true);
    setStatus(zh ? "寄送中..." : "Sending...");
    try {
      await resendVerification(captchaToken);
      setStatus(
        zh
          ? "新的驗證信已寄出。之前寄出的驗證連結都已失效，請只使用這一封。"
          : "A new verification email has been sent. Earlier links no longer work, so use this one only."
      );
    } catch (error) {
      setStatus(accountErrorMessage(errorCode(error), locale));
    } finally {
      // A captcha token works once, so the challenge is reset after every attempt.
      captcha.current?.reset();
      setSending(false);
    }
  }

  if (emailVerified) return null;

  return (
    <div className="space-y-4">
      <p className="text-sm text-muted-foreground">
        {zh
          ? "完成電子郵件驗證後才會發放歡迎 CPT，也才能轉帳。"
          : "Verify your email to receive the welcome CPT and to transfer CPT."}
      </p>
      <TurnstileWidget ref={captcha} onToken={setCaptchaToken} />
      <Button variant="outline" disabled={sending} onClick={resend}>
        {zh ? "重新寄送驗證郵件" : "Send verification email again"}
      </Button>
      <p aria-live="polite" className="text-sm text-muted-foreground">{status}</p>
    </div>
  );
}

export function BindEmailForm({ onBound }: { onBound: () => void }) {
  const { locale } = useI18n();
  const zh = locale === "zh";
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [captchaToken, setCaptchaToken] = useState("");
  const [status, setStatus] = useState("");
  const [sending, setSending] = useState(false);
  const captcha = useRef<TurnstileHandle>(null);

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (sending) return;
    const validated = validateEmail(email);
    if (!validated.ok || !password.trim() || !captchaToken) {
      const code = !validated.ok ? "invalid_email" : !password.trim() ? "password_required" : "captcha_required";
      setStatus(accountErrorMessage(code, locale));
      return;
    }
    setSending(true);
    try {
      await bindEmail(validated.email, password, captchaToken);
      setPassword("");
      onBound();
    } catch (error) {
      setStatus(accountErrorMessage(errorCode(error), locale));
    } finally {
      captcha.current?.reset();
      setSending(false);
    }
  }

  return (
    <form className="space-y-4" onSubmit={submit} aria-busy={sending}>
      <p className="text-sm text-muted-foreground">
        {zh ? "請綁定可收信的電子郵件，並在 24 小時內開啟驗證連結。帳號與既有 CPT 不會改變。" : "Add an email you can access and open its verification link within 24 hours. Your account and existing CPT are preserved."}
      </p>
      <div className="space-y-2">
        <Label htmlFor="bind-email">{zh ? "電子郵件" : "Email"}</Label>
        <Input id="bind-email" type="email" autoComplete="email" value={email} onChange={(event) => setEmail(event.target.value)} disabled={sending} required />
      </div>
      <div className="space-y-2">
        <Label htmlFor="bind-email-password">{zh ? "目前的密碼" : "Current password"}</Label>
        <Input id="bind-email-password" type="password" autoComplete="current-password" value={password} onChange={(event) => setPassword(event.target.value)} disabled={sending} required />
      </div>
      <TurnstileWidget ref={captcha} onToken={setCaptchaToken} />
      <Button type="submit" disabled={sending}>{zh ? "綁定信箱並寄出驗證信" : "Add email and send verification"}</Button>
      <p aria-live="polite" className="text-sm text-muted-foreground">{status}</p>
    </form>
  );
}

type TransferReview = { toUsername: string; amountCpt: number; idempotencyKey: string };

export function TransferForm({
  emailVerified,
  onTransferred,
}: {
  emailVerified: boolean;
  onTransferred: () => void;
}) {
  const { locale } = useI18n();
  const zh = locale === "zh";
  const [recipient, setRecipient] = useState("");
  const [amount, setAmount] = useState("");
  const [review, setReview] = useState<TransferReview | null>(null);
  const [status, setStatus] = useState("");
  const [sending, setSending] = useState(false);

  function handleReview(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const validation = validateTransferInput(recipient, amount);
    if (!validation.ok) {
      setReview(null);
      setStatus(accountErrorMessage(validation.code, locale));
      return;
    }
    // One key per transfer. A retry of this same transfer reuses the key, so a repeated
    // request cannot move CPT twice.
    setReview({
      toUsername: validation.toUsername,
      amountCpt: validation.amountCpt,
      idempotencyKey: newIdempotencyKey(),
    });
    setStatus("");
  }

  async function confirmTransfer() {
    if (!review || sending) return;
    setSending(true);
    setStatus(zh ? "轉帳處理中..." : "Sending...");
    try {
      await transferCpt(review.toUsername, review.amountCpt, review.idempotencyKey);
      setReview(null);
      setRecipient("");
      setAmount("");
      setStatus(zh ? "轉帳完成。" : "Transfer completed.");
      onTransferred();
    } catch (error) {
      // The review stays open with the same key, so the visitor can retry this transfer.
      setStatus(accountErrorMessage(errorCode(error), locale));
    } finally {
      setSending(false);
    }
  }

  return (
    <div className="space-y-4">
      {!emailVerified ? (
        <p className="text-sm text-muted-foreground">
          {zh ? "請先完成電子郵件驗證，才能轉出 CPT。" : "Verify your email before you transfer CPT."}
        </p>
      ) : null}
      <form className="space-y-4" onSubmit={handleReview} aria-busy={sending}>
        <div className="space-y-2">
          <Label htmlFor="transfer-recipient">{zh ? "收款人使用者名稱" : "Recipient username"}</Label>
          <Input
            id="transfer-recipient"
            className="h-11"
            value={recipient}
            onChange={(event) => {
              setRecipient(event.target.value);
              setReview(null);
            }}
            maxLength={64}
            autoComplete="off"
            disabled={!emailVerified || sending}
            required
          />
        </div>
        <div className="space-y-2">
          <Label htmlFor="transfer-amount">{zh ? "金額（CPT）" : "Amount (CPT)"}</Label>
          <Input
            id="transfer-amount"
            className="h-11"
            type="number"
            inputMode="numeric"
            min={1}
            step={1}
            value={amount}
            onChange={(event) => {
              setAmount(event.target.value);
              setReview(null);
            }}
            disabled={!emailVerified || sending}
            required
          />
        </div>
        {review ? (
          <div className="space-y-3 rounded-lg border p-4">
            <p className="text-sm">
              {zh
                ? `確認轉出 ${review.amountCpt} CPT 給 ${review.toUsername}？`
                : `Send ${review.amountCpt} CPT to ${review.toUsername}?`}
            </p>
            <div className="flex flex-wrap gap-2">
              <Button type="button" disabled={sending} onClick={confirmTransfer}>
                {zh ? "確認轉帳" : "Confirm transfer"}
              </Button>
              <Button type="button" variant="outline" disabled={sending} onClick={() => setReview(null)}>
                {zh ? "取消" : "Cancel"}
              </Button>
            </div>
          </div>
        ) : (
          <Button type="submit" className="h-11" disabled={!emailVerified || sending}>
            {zh ? "檢視轉帳內容" : "Review transfer"}
          </Button>
        )}
        <p aria-live="polite" className="text-sm text-muted-foreground">{status}</p>
      </form>
    </div>
  );
}

export function ChangePasswordForm() {
  const { locale } = useI18n();
  const zh = locale === "zh";
  const [current, setCurrent] = useState("");
  const [next, setNext] = useState("");
  const [confirm, setConfirm] = useState("");
  const [status, setStatus] = useState("");
  const [sending, setSending] = useState(false);

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (sending) return;
    const validation = validateChangePasswordInput(current, next, confirm);
    if (!validation.ok) {
      setStatus(accountErrorMessage(validation.code, locale));
      return;
    }
    setSending(true);
    setStatus(zh ? "變更密碼中..." : "Changing password...");
    try {
      await changePassword(current, next);
      setCurrent("");
      setNext("");
      setConfirm("");
      setStatus(zh ? "密碼已變更。" : "Password changed.");
    } catch (error) {
      setStatus(accountErrorMessage(errorCode(error), locale));
    } finally {
      setSending(false);
    }
  }

  return (
    <form className="space-y-4" onSubmit={handleSubmit} aria-busy={sending}>
      <div className="space-y-2">
        <Label htmlFor="password-current">{zh ? "目前的密碼" : "Current password"}</Label>
        <Input id="password-current" className="h-11" type="password" value={current} onChange={(event) => setCurrent(event.target.value)} autoComplete="current-password" disabled={sending} required />
      </div>
      <div className="space-y-2">
        <Label htmlFor="password-new">{zh ? "新密碼" : "New password"}</Label>
        <Input id="password-new" className="h-11" type="password" value={next} onChange={(event) => setNext(event.target.value)} autoComplete="new-password" disabled={sending} required />
      </div>
      <div className="space-y-2">
        <Label htmlFor="password-confirm">{zh ? "確認新密碼" : "Confirm new password"}</Label>
        <Input id="password-confirm" className="h-11" type="password" value={confirm} onChange={(event) => setConfirm(event.target.value)} autoComplete="new-password" disabled={sending} required />
      </div>
      <Button type="submit" className="h-11" disabled={sending}>
        {zh ? "變更密碼" : "Change password"}
      </Button>
      <p aria-live="polite" className="text-sm text-muted-foreground">{status}</p>
    </form>
  );
}
