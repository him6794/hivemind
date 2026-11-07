import type { NextConfig } from "next";
import { PHASE_DEVELOPMENT_SERVER } from "next/constants";

// The site is a static export served by Cloudflare Pages. Response headers are set
// in public/_headers, because Next.js does not apply headers() to a static export.
// The account API runs in bff/ on the website backend host.
//
// Local development can send /api/* to a website backend on this machine. Set
// HIVEMIND_LOCAL_API to any value when starting `next dev`. Rewrites are only added in the
// development server, so `next build` output does not change.
const LOCAL_API_ORIGIN = "http://127.0.0.1:3100";

export default function nextConfig(phase: string): NextConfig {
  const localApi =
    phase === PHASE_DEVELOPMENT_SERVER && process.env.HIVEMIND_LOCAL_API
      ? {
          rewrites: async () => [
            { source: "/api/:path*", destination: `${LOCAL_API_ORIGIN}/api/:path*` },
          ],
        }
      : {};

  return {
    output: "export",
    reactStrictMode: true,
    ...localApi,
  };
}
