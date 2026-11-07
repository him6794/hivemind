// Account requests for the official website. This process runs beside Nodepool and is
// the only path the site uses to reach the user service. Each route checks its input
// here before the user service is called. A route that a person starts from a form
// also needs a Cloudflare Turnstile token, which is verified here, because Nodepool
// does not check captchas.
import fs from 'node:fs';
import path from 'node:path';
import * as grpc from '@grpc/grpc-js';
import * as protoLoader from '@grpc/proto-loader';
import { createRateLimiter } from '../src/lib/rate-limit.mjs';
import {
  RequestBodyError,
  UNKNOWN_CLIENT_ADDRESS,
  clientAddress,
  readJsonObject,
} from '../src/lib/account-request-policy.mjs';
import {
  isPositiveSafeInteger,
  isValidIdempotencyKey,
  newIdempotencyKey,
  passwordProblem,
  validateEmail,
  validateRecipient,
} from '../src/lib/account-input-policy.mjs';
import { verifyTurnstileToken } from './turnstile.mjs';

const UserRpcMethods = {
  register: 'RegisterUser',
  login: 'Login',
  logout: 'Logout',
  checkEmail: 'CheckEmail',
  balance: 'GetBalance',
  account: 'GetAccount',
  verifyEmail: 'VerifyEmail',
  resendVerification: 'ResendVerification',
  bindEmail: 'BindEmail',
  changePassword: 'ChangePassword',
  transfer: 'TransferCpt',
  requestPasswordReset: 'RequestPasswordReset',
  resetPassword: 'ResetPassword',
  requestUsernameReminder: 'RequestUsernameReminder',
};

const MINUTE_MS = 60 * 1000;
const HOUR_MS = 60 * MINUTE_MS;
// Every account body is small. server.mjs caps what it buffers in any case.
const MAX_BODY_BYTES = 4096;
const WELCOME_GRANT_STATUSES = new Set(['granted', 'pool_empty', 'already_granted']);

const INVALID_EMAIL_MESSAGE = 'Enter a valid email address.';
const REGISTRATION_FAILED_MESSAGE =
  'Could not create the account. Check the username and email, then try again.';
const VERIFICATION_FAILED_MESSAGE =
  'This verification link is invalid, has expired, or was replaced by a newer verification email.';
const RESEND_FAILED_MESSAGE = 'Could not send the verification email. Try again later.';
const CHANGE_PASSWORD_FAILED_MESSAGE =
  'Could not change the password. Check your current password and try again.';
const TRANSFER_FAILED_MESSAGE = 'The transfer could not be completed.';
const RESET_FAILED_MESSAGE = 'This reset link is invalid, has expired, or was replaced by a newer reset email.';
// The account service sends these texts for a duplicate. The BFF matches them exactly and answers
// with a code, so the site can say what is taken. node-manager (grpc.rs) keeps the same texts.
const USERNAME_TAKEN_FROM_SERVICE = 'Username already exists';
const EMAIL_IN_USE_FROM_SERVICE = 'This email address is already in use.';
const USERNAME_TAKEN_MESSAGE = 'This username has already been used.';
const EMAIL_IN_USE_MESSAGE = 'This email address has already been used.';

const PASSWORD_FAILURE_MESSAGES = {
  password_too_short: 'Password must be at least 8 bytes.',
  password_too_long: 'Password must be at most 72 bytes.',
  password_edge_whitespace: 'A password cannot start or end with a space.',
};

// The address limits apply only when a proxy in front of this server supplies the
// visitor's address (WEBSITE_CLIENT_IP_HEADER). Otherwise clientAddress returns
// UNKNOWN_CLIENT_ADDRESS. A shared bucket for every visitor would let one visitor lock
// out all others, so the address limits are skipped in that case. Limits keyed by
// username or email still apply.
const NOT_LIMITED = { allowed: true, retryAfterSeconds: 0 };

