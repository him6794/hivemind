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
