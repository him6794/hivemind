// Input rules shared by the website forms and the website backend. The browser uses
// them for immediate feedback, and the backend applies the same rules before a request
// reaches Nodepool. Nodepool still decides every value it stores.

export const EMAIL_MAX_LENGTH = 254;
export const EMAIL_LOCAL_PART_MAX_LENGTH = 64;
export const PASSWORD_MIN_BYTES = 8;
export const PASSWORD_MAX_BYTES = 72;
export const RECIPIENT_MAX_LENGTH = 64;
export const IDEMPOTENCY_KEY_MAX_LENGTH = 128;

// Basic shape only: local@domain.tld, with dot-separated local parts, letter and digit
// domain labels, and at least one dot before the top-level label.
const EMAIL_PATTERN =
  /^[A-Za-z0-9!#$%&'*+\/=?^_`{|}~-]+(?:\.[A-Za-z0-9!#$%&'*+\/=?^_`{|}~-]+)*@[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?(?:\.[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?)+$/;

// Visible ASCII only, so a key is safe in a header and its length is its byte count.
const IDEMPOTENCY_KEY_PATTERN = /^[\x21-\x7e]+$/;

/**
 * @returns {{ ok: true, email: string } | { ok: false, code: "invalid_email" }}
 */
export function validateEmail(value) {
  const email = typeof value === 'string' ? value.trim() : '';
  const valid =
    email.length > 0 &&
    email.length <= EMAIL_MAX_LENGTH &&
    email.indexOf('@') <= EMAIL_LOCAL_PART_MAX_LENGTH &&
    EMAIL_PATTERN.test(email);
  return valid ? { ok: true, email } : { ok: false, code: 'invalid_email' };
}

/** The length of a password in UTF-8 bytes, the unit the account service counts. */
export function passwordBytes(value) {
  return new TextEncoder().encode(value).length;
}

/**
 * The rule for every password the site sets: 8 to 72 UTF-8 bytes, with no leading or
 * trailing whitespace. Sign-in trims the password it receives, so a password with edge
 * spaces could never be entered again.
 * @returns {"password_too_short" | "password_too_long" | "password_edge_whitespace" | null}
 */
export function passwordProblem(value) {
  if (typeof value !== 'string') return 'password_too_short';
  const bytes = passwordBytes(value);
  if (bytes < PASSWORD_MIN_BYTES) return 'password_too_short';
  if (bytes > PASSWORD_MAX_BYTES) return 'password_too_long';
  if (value !== value.trim()) return 'password_edge_whitespace';
  return null;
}

/** A whole number of CPT that is safe as a JSON number and as the stored int64. */
export function isPositiveSafeInteger(value) {
  return typeof value === 'number' && Number.isSafeInteger(value) && value > 0;
}

/** Reads the text typed into an amount field. Only plain digits are accepted. */
export function parseAmountCpt(text) {
  const trimmed = typeof text === 'string' ? text.trim() : '';
  if (!/^[0-9]+$/.test(trimmed)) return null;
  const amount = Number(trimmed);
  return isPositiveSafeInteger(amount) ? amount : null;
}

/**
 * @returns {{ ok: true, username: string } | { ok: false, code: "invalid_recipient" }}
 */
export function validateRecipient(value) {
  const username = typeof value === 'string' ? value.trim() : '';
  if (!username || username.length > RECIPIENT_MAX_LENGTH) {
    return { ok: false, code: 'invalid_recipient' };
  }
  return { ok: true, username };
}

export function isValidIdempotencyKey(value) {
  return (
    typeof value === 'string' &&
    value.length >= 1 &&
    value.length <= IDEMPOTENCY_KEY_MAX_LENGTH &&
    IDEMPOTENCY_KEY_PATTERN.test(value)
  );
}

/**
 * A random key for one transfer attempt. Reuse it when retrying the same attempt so a
 * repeated request cannot move CPT twice. It uses getRandomValues, which works on
 * plain-HTTP origins where crypto.randomUUID is unavailable.
 */
export function newIdempotencyKey() {
  const bytes = new Uint8Array(16);
  globalThis.crypto.getRandomValues(bytes);
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, '0')).join('');
}
