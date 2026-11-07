"use client";

import { FormEvent, useEffect, useRef, useState } from "react";
import { ArrowLeft, ArrowRight, Eye, EyeOff, KeyRound, Mail, Pencil, ShieldCheck, User, type LucideIcon } from "lucide-react";
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
import { AccountApiError, checkEmail, registerUser } from "@/lib/hivemind-api";
import { validateRegistrationInput } from "@/lib/auth-policy.mjs";
import { accountErrorMessage } from "@/lib/account-policy.mjs";
import {
  REGISTER_STEPS,
  firstInvalidRegisterStep,
  stepForRegisterError,
  validateRegisterStep,
} from "@/lib/register-flow-policy.mjs";

type Step = "email" | "username" | "password" | "review";
type Values = { email: string; username: string; password: string; confirm: string };

// Sign-up runs in order: email, username, password, then a review that sends the account.
const STEPS = REGISTER_STEPS as Step[];
const EMPTY_VALUES: Values = { email: "", username: "", password: "", confirm: "" };
// How long the address must stay unchanged before it is checked, so a check does not run on every keystroke.
const EMAIL_CHECK_DELAY_MS = 500;

function emailFormatOk(address: string) {
  return validateRegisterStep("email", { email: address, username: "", password: "", confirm: "" }).ok;
}
const STEP_ICONS: Record<Step, LucideIcon> = { email: Mail, username: User, password: KeyRound, review: ShieldCheck };

const COPY = {
  zh: {
    stepsLabel: "註冊步驟",
    stepWord: "步驟",
    stepNames: { email: "電子郵件", username: "使用者名稱", password: "密碼", review: "確認" },
    titles: { email: "建立 Hivemind 帳號", username: "選擇使用者名稱", password: "設定密碼", review: "確認資料" },
    hints: {
      email: "我們會寄驗證連結到這個信箱，之後也可以用它找回帳號。",
      username: "登入時使用的名稱，至少 3 個字元。",
      password: "設定一組只有你知道的密碼。",
      review: "送出後會寄出驗證信。點信中的連結完成驗證後，才會發放歡迎 CPT。",
    },
    labels: { email: "電子郵件", username: "使用者名稱", password: "密碼", confirm: "確認密碼" },
    passwordRule: "至少 8 個字元，最多 72 個位元組。",
    checking: "正在檢查這個信箱...",
    available: "這個信箱可以使用。",
    registeredUnverified: "這個信箱尚未完成驗證。若是你建立的帳號，可登入後重新寄信；若不是，請勿驗證。占用會在綁定後 24 小時到期，之後可重新註冊。",
    hiddenPassword: "••••••••",
    change: "變更",
    back: "上一步",
    next: "下一步",
    create: "建立帳號",
    creating: "建立帳號中...",
    showPassword: "顯示密碼",
    hidePassword: "隱藏密碼",
    haveAccount: "已經有帳號了？",
    signIn: "登入",
    verifyTitle: "請檢查電子郵件，完成帳號驗證",
    sentTo: (email: string) => `我們已寄出驗證信到 ${email}。`,
    verifySteps: [
      "打開信箱，找到 Hivemind 寄來的驗證信。",
      "點信中的連結完成驗證。驗證後會發放歡迎 CPT。",
      "回到這裡登入。",
    ],
    noEmail: "沒有收到？請檢查垃圾郵件。之後也可以登入後到帳號中心重新寄送，寄出的新連結會讓舊連結失效。",
    goSignIn: "前往登入",
  },
  en: {
    stepsLabel: "Sign-up steps",
    stepWord: "Step",
    stepNames: { email: "Email", username: "Username", password: "Password", review: "Review" },
    titles: { email: "Create your account", username: "Choose a username", password: "Set a password", review: "Check your details" },
    hints: {
      email: "We will email a verification link to this address, and you can use it to recover your account later.",
      username: "The name you sign in with. At least 3 characters.",
      password: "Choose a password only you know.",
      review: "We will send a verification email. The welcome CPT is paid after you open the link in it.",
    },
    labels: { email: "Email", username: "Username", password: "Password", confirm: "Confirm password" },
    passwordRule: "At least 8 characters, up to 72 bytes.",
    checking: "Checking this address...",
    available: "This address is available.",
    registeredUnverified: "This address is not verified yet. If you created the account, sign in to resend the email. Otherwise, do not verify it. The reservation expires 24 hours after binding; you can register again after that.",
    hiddenPassword: "••••••••",
    change: "Change",
    back: "Back",
    next: "Next",
    create: "Create account",
    creating: "Creating account...",
    showPassword: "Show password",
    hidePassword: "Hide password",
    haveAccount: "Already have an account?",
    signIn: "Sign in",
    verifyTitle: "Check your email to verify the account",
    sentTo: (email: string) => `We sent a verification email to ${email}.`,
    verifySteps: [
      "Open your inbox and find the Hivemind verification email.",
      "Open the link in the email. The welcome CPT is paid after verification.",
      "Then sign in here.",
    ],
    noEmail: "No email? Check your spam folder. You can also sign in and send it again from the account page. A new link replaces the older ones.",
    goSignIn: "Go to sign in",
  },
};