// Attempts are counted per running server process. Login is limited per address and
// per username, so one address cannot try many passwords on one account. Routes that
// send email are limited per address and, for the recovery routes, per email address.
// The per-email limit runs after the captcha check, so a request without a valid captcha
// cannot use up another person's budget.
function createLimits() {
  return {
    loginByAddress: createRateLimiter({ limit: 20, windowMs: 5 * MINUTE_MS }),
    loginByUsername: createRateLimiter({ limit: 10, windowMs: 15 * MINUTE_MS }),
    registerByAddress: createRateLimiter({ limit: 5, windowMs: 10 * MINUTE_MS }),
    emailRequestByAddress: createRateLimiter({ limit: 5, windowMs: 10 * MINUTE_MS }),
    recoveryByEmail: createRateLimiter({ limit: 3, windowMs: HOUR_MS }),
    passwordChangeBySession: createRateLimiter({ limit: 5, windowMs: 15 * MINUTE_MS }),
    emailCheckByAddress: createRateLimiter({ limit: 30, windowMs: 10 * MINUTE_MS }),
  };
}

let userClient;

function findProtoPath() {
  const candidates = process.env.WEBSITE_PROTO_PATH
    ? [path.resolve(process.cwd(), process.env.WEBSITE_PROTO_PATH)]
    : [
        path.resolve(process.cwd(), 'proto/hivemind.proto'),
        path.resolve(process.cwd(), '../proto/hivemind.proto'),
      ];
  const protoPath = candidates.find((candidate) => fs.existsSync(candidate));
  if (!protoPath) {
    throw new Error('hivemind.proto is not available to the website backend');
  }
  return protoPath;
}

function getUserClient() {
  if (userClient) return userClient;

  const address = process.env.WEBSITE_NODEPOOL_GRPC_ADDR?.trim();
  if (!address) {
    throw new Error('WEBSITE_NODEPOOL_GRPC_ADDR is not configured');
  }

  const definition = protoLoader.loadSync(findProtoPath(), {
    keepCase: true,
    longs: String,
    enums: String,
    defaults: true,
  });
  const loaded = grpc.loadPackageDefinition(definition);
  const UserService = loaded.nodepool?.UserService;
  if (!UserService) {
    throw new Error('UserService is missing from the protobuf definition');
  }

  // The website backend and Nodepool share a private network. Nodepool is never
  // reachable from a browser.
  userClient = new UserService(address, grpc.credentials.createInsecure());
  return userClient;
}

function callUserService(method, request) {
  return new Promise((resolve, reject) => {
    getUserClient()[method](request, (error, response) => {
      if (error) {
        reject(error);
        return;
      }
      resolve(response ?? {});
    });
  });
}

function textField(value) {
  return typeof value === 'string' ? value.trim() : '';
}

function stringField(value) {
  return typeof value === 'string' ? value : '';
}

function grpcErrorStatus(error) {
  const code = typeof error === 'object' && error !== null && 'code' in error ? error.code : undefined;
  switch (code) {
    case grpc.status.INVALID_ARGUMENT:
      return 400;
    case grpc.status.UNAUTHENTICATED:
      return 401;
    case grpc.status.PERMISSION_DENIED:
      return 403;
    case grpc.status.NOT_FOUND:
      return 404;
    case grpc.status.UNAVAILABLE:
      return 503;
    default:
      return 502;
  }
}

// Every failure carries a stable code. The browser turns the code into a
// localized message, and the English text is only a fallback for other clients.
function failure(status, code, message, headers) {
  return Response.json({ success: false, code, message, status_message: message }, { status, headers });
}

function rpcFailure(method, error) {
  // Log the method and the gRPC status only. Request fields can hold passwords.
  const code = typeof error === 'object' && error !== null && 'code' in error ? error.code : undefined;
  console.error('website account service call failed', { method, code });
  return failure(grpcErrorStatus(error), 'service_unavailable', 'Website account service is unavailable.');
}

