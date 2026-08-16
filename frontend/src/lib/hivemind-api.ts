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
    const message =
      (data as { message?: string; status_message?: string }).message ||
      (data as { message?: string; status_message?: string }).status_message ||
      `Request failed: ${res.status}`;
    throw new Error(message);
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
