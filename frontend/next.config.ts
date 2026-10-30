import type { NextConfig } from "next";

// The site is a static export served by Cloudflare Pages. Response headers are set
// in public/_headers, because Next.js does not apply headers() to a static export.
// The account API runs in bff/ on the website backend host.
const nextConfig: NextConfig = {
  output: "export",
  reactStrictMode: true,
};

export default nextConfig;