// An expired or unrecognised session is UNAUTHENTICATED from the user service. Anything
// else from a signed-in call is an outage.
function sessionOrRpcFailure(method, error) {
  return grpcErrorStatus(error) === 401 ? sessionExpired() : rpcFailure(method, error);
}

function sessionExpired() {
  return failure(401, 'session_expired', 'Your session has expired. Sign in again.');
}

function authenticationRequired() {
  return failure(401, 'authentication_required', 'Sign in to continue.');
}

function rateLimited(retryAfterSeconds) {
  return failure(429, 'rate_limited', 'Too many attempts. Try again later.', {
    'Retry-After': String(retryAfterSeconds),
  });
}

function notFound() {
  return Response.json({ success: false, message: 'Not found' }, { status: 404 });
}

function passwordFailure(code) {
  return failure(400, code, PASSWORD_FAILURE_MESSAGES[code]);
}

function captchaFailure(reason) {
  if (reason === 'not_configured') {
    return failure(503, 'captcha_not_configured', 'Verification is not configured.');
  }
  if (reason === 'unreachable') {
    return failure(503, 'captcha_unavailable', 'Verification is temporarily unavailable. Try again later.');
  }
  return failure(400, 'captcha_failed', 'Verification failed. Try again.');
}

function welcomeGrantStatus(value) {
  return WELCOME_GRANT_STATUSES.has(value) ? value : 'unknown';
}

function accountRoute(request) {
  const pathname = new URL(request.url).pathname;
  return pathname.startsWith('/api/') ? pathname.slice('/api/'.length) : '';
}

function addressOf(request, ctx) {
  return clientAddress(request, ctx.env.WEBSITE_CLIENT_IP_HEADER);
}

function checkAddress(limiter, address) {
  return address === UNKNOWN_CLIENT_ADDRESS ? NOT_LIMITED : limiter.check(address);
}

function releaseAddress(limiter, address) {
  if (address !== UNKNOWN_CLIENT_ADDRESS) limiter.release(address);
}

function bearerToken(request) {
  const match = (request.headers.get('authorization') ?? '').match(/^Bearer\s+(.+)$/i);
  return match ? match[1].trim() : '';
}

// The session is an HttpOnly cookie, so page scripts cannot read it. Scripts and other clients
// can still send the same token as a bearer header.
const SESSION_COOKIE = 'hivemind_session';
// Matches the default lifetime of the token Nodepool issues (24 hours).
const SESSION_MAX_AGE_SECONDS = 24 * 60 * 60;

function cookieValue(request, name) {
  for (const part of (request.headers.get('cookie') ?? '').split(';')) {
    const separator = part.indexOf('=');
    if (separator > 0 && part.slice(0, separator).trim() === name) {
      return part.slice(separator + 1).trim();
    }
  }
  return '';
}

function sessionToken(request) {
  return bearerToken(request) || cookieValue(request, SESSION_COOKIE);
}

function sessionCookie(token, maxAge) {
  return `${SESSION_COOKIE}=${token}; Path=/api; HttpOnly; Secure; SameSite=Lax; Max-Age=${maxAge}`;
}

// The browser attaches the session cookie to every request, including requests another site
// starts. A request that uses the cookie must come from this site, which Fetch Metadata reports.
// A request with its own bearer header is not affected.
function refuseCrossSiteCookie(request) {
  if (bearerToken(request)) return null;
  const site = request.headers.get('sec-fetch-site');
  if (site === null || site === 'same-origin' || site === 'none') return null;
  return failure(403, 'cross_site_refused', 'This request must come from the website itself.');
}

// The body is read through readJsonObject, which enforces the limit while reading, so a
// wrong Content-Length cannot make the server buffer an unbounded body.
async function readRequestBody(request) {
  try {
    return { body: await readJsonObject(request, MAX_BODY_BYTES) };
  } catch (error) {
    if (error instanceof RequestBodyError && error.reason === 'too_large') {
      return { failed: failure(413, 'request_too_large', 'The request is too large.') };
    }
    return { failed: failure(400, 'invalid_request', 'Request body must be a JSON object.') };
  }
}

