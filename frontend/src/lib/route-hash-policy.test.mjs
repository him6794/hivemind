import assert from "node:assert/strict";
import test from "node:test";
import { parseRouteHash, routeHash } from "./route-hash-policy.mjs";

const ROUTES = [
  "home",
  "login",
  "register",
  "account",
  "forgot-password",
  "forgot-username",
  "verify-email",
  "reset-password",
  "docs",
  "privacy",
];

test("an empty hash opens the home page", () => {
  assert.deepEqual(parseRouteHash("", ROUTES), { route: "home", docsSlug: "introduction", token: "" });
  assert.deepEqual(parseRouteHash("#/", ROUTES), { route: "home", docsSlug: "introduction", token: "" });
});

test("unknown routes are not parsed", () => {
  assert.equal(parseRouteHash("#/dashboard", ROUTES), null);
  assert.equal(parseRouteHash("#/tasks/anything", ROUTES), null);
});

test("documentation keeps its slug and the token is empty", () => {
  assert.deepEqual(parseRouteHash("#/docs/loops", ROUTES), { route: "docs", docsSlug: "loops", token: "" });
  assert.deepEqual(parseRouteHash("#/docs/not-a-page", ROUTES), {
    route: "docs",
    docsSlug: "introduction",
    token: "",
  });
});

test("token routes read the segment after the route name", () => {
  assert.deepEqual(parseRouteHash("#/verify-email/abc123", ROUTES), {
    route: "verify-email",
    docsSlug: "introduction",
    token: "abc123",
  });
  assert.deepEqual(parseRouteHash("#/reset-password/tok-en_9", ROUTES), {
    route: "reset-password",
    docsSlug: "introduction",
    token: "tok-en_9",
  });
});

test("token routes decode percent-encoded tokens, and a broken encoding yields an empty token", () => {
  assert.equal(parseRouteHash("#/verify-email/a%2Bb%3D", ROUTES).token, "a+b=");
  assert.equal(parseRouteHash("#/verify-email/%E0%A4%A", ROUTES).token, "");
  assert.equal(parseRouteHash("#/verify-email", ROUTES).token, "");
});

test("routes other than docs and the token routes ignore a detail segment", () => {
  assert.deepEqual(parseRouteHash("#/forgot-password/extra", ROUTES), {
    route: "forgot-password",
    docsSlug: "introduction",
    token: "",
  });
  assert.deepEqual(parseRouteHash("#/account/extra", ROUTES), {
    route: "account",
    docsSlug: "introduction",
    token: "",
  });
});

test("routeHash builds the hash for each kind of route and round-trips a token", () => {
  assert.equal(routeHash("home", {}), "#/");
  assert.equal(routeHash("login"), "#/login");
  assert.equal(routeHash("docs", { docsSlug: "loops" }), "#/docs/loops");
  assert.equal(routeHash("verify-email", { token: "abc123" }), "#/verify-email/abc123");
  assert.equal(routeHash("verify-email", { token: "" }), "#/verify-email");
  // A route without a token never carries one, even when a stale token is supplied.
  assert.equal(routeHash("login", { token: "abc123" }), "#/login");

  const token = "a+b/c=d";
  const hash = routeHash("reset-password", { token });
  assert.deepEqual(parseRouteHash(hash, ROUTES), {
    route: "reset-password",
    docsSlug: "introduction",
    token,
  });
});
