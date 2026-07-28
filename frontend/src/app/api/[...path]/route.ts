import fs from "node:fs";
import path from "node:path";
import * as grpc from "@grpc/grpc-js";
import * as protoLoader from "@grpc/proto-loader";

type JsonRecord = Record<string, unknown>;
type UnaryMethod = (
  request: JsonRecord,
  callback: (error: grpc.ServiceError | null, response: JsonRecord) => void
) => void;
type UserServiceClient = grpc.Client & Record<string, UnaryMethod>;

const UserRpcMethods = {
  register: "RegisterUser",
  login: "Login",
  balance: "GetBalance",
} as const;

type UserRpcMethod = (typeof UserRpcMethods)[keyof typeof UserRpcMethods];

let userClient: UserServiceClient | undefined;

function findProtoPath() {
  const candidates = [
    path.resolve(process.cwd(), "proto/hivemind.proto"),
    path.resolve(process.cwd(), "../proto/hivemind.proto"),
  ];
  const protoPath = candidates.find((candidate) => fs.existsSync(candidate));
  if (!protoPath) {
    throw new Error("hivemind.proto is not available to the website backend");
  }
  return protoPath;
}

function getUserClient() {
  if (userClient) return userClient;

  const address = process.env.WEBSITE_NODEPOOL_GRPC_ADDR?.trim();
  if (!address) {
    throw new Error("WEBSITE_NODEPOOL_GRPC_ADDR is not configured");
  }

  const definition = protoLoader.loadSync(findProtoPath(), {
    keepCase: true,
    longs: String,
    enums: String,
    defaults: true,
  });
  const loaded = grpc.loadPackageDefinition(definition) as unknown as {
    nodepool?: {
      UserService?: grpc.ServiceClientConstructor;
    };
  };
  const UserService = loaded.nodepool?.UserService;
  if (!UserService) {
    throw new Error("UserService is missing from the protobuf definition");
  }

  userClient = new UserService(address, grpc.credentials.createInsecure()) as UserServiceClient;
  return userClient;
}

function callUserService(method: UserRpcMethod, request: JsonRecord) {
  return new Promise<JsonRecord>((resolve, reject) => {
    getUserClient()[method](request, (error, response) => {
      if (error) {
        reject(error);
        return;
      }
      resolve(response ?? {});
    });
  });
}

function textField(value: unknown) {
  return typeof value === "string" ? value.trim() : "";
}

function responseMessage(response: JsonRecord, fallback: string) {
  const message = response.status_message ?? response.statusMessage;
  return typeof message === "string" && message ? message : fallback;
}

function grpcErrorStatus(error: unknown) {
  const code =
    typeof error === "object" && error !== null && "code" in error
      ? (error as { code?: unknown }).code
      : undefined;
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

function rpcFailure(error: unknown) {
  return Response.json(
    { success: false, message: "Website account service is unavailable." },
    { status: grpcErrorStatus(error) }
  );
}

async function routePath(context: { params: Promise<{ path?: string[] }> }) {
  const params = await context.params;
  return (params.path ?? []).join("/");
}

async function parseBody(request: Request) {
  const body: unknown = await request.json();
  if (!body || typeof body !== "object" || Array.isArray(body)) {
    throw new Error("Request body must be an object");
  }
  return body as JsonRecord;
}

function notFound() {
  return Response.json({ success: false, message: "Not found" }, { status: 404 });
}

export async function POST(
  request: Request,
  context: { params: Promise<{ path?: string[] }> }
) {
  const route = await routePath(context);
  if (route !== "register" && route !== "login") return notFound();

  let body: JsonRecord;
  try {
    body = await parseBody(request);
  } catch {
    return Response.json({ success: false, message: "Invalid request body" }, { status: 400 });
  }

  const username = textField(body.username);
  const password = textField(body.password);
  if (!username || !password) {
    return Response.json(
      { success: false, message: "Username and password are required" },
      { status: 400 }
    );
  }

  try {
    if (route === "register") {
      const response = await callUserService(UserRpcMethods.register, { username, password });
      const success = response.success === true;
      const message = responseMessage(response, success ? "Registration successful" : "Registration failed");
      return Response.json(
        { success, message, status_message: message },
        { status: success ? 201 : 400 }
      );
    }

    const response = await callUserService(UserRpcMethods.login, { username, password });
    const success = response.success === true;
    const message = responseMessage(response, success ? "Login successful" : "Login failed");
    return Response.json(
      { success, message, status_message: message, token: textField(response.token) || undefined },
      { status: success ? 200 : 401 }
    );
  } catch (error) {
    return rpcFailure(error);
  }
}

export async function GET(
  request: Request,
  context: { params: Promise<{ path?: string[] }> }
) {
  if ((await routePath(context)) !== "balance") return notFound();

  const authorization = request.headers.get("authorization") ?? "";
  const match = authorization.match(/^Bearer\s+(.+)$/i);
  if (!match) {
    return Response.json({ success: false, message: "Authentication required" }, { status: 401 });
  }

  try {
    const response = await callUserService(UserRpcMethods.balance, {
      username: "",
      token: match[1],
    });
    const success = response.success === true;
    const message = responseMessage(response, success ? "OK" : "Unable to load balance");
    return Response.json(
      {
        success,
        balance: Number(response.balance ?? 0),
        message,
        status_message: message,
      },
      { status: success ? 200 : 401 }
    );
  } catch (error) {
    return rpcFailure(error);
  }
}