async function verifyHuman(ctx, captchaToken, address) {
  return ctx.verifyCaptcha({
    token: captchaToken,
    remoteIp: address === UNKNOWN_CLIENT_ADDRESS ? undefined : address,
  });
}

async function postRegister(request, ctx) {
  const parsed = await readRequestBody(request);
  if (parsed.failed) return parsed.failed;

  const username = textField(parsed.body.username);
  const password = stringField(parsed.body.password);
  if (!username || !password.trim()) {
    return failure(400, 'credentials_required', 'Username and password are required.');
  }
  const email = validateEmail(parsed.body.email);
  if (!email.ok) return failure(400, email.code, INVALID_EMAIL_MESSAGE);
  // The password reaches the user service exactly as typed. Edge spaces are refused,
  // because sign-in trims passwords and a silent trim would store a different value.
  const problem = passwordProblem(password);
  if (problem) return passwordFailure(problem);

  const address = addressOf(request, ctx);
  const limit = checkAddress(ctx.limits.registerByAddress, address);
  if (!limit.allowed) return rateLimited(limit.retryAfterSeconds);
  const human = await verifyHuman(ctx, stringField(parsed.body.captchaToken), address);
  if (!human.ok) return captchaFailure(human.reason);

  try {
    const response = await ctx.callUserService(UserRpcMethods.register, {
      username,
      password,
      email: email.email,
    });
    if (response.success !== true) {
      // A duplicate username or address is named, as the site requires. Any other refusal keeps
      // the generic text, and the account service's own wording is never returned.
      if (response.status_message === USERNAME_TAKEN_FROM_SERVICE) {
        return failure(400, 'username_taken', USERNAME_TAKEN_MESSAGE);
      }
      if (response.status_message === EMAIL_IN_USE_FROM_SERVICE) {
        return failure(400, 'email_in_use', EMAIL_IN_USE_MESSAGE);
      }
      return failure(400, 'registration_failed', REGISTRATION_FAILED_MESSAGE);
    }
    return Response.json(
      { success: true, message: 'Registration successful. Check your email to verify the account.' },
      { status: 201 }
    );
  } catch (error) {
    return rpcFailure(UserRpcMethods.register, error);
  }
}

async function postLogin(request, ctx) {
  const parsed = await readRequestBody(request);
  if (parsed.failed) return parsed.failed;

  const username = textField(parsed.body.username);
  const password = stringField(parsed.body.password);
  if (!username || !password.trim()) {
    return failure(400, 'credentials_required', 'Username and password are required.');
  }
  const address = addressOf(request, ctx);
  // The captcha runs before the sign-in limits, so a request that cannot pass it cannot use
  // up another person's sign-in budget for that username.
  const human = await verifyHuman(ctx, stringField(parsed.body.captchaToken), address);
  if (!human.ok) return captchaFailure(human.reason);
  // Sign-in trims the password, as it did before. Accounts created earlier store the
  // trimmed value, so the same trim keeps them reachable.
  return login(ctx, address, username, password.trim());
}

async function login(ctx, address, username, password) {
  const byAddress = checkAddress(ctx.limits.loginByAddress, address);
  const byUsername = ctx.limits.loginByUsername.check(username.toLowerCase());
  if (!byAddress.allowed || !byUsername.allowed) {
    return rateLimited(Math.max(byAddress.retryAfterSeconds, byUsername.retryAfterSeconds));
  }

  try {
    const response = await ctx.callUserService(UserRpcMethods.login, { username, password });
    if (response.success !== true) {
      return failure(401, 'login_failed', 'The username or password is incorrect.');
    }
    const token = textField(response.token);
    if (!token) {
      return rpcFailure(UserRpcMethods.login, new Error('login succeeded without a token'));
    }
    // A successful sign-in gives back its attempt, so signing in and out often does not lock the visitor out.
    // Attempts that do not sign in stay counted.
    ctx.limits.loginByUsername.release(username.toLowerCase());
    releaseAddress(ctx.limits.loginByAddress, address);
    // The token goes into the HttpOnly cookie only. It is not returned in the body.
    return Response.json(
      { success: true, message: 'Login successful', status_message: 'Login successful', username },
      { status: 200, headers: { 'Set-Cookie': sessionCookie(token, SESSION_MAX_AGE_SECONDS) } }
    );
  } catch (error) {
    return rpcFailure(UserRpcMethods.login, error);
  }
}

