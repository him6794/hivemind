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

test("account error codes map to a message in the visitor's language", async () => {
  const { accountErrorMessage } = await loadPolicy();

  assert.equal(accountErrorMessage("rate_limited", "en"), "Too many attempts. Try again later.");
  assert.equal(accountErrorMessage("rate_limited", "zh"), "嘗試次數過多，請稍後再試。");
  assert.equal(accountErrorMessage("login_failed", "zh"), "使用者名稱或密碼不正確。");
  // Unknown codes and prototype keys fall back to the generic message.
  assert.equal(accountErrorMessage("unknown_code", "en"), accountErrorMessage("request_failed", "en"));
  assert.equal(accountErrorMessage("toString", "en"), accountErrorMessage("request_failed", "en"));
});
