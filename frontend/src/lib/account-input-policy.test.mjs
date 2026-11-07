import assert from "node:assert/strict";
import test from "node:test";
import {
  isPositiveSafeInteger,
  isValidIdempotencyKey,
  newIdempotencyKey,
  parseAmountCpt,
  passwordBytes,
  passwordProblem,
  validateEmail,
  validateRecipient,
} from "./account-input-policy.mjs";

test("email validation trims the address and accepts local@domain.tld", () => {
  assert.deepEqual(validateEmail("  alice@example.com  "), { ok: true, email: "alice@example.com" });
  assert.deepEqual(validateEmail("first.last+tag@mail.example.co.uk"), {
    ok: true,
    email: "first.last+tag@mail.example.co.uk",
  });
});

test("email validation rejects values without a domain, a top-level label, or valid labels", () => {
  for (const value of [
    "",
    "   ",
    "alice",
    "alice@",
    "@example.com",
    "alice@localhost",
    "alice@@example.com",
    "alice@example.",
    "alice@-example.com",
    "alice@example-.com",
    "a b@example.com",
    ".alice@example.com",
    "alice.@example.com",
    "alice..bob@example.com",
    null,
    undefined,
    42,
  ]) {
    assert.deepEqual(validateEmail(value), { ok: false, code: "invalid_email" }, JSON.stringify(value));
  }
});

test("email validation enforces the 254 character and 64 character local part limits", () => {
  const local = "a".repeat(64);
  assert.equal(validateEmail(`${local}@example.com`).ok, true);
  assert.equal(validateEmail(`${"a".repeat(65)}@example.com`).ok, false);

  // 63 + 1 + 63 + 1 + 63 + 1 + 59 + 4 = 255 characters, one over the limit.
  const label63 = "a".repeat(63);
  const tooLong = `${label63}@${label63}.${label63}.${"b".repeat(59)}.com`;
  assert.equal(tooLong.length, 255);
  assert.equal(validateEmail(tooLong).ok, false);
  // Exactly 254 characters is still within the limit and has a valid shape.
  assert.equal(tooLong.slice(0, 254).length, 254);
  assert.equal(validateEmail(tooLong.slice(0, 254)).ok, true);
});

test("password policy counts UTF-8 bytes between 8 and 72", () => {
  assert.equal(passwordProblem("1234567"), "password_too_short");
  assert.equal(passwordProblem("12345678"), null);
  assert.equal(passwordProblem("a".repeat(72)), null);
  assert.equal(passwordProblem("a".repeat(73)), "password_too_long");

  // Each of these characters is two bytes, so 36 of them fill 72 bytes exactly.
  assert.equal(passwordBytes("é"), 2);
  assert.equal(passwordProblem("é".repeat(36)), null);
  assert.equal(passwordProblem("é".repeat(37)), "password_too_long");

  // Four CJK characters are 12 bytes. They pass the byte rule even though they are fewer than 8 characters.
  assert.equal(passwordProblem("密碼測試"), null);
});

test("password policy refuses leading or trailing whitespace and non-string input", () => {
  assert.equal(passwordProblem(" long-enough"), "password_edge_whitespace");
  assert.equal(passwordProblem("long-enough "), "password_edge_whitespace");
  assert.equal(passwordProblem("long enough"), null);
  assert.equal(passwordProblem(undefined), "password_too_short");
  assert.equal(passwordProblem(12345678), "password_too_short");
});

test("CPT amounts must be positive safe integers", () => {
  assert.equal(isPositiveSafeInteger(1), true);
  assert.equal(isPositiveSafeInteger(Number.MAX_SAFE_INTEGER), true);
  for (const value of [0, -1, 1.5, "1", NaN, Infinity, Number.MAX_SAFE_INTEGER + 1, null]) {
    assert.equal(isPositiveSafeInteger(value), false, String(value));
  }
});

test("amount text is parsed only when it is plain digits", () => {
  assert.equal(parseAmountCpt("10"), 10);
  assert.equal(parseAmountCpt(" 42 "), 42);
  assert.equal(parseAmountCpt("007"), 7);
  for (const text of ["", "0", "-1", "1.5", "1e3", "12 CPT", "9007199254740992", "abc", null]) {
    assert.equal(parseAmountCpt(text), null, String(text));
  }
});

test("recipient usernames are trimmed and bounded", () => {
  assert.deepEqual(validateRecipient("  alice  "), { ok: true, username: "alice" });
  assert.deepEqual(validateRecipient("   "), { ok: false, code: "invalid_recipient" });
  assert.deepEqual(validateRecipient("x".repeat(64)), { ok: true, username: "x".repeat(64) });
  assert.deepEqual(validateRecipient("x".repeat(65)), { ok: false, code: "invalid_recipient" });
  assert.deepEqual(validateRecipient(undefined), { ok: false, code: "invalid_recipient" });
});

test("idempotency keys are visible ASCII up to 128 characters", () => {
  assert.equal(isValidIdempotencyKey("transfer-0001"), true);
  assert.equal(isValidIdempotencyKey("x".repeat(128)), true);
  for (const value of ["", "x".repeat(129), "has space", "tab\there", "ümlaut", 12, null]) {
    assert.equal(isValidIdempotencyKey(value), false, JSON.stringify(value));
  }
});

test("generated idempotency keys are valid, distinct, and 32 hex characters", () => {
  const first = newIdempotencyKey();
  const second = newIdempotencyKey();
  assert.match(first, /^[0-9a-f]{32}$/);
  assert.notEqual(first, second);
  assert.equal(isValidIdempotencyKey(first), true);
});
