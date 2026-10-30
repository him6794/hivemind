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

async function request(method: string, path: string, body?: JsonValue, token?: string) {
  const headers: Record<string, string> = {};
  if (token) headers.Authorization = `Bearer ${token}`;
  if (body !== undefined) headers["Content-Type"] = "application/json";

  const res = await fetch(`${apiBase}${path}`, {
    method,
    headers,
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

export async function registerUser(username: string, password: string) {
  return request("POST", "/api/register", { username, password });
}

export async function loginUser(username: string, password: string) {
  return request("POST", "/api/login", { username, password });
}

export async function getBalance(token: string) {
  return request("GET", "/api/balance", undefined, token);
}
