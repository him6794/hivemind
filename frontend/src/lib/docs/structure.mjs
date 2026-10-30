// Shared layout of the documentation pages. Both locales use these group ids,
// slugs, and order; the text lives in pages-en.mjs and pages-zh.mjs.

export const DOCS_GROUP_IDS = ['start', 'tutorials', 'templates', 'components', 'reference'];

export const DOCS_PAGE_SLUGS = [
  'introduction',
  'installation',
  'first-task',
  'read-input',
  'loops',
  'conditions',
  'functions',
  'collections',
  'text',
  'budget',
  'master-api',
  'templates-numbers',
  'templates-records',
  'templates-text',
  'ui-overview',
  'ui-forms',
  'ui-feedback',
  'ui-overlays',
  'ui-navigation',
  'ui-layout',
  'ui-charts',
  'ui-animations',
  'language',
  'settings',
  'billing',
  'limits',
  'errors',
  'api-reference',
  'platforms',
];

export const DEFAULT_DOCS_SLUG = 'introduction';

export function docsSlugFrom(value) {
  return DOCS_PAGE_SLUGS.includes(value) ? value : DEFAULT_DOCS_SLUG;
}
