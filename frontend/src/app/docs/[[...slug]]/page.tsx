import { DOCS_PAGE_SLUGS } from "@/lib/docs/structure.mjs";
import { DocsAddress } from "./docs-address";

// A static export needs every docs address listed at build time. The hash redirect
// runs in the browser, so one shell per known page is enough.
export function generateStaticParams() {
  return [{ slug: [] }, ...DOCS_PAGE_SLUGS.map((slug) => ({ slug: [slug] }))];
}

export default function DocsAddressPage() {
  return <DocsAddress />;
}
