import assert from "node:assert/strict";
import test from "node:test";

async function loadPolicy() {
  try {
    return await import("./auth-policy.mjs");
  } catch (error) {
    assert.fail(`auth-policy.mjs must exist and load: ${error.message}`);
  }
}

const EMAIL = "alice@example.com";

test("login validation trims usernames and rejects missing credentials", async () => {
  const { validateLoginInput } = await loadPolicy();

  assert.deepEqual(validateLoginInput("  alice  ", "correct horse"), {
    ok: true,
    username: "alice",
  });
  assert.deepEqual(validateLoginInput(" ", "correct horse"), {
    ok: false,
    code: "credentials_required",
  });
  assert.deepEqual(validateLoginInput("alice", ""), {
    ok: false,
    code: "credentials_required",
  });
});

test("registration validation mirrors the nodepool username and password contract", async () => {
  const { validateRegistrationInput } = await loadPolicy();

  assert.deepEqual(validateRegistrationInput("  alice  ", "long-enough", "long-enough", EMAIL), {
    ok: true,
    username: "alice",
    email: EMAIL,
  });
  assert.deepEqual(validateRegistrationInput("ab", "long-enough", "long-enough", EMAIL), {
    ok: false,
    code: "username_too_short",
  });
  assert.deepEqual(validateRegistrationInput("alice", "short", "short", EMAIL), {
    ok: false,
    code: "password_too_short",
  });
  assert.deepEqual(validateRegistrationInput("alice", "long-enough", "different", EMAIL), {
    ok: false,
    code: "password_mismatch",
  });
});

test("registration refuses passwords with leading or trailing spaces", async () => {
  const { validateRegistrationInput } = await loadPolicy();

  assert.deepEqual(validateRegistrationInput("alice", "  long-enough", "  long-enough", EMAIL), {
    ok: false,
    code: "password_edge_whitespace",
  });
  assert.deepEqual(validateRegistrationInput("alice", "long-enough ", "long-enough ", EMAIL), {
    ok: false,
    code: "password_edge_whitespace",
  });
  assert.deepEqual(validateRegistrationInput("alice", "long enough", "long enough", EMAIL), {
    ok: true,
    username: "alice",
    email: EMAIL,
  });
});

test("registration requires a valid email and trims it", async () => {
  const { validateRegistrationInput } = await loadPolicy();

  assert.deepEqual(validateRegistrationInput("alice", "long-enough", "long-enough", "  Alice@Example.com  "), {
    ok: true,
    username: "alice",
    email: "Alice@Example.com",
  });
  for (const email of ["", "alice", "alice@localhost", undefined, null]) {
    assert.deepEqual(
      validateRegistrationInput("alice", "long-enough", "long-enough", email),
      { ok: false, code: "invalid_email" },
      String(email)
    );
  }
});

test("registration counts the password limit in UTF-8 bytes", async () => {
  const { validateRegistrationInput } = await loadPolicy();

  assert.deepEqual(validateRegistrationInput("alice", "a".repeat(73), "a".repeat(73), EMAIL), {
    ok: false,
    code: "password_too_long",
  });
  assert.deepEqual(validateRegistrationInput("alice", "é".repeat(36), "é".repeat(36), EMAIL), {
    ok: true,
    username: "alice",
    email: EMAIL,
  });
});

test("change-password validation needs the current password and a valid new password that matches", async () => {
  const { validateChangePasswordInput } = await loadPolicy();

  assert.deepEqual(validateChangePasswordInput("old password", "new password 2", "new password 2"), { ok: true });
  assert.deepEqual(validateChangePasswordInput("  ", "new password 2", "new password 2"), {
    ok: false,
    code: "password_required",
  });
  assert.deepEqual(validateChangePasswordInput("old", "short", "short"), {
    ok: false,
    code: "password_too_short",
  });
  assert.deepEqual(validateChangePasswordInput("old", "new password 2", "new password 3"), {
    ok: false,
    code: "password_mismatch",
  });
  assert.deepEqual(validateChangePasswordInput("old", " new password 2", " new password 2"), {
    ok: false,
    code: "password_edge_whitespace",
  });
});

test("reset-password validation checks the new password and its confirmation", async () => {
  const { validateResetPasswordInput } = await loadPolicy();

  assert.deepEqual(validateResetPasswordInput("brand new password", "brand new password"), { ok: true });
  assert.deepEqual(validateResetPasswordInput("a".repeat(73), "a".repeat(73)), {
    ok: false,
    code: "password_too_long",
  });
  assert.deepEqual(validateResetPasswordInput("brand new password", "brand new passwords"), {
    ok: false,
    code: "password_mismatch",
  });
});

test("transfer validation trims the recipient and accepts only whole CPT amounts", async () => {
  const { validateTransferInput } = await loadPolicy();

  assert.deepEqual(validateTransferInput("  bob  ", " 25 "), {
    ok: true,
    toUsername: "bob",
    amountCpt: 25,
  });
  assert.deepEqual(validateTransferInput("  ", "25"), { ok: false, code: "invalid_recipient" });
  for (const amount of ["", "0", "-3", "2.5", "1e3", "ten"]) {
    assert.deepEqual(validateTransferInput("bob", amount), { ok: false, code: "invalid_amount" }, amount);
  }
});
