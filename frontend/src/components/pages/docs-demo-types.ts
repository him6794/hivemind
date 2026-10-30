import type { ReactElement } from "react";

// One live example on a documentation page. `code` is the snippet shown below the
// preview; it is written out by hand so the page can show it without a build step.
export type DemoEntry = {
  title: { en: string; zh: string };
  code: string;
  Preview: () => ReactElement;
};