// Clears the session cookie. The token stays valid until it expires, because the account service
// keeps no session state that could be revoked here.
async function postLogout(request, ctx) {
  const refused = refuseCrossSiteCookie(request);
  if (refused) return refused;
  // Nodepool ends the session, so the token stops working even where a copy of it survives. The cookie is cleared
  // whether or not Nodepool answers.
  const token = sessionToken(request);
  if (token) {
    try {
      await ctx.callUserService(UserRpcMethods.logout, { token });
    } catch {
      // Best effort: the browser forgets the token either way.
    }
  }
  return Response.json(
    { success: true, message: 'Signed out.', status_message: 'Signed out.' },
    { status: 200, headers: { 'Set-Cookie': sessionCookie('', 0) } }
  );
}

// Tells the sign-up form whether a verified account already owns the address, so the visitor learns it on the
// first step. The answer is only available or taken, with no account details. The limit per address keeps the
// check from being used to try many addresses.
async function postCheckEmail(request, ctx) {
  const parsed = await readRequestBody(request);
  if (parsed.failed) return parsed.failed;

  const email = validateEmail(parsed.body.email);
  if (!email.ok) return failure(400, email.code, INVALID_EMAIL_MESSAGE);

  const address = addressOf(request, ctx);
  const limit = checkAddress(ctx.limits.emailCheckByAddress, address);
  if (!limit.allowed) return rateLimited(limit.retryAfterSeconds);

  try {
    const response = await ctx.callUserService(UserRpcMethods.checkEmail, { email: email.email });
    if (response.success !== true) {
      return failure(503, 'service_unavailable', 'Website account service is unavailable.');
    }
    return Response.json({
      success: true,
      available: response.available === true,
      verified: response.verified === true,
    });
  } catch (error) {
    return rpcFailure(UserRpcMethods.checkEmail, error);
  }
}

async function postVerifyEmail(request, ctx) {
  const parsed = await readRequestBody(request);
  if (parsed.failed) return parsed.failed;

  const verificationToken = textField(parsed.body.verificationToken);
  if (!verificationToken) return failure(400, 'verification_failed', VERIFICATION_FAILED_MESSAGE);

  try {
    const response = await ctx.callUserService(UserRpcMethods.verifyEmail, {
      verification_token: verificationToken,
    });
    if (response.success !== true) {
      if (response.status_message === EMAIL_IN_USE_FROM_SERVICE) {
        return failure(400, 'email_in_use', EMAIL_IN_USE_MESSAGE);
      }
      return failure(400, 'verification_failed', VERIFICATION_FAILED_MESSAGE);
    }
    return Response.json({
      success: true,
      message: 'Email verified.',
      welcomeGrantStatus: welcomeGrantStatus(response.welcome_grant_status),
    });
  } catch (error) {
    return rpcFailure(UserRpcMethods.verifyEmail, error);
  }
}