function errorCode(error: unknown) {
  return error instanceof AccountApiError ? error.code : "request_failed";
}

function StepIndicator({ current, zh }: { current: number; zh: boolean }) {
  const copy = zh ? COPY.zh : COPY.en;
  const step = STEPS[current];
  return (
    <nav aria-label={copy.stepsLabel}>
      <ol className="flex items-center gap-2">
        {STEPS.map((item, index) => (
          <li
            key={item}
            aria-current={index === current ? "step" : undefined}
            className={`h-1.5 flex-1 rounded-full transition-colors ${index <= current ? "bg-foreground" : "bg-muted"}`}
          />
        ))}
      </ol>
      <p className="mt-3 text-xs text-muted-foreground">
        {zh
          ? `${copy.stepWord} ${current + 1} / ${STEPS.length} · ${copy.stepNames[step]}`
          : `${copy.stepWord} ${current + 1} of ${STEPS.length} · ${copy.stepNames[step]}`}
      </p>
    </nav>
  );
}

function SummaryRow({ label, value, onChange, changeLabel }: { label: string; value: string; onChange: () => void; changeLabel: string }) {
  return (
    <div className="flex items-center justify-between gap-3 p-4">
      <div className="min-w-0">
        <dt className="text-xs text-muted-foreground">{label}</dt>
        <dd className="truncate text-sm font-medium">{value}</dd>
      </div>
      <Button type="button" variant="ghost" size="sm" className="shrink-0" onClick={onChange} aria-label={`${changeLabel} ${label}`}>
        <Pencil aria-hidden="true" className="size-3.5" />
        {changeLabel}
      </Button>
    </div>
  );
}

