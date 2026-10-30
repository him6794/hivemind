"use client";

import { useEffect } from "react";
import { useParams } from "next/navigation";
import { docsSlugFrom } from "@/lib/docs/structure.mjs";

// The site routes pages with the URL hash. A typed address such as /docs/loops
// moves to the same page in the hash form, so it opens instead of returning 404.
export function DocsAddress() {
  const params = useParams<{ slug?: string[] }>();

  useEffect(() => {
    const slug = docsSlugFrom(params.slug?.[0]);
    window.location.replace(`/#/docs/${slug}`);
  }, [params.slug]);

  return null;
}
