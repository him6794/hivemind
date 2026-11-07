const apiBase = "";

type JsonValue = Record<string, unknown>;

async function readJson(res: Response) {
  const text = await res.text();
  if (!text) return {};
  try {
    return JSON.parse(text);
  } catch {
    return {};
  }
}

// Carries the stable error code the website backend returns, so pages can show
// a message in the visitor's language instead of the server's English text.
export class AccountApiError extends Error {
  readonly code: string;

  constructor(message: string, code: string) {
    super(message);
    this.name = "AccountApiError";
    this.code = code;
  }
}

// The session is an HttpOnly cookie that the browser sends with each request, so the page never
// holds the token. Only the signed-in account's name is kept in the page state.
async function request(method: string, path: string, body?: JsonValue) {
  const headers: Record<string, string> = {};
  if (body !== undefined) headers["Content-Type"] = "application/json";

  const res = await fetch(`${apiBase}${path}`, {
    method,
    headers,
    credentials: "same-origin",
    body: body === undefined ? undefined : JSON.stringify(body),
  });

  const data = await readJson(res);
  if (!res.ok) {
    const failure = data as { message?: string; status_message?: string; code?: unknown };
    const message = failure.message || failure.status_message || `Request failed: ${res.status}`;
    const code = typeof failure.code === "string" ? failure.code : "request_failed";
    throw new AccountApiError(message, code);
  }

  return data as JsonValue;
}

export type WelcomeGrantStatus = "granted" | "pool_empty" | "already_granted" | "unknown";

export interface TransferReceipt {
  fromBalance: number;
  toBalance: number;
  transferId: string;
}

function text(value: unknown) {
  return typeof value === "string" ? value : "";
}

function grantStatus(value: unknown): WelcomeGrantStatus {
  return value === "granted" || value === "pool_empty" || value === "already_granted" ? value : "unknown";
}

function numberOf(value: unknown) {
  const number = Number(value ?? 0);
  return Number.isFinite(number) ? number : 0;
}

export async function registerUser(input: {
  username: string;
  password: string;
  email: string;
  captchaToken: string;
}) {
  await request("POST", "/api/register", { ...input });
}

// Whether an account already uses the address, and whether that account has verified it. Sign-up asks while the
// visitor is still on the email step.
export async function checkEmail(email: string): Promise<{ available: boolean; verified: boolean }> {
  const data = await request("POST", "/api/check-email", { email });
  return { available: data.available === true, verified: data.verified === true };
}

export async function loginUser(username: string, password: string, captchaToken: string) {
  return request("POST", "/api/login", { username, password, captchaToken });
}

export async function logoutUser() {
  await request("POST", "/api/logout");
}

export async function getBalance() {
  return request("GET", "/api/balance");
}

// The raw account payload. The account page reads the balance with parseAccountBalance.
// It fails with AccountApiError when there is no signed-in session.
export async function getAccount() {
  return request("GET", "/api/account");
}

export async function verifyEmail(verificationToken: string): Promise<WelcomeGrantStatus> {
  const data = await request("POST", "/api/verify-email", { verificationToken });
  return grantStatus(data.welcomeGrantStatus);
}

export async function resendVerification(captchaToken: string) {
  await request("POST", "/api/resend-verification", { captchaToken });
}

export async function bindEmail(email: string, currentPassword: string, captchaToken: string) {
  await request("POST", "/api/bind-email", { email, currentPassword, captchaToken });
}

export async function changePassword(currentPassword: string, newPassword: string) {
  await request("POST", "/api/change-password", { currentPassword, newPassword });
}

export async function transferCpt(
  toUsername: string,
  amountCpt: number,
  idempotencyKey: string
): Promise<TransferReceipt> {
  const data = await request("POST", "/api/transfer", { toUsername, amountCpt, idempotencyKey });
  return {
    fromBalance: numberOf(data.fromBalance),
    toBalance: numberOf(data.toBalance),
    transferId: text(data.transferId),
  };
}

export async function requestPasswordReset(email: string, captchaToken: string) {
  await request("POST", "/api/forgot-password", { email, captchaToken });
}

export async function requestUsernameReminder(email: string, captchaToken: string) {
  await request("POST", "/api/forgot-username", { email, captchaToken });
}

export async function resetPassword(resetToken: string, newPassword: string) {
  await request("POST", "/api/reset-password", { resetToken, newPassword });
}
