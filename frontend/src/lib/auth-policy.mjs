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
 *   username: string
 * } | {
 *   ok: false,
 *   code: "username_too_short" | "password_too_short" | "password_mismatch"
 * }} RegistrationValidation
 */

/** @returns {RegistrationValidation} */
export function validateRegistrationInput(username, password, confirmation) {
  const normalizedUsername = text(username).trim();
  if (normalizedUsername.length < 3) {
    return { ok: false, code: "username_too_short" };
  }
  if (text(password).length < 8) {
    return { ok: false, code: "password_too_short" };
  }
  if (password !== confirmation) {
    return { ok: false, code: "password_mismatch" };
  }
  return { ok: true, username: normalizedUsername };
}
