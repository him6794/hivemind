import { passwordProblem, validateEmail } from "./account-input-policy.mjs";

/**
 * The steps of sign-up, in order. Each step asks for one thing. The last step confirms the details and
 * sends them with the human verification.
 */
export const REGISTER_STEPS = Object.freeze(["email", "username", "password", "review"]);

const USERNAME_MIN_LENGTH = 3;

function text(value) {
  return typeof value === "string" ? value : "";
}

/**
 * Checks the fields of one step. The codes are the ones the account policy already translates.
 * @param {string} step one of REGISTER_STEPS
 * @param {{ email: string, username: string, password: string, confirm: string }} values
 * @returns {{ ok: true } | { ok: false, code: string }}
 */
export function validateRegisterStep(step, values) {
  if (step === "email") {
    return validateEmail(text(values.email)).ok ? { ok: true } : { ok: false, code: "invalid_email" };
  }
  if (step === "username") {
    return text(values.username).trim().length >= USERNAME_MIN_LENGTH
      ? { ok: true }
      : { ok: false, code: "username_too_short" };
  }
  if (step === "password") {
    const problem = passwordProblem(text(values.password));
    if (problem) return { ok: false, code: problem };
    return text(values.password) === text(values.confirm) ? { ok: true } : { ok: false, code: "password_mismatch" };
  }
  return { ok: true };
}

/**
 * The first step that still has a problem, with its code, or null when every field passes.
 * @returns {{ step: string, code: string } | null}
 */
export function firstInvalidRegisterStep(values) {
  for (const step of REGISTER_STEPS.slice(0, -1)) {
    const result = validateRegisterStep(step, values);
    if (!result.ok) return { step, code: result.code };
  }
  return null;
}

/**
 * The step a refusal from the server belongs to, so the visitor goes back to the field that needs changing.
 * Anything without a field of its own, such as the human verification, stays on the review step.
 */
export function stepForRegisterError(code) {
  if (code === "email_in_use" || code === "invalid_email") return "email";
  if (code === "username_taken" || code === "username_too_short") return "username";
  if (typeof code === "string" && code.startsWith("password_")) return "password";
  return "review";
}