async function postResendVerification(request, ctx) {
  const token = sessionToken(request);
  if (!token) return authenticationRequired();
  const refused = refuseCrossSiteCookie(request);
  if (refused) return refused;
  const parsed = await readRequestBody(request);
  if (parsed.failed) return parsed.failed;

  const address = addressOf(request, ctx);
  const limit = checkAddress(ctx.limits.emailRequestByAddress, address);
  if (!limit.allowed) return rateLimited(limit.retryAfterSeconds);
  const human = await verifyHuman(ctx, stringField(parsed.body.captchaToken), address);
  if (!human.ok) return captchaFailure(human.reason);

  try {
    const response = await ctx.callUserService(UserRpcMethods.resendVerification, { token });
    if (response.success !== true) {
      if (response.status_message === EMAIL_IN_USE_FROM_SERVICE) {
        return failure(400, 'email_in_use', EMAIL_IN_USE_MESSAGE);
      }
      return failure(400, 'resend_failed', RESEND_FAILED_MESSAGE);
    }
    return Response.json({ success: true, message: 'Verification email sent. Earlier verification links no longer work.' });
  } catch (error) {
    return sessionOrRpcFailure(UserRpcMethods.resendVerification, error);
  }
}

async function postBindEmail(request, ctx) {
  const token = sessionToken(request);
  if (!token) return authenticationRequired();
  const refused = refuseCrossSiteCookie(request);
  if (refused) return refused;
  const parsed = await readRequestBody(request);
  if (parsed.failed) return parsed.failed;
  const email = validateEmail(parsed.body.email);
  if (!email.ok) return failure(400, 'invalid_email', INVALID_EMAIL_MESSAGE);
  const currentPassword = stringField(parsed.body.currentPassword).trim();
  if (!currentPassword) return failure(400, 'password_required', 'Enter your current password.');
  const limit = ctx.limits.passwordChangeBySession.check(token);
  if (!limit.allowed) return rateLimited(limit.retryAfterSeconds);
  const address = addressOf(request, ctx);
  const mailLimit = checkAddress(ctx.limits.emailRequestByAddress, address);
  if (!mailLimit.allowed) return rateLimited(mailLimit.retryAfterSeconds);
  const human = await verifyHuman(ctx, stringField(parsed.body.captchaToken), address);
  if (!human.ok) return captchaFailure(human.reason);
  try {
    const response = await ctx.callUserService(UserRpcMethods.bindEmail, {
      token, current_password: currentPassword, email: email.email,
    });
    if (response.success !== true) {
      if (response.status_message === EMAIL_IN_USE_FROM_SERVICE) {
        return failure(400, 'email_in_use', EMAIL_IN_USE_MESSAGE);
      }
      return failure(400, 'bind_email_failed', 'Could not add the email. Check your current password and account email.');
    }
    return Response.json({ success: true, message: 'Email added. Verify it before transferring CPT.' });
  } catch (error) {
    return sessionOrRpcFailure(UserRpcMethods.bindEmail, error);
  }
}

async function postChangePassword(request, ctx) {
  const token = sessionToken(request);
  if (!token) return authenticationRequired();
  const refused = refuseCrossSiteCookie(request);
  if (refused) return refused;
  const parsed = await readRequestBody(request);
  if (parsed.failed) return parsed.failed;

  const currentPassword = stringField(parsed.body.currentPassword);
  const newPassword = stringField(parsed.body.newPassword);
  if (!currentPassword.trim()) {
    return failure(400, 'password_required', 'Enter your current password.');
  }
  const problem = passwordProblem(newPassword);
  if (problem) return passwordFailure(problem);

  // The current password is checked on every attempt, so attempts are limited per session.
  const limit = ctx.limits.passwordChangeBySession.check(token);
  if (!limit.allowed) return rateLimited(limit.retryAfterSeconds);

  try {
    // Sign-in trims the password it receives. The current password is trimmed the same way
    // so it matches the stored value.
    const response = await ctx.callUserService(UserRpcMethods.changePassword, {
      token,
      current_password: currentPassword.trim(),
      new_password: newPassword,
    });
    if (response.success !== true) {
      return failure(400, 'change_password_failed', CHANGE_PASSWORD_FAILED_MESSAGE);
    }
    return Response.json({ success: true, message: 'Password changed.' });
  } catch (error) {
    return sessionOrRpcFailure(UserRpcMethods.changePassword, error);
  }
}

