// Hash routing for the official site: "#/<route>" or "#/<route>/<detail>". Only the
// documentation route and the token routes read the detail segment. Every other route
// ignores it.
import { DEFAULT_DOCS_SLUG, docsSlugFrom } from "./docs/structure.mjs";

// Routes whose detail segment is a one-time token from an email link.
export const TOKEN_ROUTE_IDS = Object.freeze(["verify-email", "reset-password"]);

function decodeDetail(detail) {
  if (typeof detail !== "string" || detail === "") return "";
  try {
    return decodeURIComponent(detail);
  } catch {
    return "";
  }
}

/**
 * @param {string} hash window.location.hash
 * @param {readonly string[]} routeIds every route the site renders
 * @returns {{ route: string, docsSlug: string, token: string } | null}
 */
export function parseRouteHash(hash, routeIds) {
  const raw = String(hash ?? "").replace(/^#\/?/, "");
  if (!raw) {
    return { route: "home", docsSlug: DEFAULT_DOCS_SLUG, token: "" };
  }
  const [head, detail] = raw.split("/");
  if (!routeIds.includes(head)) {
    return null;
  }
  return {
    route: head,
    docsSlug: head === "docs" ? docsSlugFrom(detail) : DEFAULT_DOCS_SLUG,
    token: TOKEN_ROUTE_IDS.includes(head) ? decodeDetail(detail) : "",
  };
}

/**
 * The hash that shows a route. The token is encoded, so a token that contains reserved
 * characters still round-trips through parseRouteHash.
 */
export function routeHash(route, { docsSlug = DEFAULT_DOCS_SLUG, token = "" } = {}) {
  if (route === "home") return "#/";
  if (route === "docs") return `#/docs/${docsSlug}`;
  if (TOKEN_ROUTE_IDS.includes(route) && token) {
    return `#/${route}/${encodeURIComponent(token)}`;
  }
  return `#/${route}`;
}