export function RegisterPage() {
  const navigate = useAppStore((state) => state.navigate);
  const { locale } = useI18n();
  const zh = locale === "zh";
  const copy = zh ? COPY.zh : COPY.en;
  const [stepIndex, setStepIndex] = useState(0);
  const [values, setValues] = useState<Values>(EMPTY_VALUES);
  const [showPassword, setShowPassword] = useState(false);
  const [captchaToken, setCaptchaToken] = useState("");
  const [status, setStatus] = useState("");
  const [loading, setLoading] = useState(false);
  // The address the verification email went to. Set once the account exists.
  const [sentTo, setSentTo] = useState("");
  // The server's answer for each address the visitor has typed on the first step.
  const [emailResults, setEmailResults] = useState<Record<string, { available: boolean; verified: boolean }>>({});
  // The address whose check is running now, if any.
  const [checkingEmail, setCheckingEmail] = useState("");
  const [emailTouched, setEmailTouched] = useState(false);
  const captcha = useRef<TurnstileHandle>(null);
  const firstField = useRef<HTMLInputElement>(null);
  const step = STEPS[stepIndex];
  const Icon = STEP_ICONS[step];

  // Each step opens with its first field focused, so the visitor can type straight away.
  useEffect(() => {
    if (!sentTo) firstField.current?.focus();
  }, [stepIndex, sentTo]);

  // As soon as the address is well formed, ask whether an account already uses it. The answer shows on this step,
  // so a taken address is found before the visitor moves on.
  useEffect(() => {
    const typed = values.email.trim();
    if (sentTo || step !== "email" || !emailFormatOk(typed) || emailResults[typed] !== undefined) return;
    const timer = window.setTimeout(() => {
      setCheckingEmail(typed);
      checkEmail(typed)
        .then((answer) => setEmailResults((current) => ({ ...current, [typed]: answer })))
        .catch(() => {
          // The check only guides the visitor, and the final step checks again. A failed check does not block sign-up.
        })
        .finally(() => setCheckingEmail((current) => (current === typed ? "" : current)));
    }, EMAIL_CHECK_DELAY_MS);
    return () => window.clearTimeout(timer);
  }, [sentTo, step, values.email, emailResults]);

  const address = values.email.trim();
  const addressFormatOk = emailFormatOk(address);
  const answer = emailResults[address];
  const addressTaken = answer !== undefined && !answer.available;
  const addressAvailable = answer !== undefined && answer.available && addressFormatOk;
  const formatProblem = emailTouched && values.email !== "" && !addressFormatOk;
  const emailNoticeIsProblem = formatProblem || addressTaken;
  // A verified account is the strong case. An account that never verified the address is told how to finish it.
  const takenNotice = answer?.verified ? accountErrorMessage("email_in_use", locale) : copy.registeredUnverified;
  const emailNotice = formatProblem
    ? accountErrorMessage("invalid_email", locale)
    : addressTaken
      ? takenNotice
      : checkingEmail === address && addressFormatOk
        ? copy.checking
        : addressAvailable
          ? copy.available
          : "";

  function update(field: keyof Values, value: string) {
    setValues((current) => ({ ...current, [field]: value }));
  }

  function goTo(index: number) {
    // The verification widget is only on the review step. Its token is for that page, so it is dropped when the visitor leaves.
    if (step === "review") setCaptchaToken("");
    setStatus("");
    setStepIndex(index);
  }

  // Takes the visitor to the step that holds the problem and shows the message there.
  function showProblem(code: string) {
    setStepIndex(STEPS.indexOf(stepForRegisterError(code) as Step));
    setStatus(accountErrorMessage(code, locale));
  }

  function next() {
    const check = validateRegisterStep(step, values);
    if (!check.ok) {
      setStatus(accountErrorMessage(check.code, locale));
      return;
    }
    goTo(stepIndex + 1);
  }

  async function submit() {
    if (loading) return;
    const validation = validateRegistrationInput(values.username, values.password, values.confirm, values.email);
    if (!validation.ok) {
      const invalid = firstInvalidRegisterStep(values);
      showProblem(invalid ? invalid.code : validation.code);
      return;
    }
    if (!captchaToken) {
      setStatus(accountErrorMessage("captcha_required", locale));
      return;
    }
    setLoading(true);
    setStatus(copy.creating);
    try {
      await registerUser({
        username: validation.username,
        password: values.password,
        email: validation.email,
        captchaToken,
      });
      // The account is not signed in here. The welcome grant is paid only after the email is verified.
      setValues(EMPTY_VALUES);
      setStatus("");
      setSentTo(validation.email);
    } catch (error) {
      setCaptchaToken("");
      captcha.current?.reset();
      showProblem(errorCode(error));
    } finally {
      setLoading(false);
    }
  }

  // The email step waits for the server's answer before it moves on, so a taken address does not pass.
  async function nextFromEmail() {
    const check = validateRegisterStep("email", values);
    if (!check.ok) {
      // The field shows the problem, so the form's status line stays empty and the message is not repeated.
      setEmailTouched(true);
      firstField.current?.focus();
      return;
    }
    let known = emailResults[address];
    if (known === undefined) {
      setCheckingEmail(address);
      try {
        const answer = await checkEmail(address);
        setEmailResults((current) => ({ ...current, [address]: answer }));
        known = answer;
      } catch {
        // The check only guides the visitor, and the final step checks again. A failed check does not block sign-up.
        known = { available: true, verified: false };
      } finally {
        setCheckingEmail((current) => (current === address ? "" : current));
      }
    }
    if (!known.available) {
      // The field already says the address is in use. Focus goes back to it, so the visitor can correct it.
      firstField.current?.focus();
      return;
    }
    goTo(stepIndex + 1);
  }

  function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (step === "review") {
      void submit();
    } else if (step === "email") {
      void nextFromEmail();
    } else {
      next();
    }
  }

  return (
    <section className="mx-auto flex min-h-screen w-full max-w-md flex-col justify-center gap-6 px-4 py-12">
      <div className="flex items-center justify-between gap-3">
        <Button variant="ghost" className="px-0 hover:bg-transparent" aria-label={zh ? "回到首頁" : "Hivemind home"} onClick={() => navigate("home")}><HiveLogo withText /></Button>
        <div className="flex items-center gap-1"><ThemeToggle /><LocaleToggle /></div>
      </div>
      <Card>
        {sentTo ? (
          <>
            <CardHeader className="space-y-2">
              <span className="flex size-10 items-center justify-center rounded-full bg-muted">
                <Mail aria-hidden="true" className="size-5" />
              </span>
              <h1 className="text-2xl font-semibold tracking-tight">{copy.verifyTitle}</h1>
            </CardHeader>
            <CardContent className="space-y-5">
              <p className="text-sm">{copy.sentTo(sentTo)}</p>
              <ol className="list-decimal space-y-1 pl-5 text-sm text-muted-foreground">
                {copy.verifySteps.map((line) => (
                  <li key={line}>{line}</li>
                ))}
              </ol>
              <p className="text-sm text-muted-foreground">{copy.noEmail}</p>
              <Button className="h-11 w-full" onClick={() => navigate("login")}>
                {copy.goSignIn}
                <ArrowRight aria-hidden="true" className="size-4" />
              </Button>
            </CardContent>
          </>
        ) : (
          <>
            <CardHeader className="space-y-5">
              <StepIndicator current={stepIndex} zh={zh} />
              <div className="flex items-start gap-3">
                <span className="flex size-10 shrink-0 items-center justify-center rounded-full bg-muted">
                  <Icon aria-hidden="true" className="size-5" />
                </span>
                <div className="space-y-1.5">
                  <h1 className="text-2xl font-semibold tracking-tight">{copy.titles[step]}</h1>
                  <p className="text-sm text-muted-foreground">{copy.hints[step]}</p>
                </div>
              </div>
            </CardHeader>
            <CardContent>
              <form className="space-y-5" onSubmit={handleSubmit} noValidate aria-busy={loading}>
                {step === "email" ? (
                  <div className="space-y-2">
                    <Label htmlFor="register-email">{copy.labels.email}</Label>
                    <Input
                      id="register-email"
                      ref={firstField}
                      className="h-11"
                      type="email"
                      value={values.email}
                      onChange={(event) => update("email", event.target.value)}
                      onBlur={() => setEmailTouched(true)}
                      aria-invalid={emailNoticeIsProblem}
                      aria-describedby="register-email-status"
                      autoComplete="email"
                      maxLength={254}
                      disabled={loading}
                      required
                    />
                    <p id="register-email-status" aria-live="polite" className={`text-sm ${emailNoticeIsProblem ? "text-destructive" : "text-muted-foreground"}`}>
                      {emailNotice}
                    </p>
                  </div>
                ) : null}

                {step === "username" ? (
                  <div className="space-y-2">
                    <Label htmlFor="register-username">{copy.labels.username}</Label>
                    <Input
                      id="register-username"
                      ref={firstField}
                      className="h-11"
                      value={values.username}
                      onChange={(event) => update("username", event.target.value)}
                      autoComplete="username"
                      disabled={loading}
                      required
                    />
                  </div>
                ) : null}

                {step === "password" ? (
                  <>
                    <div className="space-y-2">
                      <Label htmlFor="register-password">{copy.labels.password}</Label>
                      <div className="relative">
                        <Input
                          id="register-password"
                          ref={firstField}
                          className="h-11 pr-11"
                          type={showPassword ? "text" : "password"}
                          value={values.password}
                          onChange={(event) => update("password", event.target.value)}
                          autoComplete="new-password"
                          aria-describedby="register-password-hint"
                          disabled={loading}
                          required
                        />
                        <Button
                          type="button"
                          variant="ghost"
                          size="icon"
                          className="absolute right-1 top-1/2 size-9 -translate-y-1/2"
                          aria-label={showPassword ? copy.hidePassword : copy.showPassword}
                          onClick={() => setShowPassword((shown) => !shown)}
                        >
                          {showPassword ? <EyeOff aria-hidden="true" className="size-4" /> : <Eye aria-hidden="true" className="size-4" />}
                        </Button>
                      </div>
                      <p id="register-password-hint" className="text-xs text-muted-foreground">{copy.passwordRule}</p>
                    </div>
                    <div className="space-y-2">
                      <Label htmlFor="register-confirm-password">{copy.labels.confirm}</Label>
                      <Input
                        id="register-confirm-password"
                        className="h-11"
                        type={showPassword ? "text" : "password"}
                        value={values.confirm}
                        onChange={(event) => update("confirm", event.target.value)}
                        autoComplete="new-password"
                        disabled={loading}
                        required
                      />
                    </div>
                  </>
                ) : null}

                {step === "review" ? (
                  <>
                    <dl className="divide-y rounded-lg border">
                      <SummaryRow label={copy.labels.email} value={values.email} changeLabel={copy.change} onChange={() => goTo(STEPS.indexOf("email"))} />
                      <SummaryRow label={copy.labels.username} value={values.username} changeLabel={copy.change} onChange={() => goTo(STEPS.indexOf("username"))} />
                      <SummaryRow label={copy.labels.password} value={copy.hiddenPassword} changeLabel={copy.change} onChange={() => goTo(STEPS.indexOf("password"))} />
                    </dl>
                    <TurnstileWidget ref={captcha} onToken={setCaptchaToken} />
                  </>
                ) : null}

                <div className="flex gap-3">
                  {stepIndex > 0 ? (
                    <Button type="button" variant="outline" className="h-11" disabled={loading} onClick={() => goTo(stepIndex - 1)}>
                      <ArrowLeft aria-hidden="true" className="size-4" />
                      {copy.back}
                    </Button>
                  ) : null}
                  <Button type="submit" className="h-11 flex-1" disabled={loading}>
                    {step === "review" ? (loading ? copy.creating : copy.create) : copy.next}
                    {step === "review" ? null : <ArrowRight aria-hidden="true" className="size-4" />}
                  </Button>
                </div>
                <p aria-live="polite" className="text-sm text-muted-foreground">{status}</p>
              </form>
              {step === "email" ? (
                <div className="mt-6 border-t pt-5 text-sm">
                  <p className="text-muted-foreground">
                    {copy.haveAccount}{" "}
                    <Button variant="link" className="h-auto p-0" onClick={() => navigate("login")}>
                      {copy.signIn}
                    </Button>
                  </p>
                </div>
              ) : null}
            </CardContent>
          </>
        )}
      </Card>
    </section>
  );
}