async function postTransfer(request, ctx) {
  const token = sessionToken(request);
  if (!token) return authenticationRequired();
  const refused = refuseCrossSiteCookie(request);
  if (refused) return refused;
  const parsed = await readRequestBody(request);
  if (parsed.failed) return parsed.failed;

  const recipient = validateRecipient(parsed.body.toUsername);
  if (!recipient.ok) return failure(400, recipient.code, 'Enter the recipient username.');
  const amountCpt = parsed.body.amountCpt;
  if (!isPositiveSafeInteger(amountCpt)) {
    return failure(400, 'invalid_amount', 'Enter a whole number of CPT greater than 0.');
  }
  const suppliedKey = parsed.body.idempotencyKey;
  const hasKey = suppliedKey !== undefined && suppliedKey !== null;
  if (hasKey && !isValidIdempotencyKey(suppliedKey)) {
    return failure(400, 'invalid_request', 'The idempotency key is not valid.');
  }
  // A caller that retries should send its own key. Otherwise each request gets a new one.
  const idempotencyKey = hasKey ? suppliedKey : newIdempotencyKey();

  try {
    const response = await ctx.callUserService(UserRpcMethods.transfer, {
      token,
      to_username: recipient.username,
      amount_cpt: amountCpt,
      idempotency_key: idempotencyKey,
    });
    if (response.success !== true) {
      return failure(400, 'transfer_failed', TRANSFER_FAILED_MESSAGE);
    }
    return Response.json({
      success: true,
      message: 'Transfer completed.',
      fromBalance: Number(response.from_balance ?? 0),
      toBalance: Number(response.to_balance ?? 0),
      transferId: textField(response.transfer_id),
    });
  } catch (error) {
    return sessionOrRpcFailure(UserRpcMethods.transfer, error);
  }
}

// Password reset and username reminder share one flow. The answer is the same whether
// or not an account uses the address, so the user service's answer is not returned.
function recoveryRoute(method, message) {
  return async function recover(request, ctx) {
    const parsed = await readRequestBody(request);
    if (parsed.failed) return parsed.failed;

    const email = validateEmail(parsed.body.email);
    if (!email.ok) return failure(400, email.code, INVALID_EMAIL_MESSAGE);

    const address = addressOf(request, ctx);
    const byAddress = checkAddress(ctx.limits.emailRequestByAddress, address);
    if (!byAddress.allowed) return rateLimited(byAddress.retryAfterSeconds);
    const human = await verifyHuman(ctx, stringField(parsed.body.captchaToken), address);
    if (!human.ok) return captchaFailure(human.reason);
    const byEmail = ctx.limits.recoveryByEmail.check(email.email.toLowerCase());
    if (!byEmail.allowed) return rateLimited(byEmail.retryAfterSeconds);

    try {
      await ctx.callUserService(method, { email: email.email });
      return Response.json({ success: true, message });
    } catch (error) {
      return rpcFailure(method, error);
    }
  };
}

const postForgotPassword = recoveryRoute(
  UserRpcMethods.requestPasswordReset,
  'If an account uses that email address, a password reset link has been sent.'
);

const postForgotUsername = recoveryRoute(
  UserRpcMethods.requestUsernameReminder,
  'If a verified account uses that email address, the username has been sent to it.'
);

async function postResetPassword(request, ctx) {
  const parsed = await readRequestBody(request);
  if (parsed.failed) return parsed.failed;

  const resetToken = textField(parsed.body.resetToken);
  const newPassword = stringField(parsed.body.newPassword);
  if (!resetToken) return failure(400, 'reset_failed', RESET_FAILED_MESSAGE);
  const problem = passwordProblem(newPassword);
  if (problem) return passwordFailure(problem);

  try {
    const response = await ctx.callUserService(UserRpcMethods.resetPassword, {
      reset_token: resetToken,
      new_password: newPassword,
    });
    if (response.success !== true) {
      return failure(400, 'reset_failed', RESET_FAILED_MESSAGE);
    }
    return Response.json({ success: true, message: 'Password reset. Sign in with the new password.' });
  } catch (error) {
    return rpcFailure(UserRpcMethods.resetPassword, error);
  }
}

