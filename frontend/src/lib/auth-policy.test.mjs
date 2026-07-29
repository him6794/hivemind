import assert from "node:assert/strict";
import test from "node:test";

async function loadPolicy() {
  try {
    return await import("./auth-policy.mjs");
  } catch (error) {
    assert.fail(`auth-policy.mjs must exist and load: ${error.message}`);
  }
}

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

  assert.deepEqual(validateRegistrationInput("  alice  ", "long-enough", "long-enough"), {
    ok: true,
    username: "alice",
  });
  assert.deepEqual(validateRegistrationInput("ab", "long-enough", "long-enough"), {
    ok: false,
    code: "username_too_short",
  });
  assert.deepEqual(validateRegistrationInput("alice", "short", "short"), {
    ok: false,
    code: "password_too_short",
  });
  assert.deepEqual(validateRegistrationInput("alice", "long-enough", "different"), {
    ok: false,
    code: "password_mismatch",
  });
});
