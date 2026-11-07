import assert from "node:assert/strict";
import test from "node:test";
import { TURNSTILE_SCRIPT_URL, TURNSTILE_TEST_SITE_KEY, turnstileSiteKey } from "./turnstile-policy.mjs";

test("the widget uses the always-pass test key when no site key is configured", () => {
  assert.equal(TURNSTILE_TEST_SITE_KEY, "1x00000000000000000000AA");
  assert.equal(turnstileSiteKey(undefined), TURNSTILE_TEST_SITE_KEY);
  assert.equal(turnstileSiteKey(""), TURNSTILE_TEST_SITE_KEY);
  assert.equal(turnstileSiteKey("   "), TURNSTILE_TEST_SITE_KEY);
});

test("a configured site key is trimmed and used as given", () => {
  assert.equal(turnstileSiteKey("  0x4AAAAAAA-example  "), "0x4AAAAAAA-example");
});

test("the only third-party script is the Turnstile loader, rendered explicitly", () => {
  assert.equal(TURNSTILE_SCRIPT_URL, "https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit");
});