async function getBalance(request, ctx) {
  const token = sessionToken(request);
  if (!token) {
    return failure(401, 'authentication_required', 'Sign in to view your balance.');
  }
  const refused = refuseCrossSiteCookie(request);
  if (refused) return refused;

  try {
    const response = await ctx.callUserService(UserRpcMethods.balance, { username: '', token });
    if (response.success !== true) return sessionExpired();
    return Response.json(
      {
        success: true,
        balance: Number(response.balance ?? 0),
        message: 'OK',
        status_message: 'OK',
      },
      { status: 200 }
    );
  } catch (error) {
    return sessionOrRpcFailure(UserRpcMethods.balance, error);
  }
}

async function getAccount(request, ctx) {
  const token = sessionToken(request);
  if (!token) return authenticationRequired();
  const refused = refuseCrossSiteCookie(request);
  if (refused) return refused;

  try {
    const response = await ctx.callUserService(UserRpcMethods.account, { token });
    if (response.success !== true) return sessionExpired();
    return Response.json({
      success: true,
      message: 'OK',
      username: textField(response.username),
      email: textField(response.email),
      emailVerified: response.email_verified === true,
      balance: Number(response.balance ?? 0),
      welcomeGrantStatus: welcomeGrantStatus(response.welcome_grant_status),
    });
  } catch (error) {
    return sessionOrRpcFailure(UserRpcMethods.account, error);
  }
}

// Each entry is one path under /api/. Adding a route here also adds it to ACCOUNT_API_PATHS,
// which server.mjs uses as its allowlist.
const POST_ROUTES = new Map([
  ['register', postRegister],
  ['login', postLogin],
  ['logout', postLogout],
  ['check-email', postCheckEmail],
  ['verify-email', postVerifyEmail],
  ['resend-verification', postResendVerification],
  ['bind-email', postBindEmail],
  ['change-password', postChangePassword],
  ['transfer', postTransfer],
  ['forgot-password', postForgotPassword],
  ['reset-password', postResetPassword],
  ['forgot-username', postForgotUsername],
]);

const GET_ROUTES = new Map([
  ['balance', getBalance],
  ['account', getAccount],
]);

export const ACCOUNT_API_PATHS = Object.freeze(
  [...POST_ROUTES.keys(), ...GET_ROUTES.keys()].map((name) => `/api/${name}`)
);

/**
 * Builds the account handler. The options exist for tests: they replace the user service,
 * the captcha check, the fetch used for the captcha check, and the environment. Production
 * uses the defaults, which read the environment on every request.
 */
export function createAccountApi(options = {}) {
  const env = options.env ?? process.env;
  const fetchImpl = options.fetchImpl ?? globalThis.fetch;
  const ctx = {
    env,
    callUserService: options.callUserService ?? callUserService,
    verifyCaptcha:
      options.verifyCaptcha ??
      (({ token, remoteIp }) =>
        verifyTurnstileToken({ secret: env.TURNSTILE_SECRET_KEY, token, remoteIp, fetchImpl })),
    limits: createLimits(),
  };

  return {
    async handle(request) {
      const route = accountRoute(request);
      if (request.method === 'POST') {
        const handler = POST_ROUTES.get(route);
        return handler ? handler(request, ctx) : notFound();
      }
      if (request.method === 'GET') {
        const handler = GET_ROUTES.get(route);
        return handler ? handler(request, ctx) : notFound();
      }
      return Response.json(
        { success: false, code: 'method_not_allowed', message: 'Method not allowed.' },
        { status: 405, headers: { Allow: 'GET, POST' } }
      );
    },
  };
}

const defaultAccountApi = createAccountApi();

export async function handleAccountRequest(request) {
  return defaultAccountApi.handle(request);
}
