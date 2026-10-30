// Account requests for the official website. This is the website backend: it runs
// as its own process beside Nodepool and is the only path the site uses to reach
// the user service. It was moved here unchanged from the Next.js route handler, so
// the limits, body bound, error codes, and generic messages stay as they were.
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

const UserRpcMethods = {
  register: 'RegisterUser',
  login: 'Login',
  balance: 'GetBalance',
};

const MINUTE_MS = 60 * 1000;
const MAX_BODY_BYTES = 4096;

// Attempts are counted per running server process, and the account service is
// not called once a limit is reached. Login is limited per address and per
// username, so one address cannot try many passwords on one account.
const loginByAddress = createRateLimiter({ limit: 20, windowMs: 5 * MINUTE_MS });
const loginByUsername = createRateLimiter({ limit: 10, windowMs: 15 * MINUTE_MS });
const registerByAddress = createRateLimiter({ limit: 5, windowMs: 10 * MINUTE_MS });

// The address limits apply only when a proxy in front of this server supplies the
// visitor's address (WEBSITE_CLIENT_IP_HEADER). Otherwise clientAddress returns
// UNKNOWN_CLIENT_ADDRESS. A shared bucket for every visitor would let one visitor
// lock out all others, so the address limits are skipped in that case. The
// per-username login limit still applies.
const NOT_LIMITED = { allowed: true, retryAfterSeconds: 0 };

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

function rateLimited(retryAfterSeconds) {
  return failure(429, 'rate_limited', 'Too many attempts. Try again later.', {
    'Retry-After': String(retryAfterSeconds),
  });
}

function notFound() {
  return Response.json({ success: false, message: 'Not found' }, { status: 404 });
}

function accountRoute(request) {
  const pathname = new URL(request.url).pathname;
  return pathname.startsWith('/api/') ? pathname.slice('/api/'.length) : '';
}

async function register(address, username, password) {
  const limit = address === UNKNOWN_CLIENT_ADDRESS ? NOT_LIMITED : registerByAddress.check(address);
  if (!limit.allowed) return rateLimited(limit.retryAfterSeconds);

  // The account service receives the password exactly as typed. Edge spaces are
  // refused here, because sign-in trims passwords and a silent trim would make
  // the stored value differ from what the visitor typed.
  if (password !== password.trim()) {
    return failure(400, 'password_edge_whitespace', 'A password cannot start or end with a space.');
  }

  try {
    const response = await callUserService(UserRpcMethods.register, { username, password });
    if (response.success !== true) {
      // Do not pass the account service's text through. Its duplicate-username
      // message would tell anyone which usernames exist.
      return failure(
        400,
        'registration_failed',
        'Could not create the account. Choose a different username and try again.'
      );
    }
    return Response.json({ success: true, message: 'Registration successful' }, { status: 201 });
  } catch (error) {
    return rpcFailure(UserRpcMethods.register, error);
  }
}

async function login(address, username, password) {
  const byAddress = address === UNKNOWN_CLIENT_ADDRESS ? NOT_LIMITED : loginByAddress.check(address);
  const byUsername = loginByUsername.check(username.toLowerCase());
  if (!byAddress.allowed || !byUsername.allowed) {
    return rateLimited(Math.max(byAddress.retryAfterSeconds, byUsername.retryAfterSeconds));
  }

  try {
    const response = await callUserService(UserRpcMethods.login, { username, password });
    if (response.success !== true) {
      return failure(401, 'login_failed', 'The username or password is incorrect.');
    }
    const token = textField(response.token);
    if (!token) {
      return rpcFailure(UserRpcMethods.login, new Error('login succeeded without a token'));
    }
    return Response.json(
      { success: true, message: 'Login successful', status_message: 'Login successful', token },
      { status: 200 }
    );
  } catch (error) {
    return rpcFailure(UserRpcMethods.login, error);
  }
}

async function postAccount(request, route) {
  if (route !== 'register' && route !== 'login') return notFound();

  let body;
  try {
    body = await readJsonObject(request, MAX_BODY_BYTES);
  } catch (error) {
    if (error instanceof RequestBodyError && error.reason === 'too_large') {
      return failure(413, 'request_too_large', 'The request is too large.');
    }
    return failure(400, 'invalid_request', 'Request body must be a JSON object.');
  }

  const username = textField(body.username);
  const password = typeof body.password === 'string' ? body.password : '';
  if (!username || !password.trim()) {
    return failure(400, 'credentials_required', 'Username and password are required.');
  }

  const address = clientAddress(request, process.env.WEBSITE_CLIENT_IP_HEADER);
  if (route === 'register') {
    return register(address, username, password);
  }
  // Sign-in trims the password, as it did before. Accounts created earlier store
  // the trimmed value, so the same trim keeps them reachable.
  return login(address, username, password.trim());
}

async function getAccount(request, route) {
  if (route !== 'balance') return notFound();

  const authorization = request.headers.get('authorization') ?? '';
  const match = authorization.match(/^Bearer\s+(.+)$/i);
  if (!match) {
    return failure(401, 'authentication_required', 'Sign in to view your balance.');
  }

  try {
    const response = await callUserService(UserRpcMethods.balance, {
      username: '',
      token: match[1],
    });
    if (response.success !== true) {
      return failure(401, 'session_expired', 'Your session has expired. Sign in again.');
    }
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
    // The account service rejects a token it does not recognise with UNAUTHENTICATED. That is
    // an expired or invalid session, not an outage.
    if (grpcErrorStatus(error) === 401) {
      return failure(401, 'session_expired', 'Your session has expired. Sign in again.');
    }
    return rpcFailure(UserRpcMethods.balance, error);
  }
}

export async function handleAccountRequest(request) {
  const route = accountRoute(request);
  if (request.method === 'POST') return postAccount(request, route);
  if (request.method === 'GET') return getAccount(request, route);
  return Response.json({ success: false, code: 'method_not_allowed', message: 'Method not allowed.' }, { status: 405, headers: { Allow: 'GET, POST' } });
}
