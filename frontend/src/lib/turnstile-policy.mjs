// The Cloudflare Turnstile widget on the site. The site key is public, and it comes from
// NEXT_PUBLIC_TURNSTILE_SITE_KEY at build time. When the variable is unset, the documented
// always-pass test key is used so local development works without an account. A production
// build must set the real key. The website backend still refuses every token that the real
// secret does not confirm.
export const TURNSTILE_TEST_SITE_KEY = "1x00000000000000000000AA";

export const TURNSTILE_SCRIPT_URL = "https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit";

export function turnstileSiteKey(configured) {
  const key = typeof configured === "string" ? configured.trim() : "";
  return key || TURNSTILE_TEST_SITE_KEY;
}
