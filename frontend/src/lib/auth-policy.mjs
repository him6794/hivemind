import {
  passwordProblem,
  parseAmountCpt,
  validateEmail,
  validateRecipient,
} from "./account-input-policy.mjs";

function text(value) {
  return typeof value === "string" ? value : "";
}

/**
 * @typedef {{ ok: true, username: string } | { ok: false, code: "credentials_required" }} LoginValidation
 */

/** @returns {LoginValidation} */
export function validateLoginInput(username, password) {
  const normalizedUsername = text(username).trim();
  if (!normalizedUsername || !text(password)) {
    return { ok: false, code: "credentials_required" };
  }
  return { ok: true, username: normalizedUsername };
}

/**
 * @typedef {{
 *   ok: true,
 *   username: string,
 *   email: string
 * } | {
 *   ok: false,
 *   code: "username_too_short" | "invalid_email" | "password_too_short" | "password_too_long" | "password_edge_whitespace" | "password_mismatch"
 * }} RegistrationValidation
 */

/** @returns {RegistrationValidation} */
export function validateRegistrationInput(username, password, confirmation, email) {
  const normalizedUsername = text(username).trim();
  if (normalizedUsername.length < 3) {
    return { ok: false, code: "username_too_short" };
  }
  const address = validateEmail(email);
  if (!address.ok) {
    return { ok: false, code: "invalid_email" };
  }
  // The website backend trims passwords at sign-in. Refusing edge spaces here keeps the
  // password a user typed the same password that is stored.
  const problem = passwordProblem(password);
  if (problem) {
    return { ok: false, code: problem };
  }
  if (password !== confirmation) {
    return { ok: false, code: "password_mismatch" };
  }
  return { ok: true, username: normalizedUsername, email: address.email };
}

/**
 * @typedef {{ ok: true } | {
 *   ok: false,
 *   code: "password_required" | "password_too_short" | "password_too_long" | "password_edge_whitespace" | "password_mismatch"
 * }} PasswordChangeValidation
 */

/** @returns {PasswordChangeValidation} */
export function validateChangePasswordInput(currentPassword, newPassword, confirmation) {
  if (!text(currentPassword).trim()) {
    return { ok: false, code: "password_required" };
  }
  const problem = passwordProblem(newPassword);
  if (problem) {
    return { ok: false, code: problem };
  }
  if (newPassword !== confirmation) {
    return { ok: false, code: "password_mismatch" };
  }
  return { ok: true };
}

/**
 * @typedef {{ ok: true } | {
 *   ok: false,
 *   code: "password_too_short" | "password_too_long" | "password_edge_whitespace" | "password_mismatch"
 * }} ResetPasswordValidation
 */

/** @returns {ResetPasswordValidation} */
export function validateResetPasswordInput(newPassword, confirmation) {
  const problem = passwordProblem(newPassword);
  if (problem) {
    return { ok: false, code: problem };
  }
  if (newPassword !== confirmation) {
    return { ok: false, code: "password_mismatch" };
  }
  return { ok: true };
}

/**
 * @typedef {{ ok: true, toUsername: string, amountCpt: number } | {
 *   ok: false,
 *   code: "invalid_recipient" | "invalid_amount"
 * }} TransferValidation
 */

/** @returns {TransferValidation} */
export function validateTransferInput(recipient, amountText) {
  const to = validateRecipient(recipient);
  if (!to.ok) {
    return to;
  }
  const amountCpt = parseAmountCpt(amountText);
  if (amountCpt === null) {
    return { ok: false, code: "invalid_amount" };
  }
  return { ok: true, toUsername: to.username, amountCpt };
}
