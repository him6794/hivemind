import assert from "node:assert/strict";
import test from "node:test";

async function loadPolicy() {
  try {
    return await import("./account-policy.mjs");
  } catch (error) {
    assert.fail(`account-policy.mjs must exist and load: ${error.message}`);
  }
}

test("account balance parsing accepts supported response fields", async () => {
  const { parseAccountBalance } = await loadPolicy();

  assert.equal(parseAccountBalance({ balance: 12.5 }), 12.5);
  assert.equal(parseAccountBalance({ cpt_balance: "42" }), 42);
});

test("account balance parsing rejects missing or malformed values", async () => {
  const { parseAccountBalance } = await loadPolicy();

  assert.throws(() => parseAccountBalance({}), /invalid balance/i);
  assert.throws(() => parseAccountBalance({ balance: "not-a-number" }), /invalid balance/i);
  assert.throws(() => parseAccountBalance({ balance: Number.POSITIVE_INFINITY }), /invalid balance/i);
});

test("a duplicate username or address says it has already been used", async () => {
  const { accountErrorMessage } = await loadPolicy();

  assert.match(accountErrorMessage("username_taken", "zh"), /已經有人用過了/);
  assert.match(accountErrorMessage("username_taken", "en"), /already been used/);
  assert.match(accountErrorMessage("email_in_use", "zh"), /已經有人用過了/);
  assert.match(accountErrorMessage("email_in_use", "en"), /already been used/);
});

test("a failed verification or reset link says a newer email may have replaced it", async () => {
  const { accountErrorMessage } = await loadPolicy();

  assert.match(accountErrorMessage("verification_failed", "zh"), /較新的驗證信取代/);
  assert.match(accountErrorMessage("verification_failed", "en"), /replaced by a newer verification email/);
  assert.match(accountErrorMessage("reset_failed", "zh"), /較新的重設信取代/);
  assert.match(accountErrorMessage("reset_failed", "en"), /replaced by a newer reset email/);
});

test("account error codes map to a message in the visitor's language", async () => {
  const { accountErrorMessage } = await loadPolicy();

  assert.equal(accountErrorMessage("rate_limited", "en"), "Too many attempts. Try again later.");
  assert.equal(accountErrorMessage("rate_limited", "zh"), "嘗試次數過多，請稍後再試。");
  assert.equal(accountErrorMessage("login_failed", "zh"), "使用者名稱或密碼不正確。");
  // Unknown codes and prototype keys fall back to the generic message.
  assert.equal(accountErrorMessage("unknown_code", "en"), accountErrorMessage("request_failed", "en"));
  assert.equal(accountErrorMessage("toString", "en"), accountErrorMessage("request_failed", "en"));
});

test("every code the website backend can return has a message in both languages", async () => {
  const { accountErrorMessage } = await loadPolicy();
  const codes = [
    "captcha_failed",
    "captcha_not_configured",
    "captcha_unavailable",
    "invalid_email",
    "password_too_short",
    "password_too_long",
    "password_edge_whitespace",
    "password_required",
    "verification_failed",
    "resend_failed",
    "reset_failed",
    "change_password_failed",
    "transfer_failed",
    "invalid_recipient",
    "invalid_amount",
    "authentication_required",
    "session_expired",
    "registration_failed",
    "username_taken",
    "email_in_use",
  ];
  for (const code of codes) {
    for (const locale of ["en", "zh"]) {
      const message = accountErrorMessage(code, locale);
      assert.notEqual(message, accountErrorMessage("request_failed", locale), `${locale} ${code} needs its own message`);
      assert.ok(message.length > 0, `${locale} ${code}`);
    }
  }
});

test("the registration error does not promise that only the username is wrong", async () => {
  const { accountErrorMessage } = await loadPolicy();

  assert.match(accountErrorMessage("registration_failed", "en"), /username and email/);
  assert.match(accountErrorMessage("registration_failed", "zh"), /電子郵件/);
});

test("the welcome grant line waits for email verification and then reports the grant status", async () => {
  const { welcomeGrantMessage } = await loadPolicy();

  assert.equal(
    welcomeGrantMessage("granted", false, "en"),
    "The welcome grant is paid after you verify your email."
  );
  assert.equal(welcomeGrantMessage("", false, "zh"), "歡迎點數會在完成電子郵件驗證後發放。");

  assert.equal(welcomeGrantMessage("granted", true, "en"), "The welcome grant has been credited.");
  assert.equal(welcomeGrantMessage("already_granted", true, "en"), "The welcome grant was already credited.");
  assert.match(welcomeGrantMessage("pool_empty", true, "en"), /on hold/);
  assert.equal(welcomeGrantMessage("unknown", true, "en"), "");
  assert.equal(welcomeGrantMessage("toString", true, "zh"), "");
});
