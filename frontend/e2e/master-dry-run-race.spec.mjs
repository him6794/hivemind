import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { expect, test } from '@playwright/test';
import { createServer } from '../master-ui/node_modules/vite/dist/node/index.js';

// Runs the real React form with a delayed HTTP boundary, not a replacement
// component. The private ephemeral Vite server never contacts a real Master.
const root = fileURLToPath(new URL('../master-ui/', import.meta.url));
let server;
let uiUrl;

test.beforeAll(async () => {
  server = await createServer({
    root,
    configFile: path.join(root, 'vite.config.js'),
    define: { 'import.meta.env.VITE_API_BASE': '""' },
    server: { host: '127.0.0.1', port: 0 },
    logLevel: 'error',
  });
  await server.listen();
  uiUrl = `http://127.0.0.1:${server.httpServer.address().port}`;
});

test.afterAll(async () => {
  await server?.close();
});

function deferred() {
  let resolve;
  const promise = new Promise((done) => { resolve = done; });
  return { promise, resolve };
}

const quote = {
  success: true,
  message: 'The run completed',
  output: '60',
  output_bytes: 2,
  output_truncated: false,
  exceeds_consensus_result_limit: false,
  consensus_result_limit_bytes: 262144,
  usage_units: 25,
  quote: {
    replica_usage_units: 25,
    replica_count: 3,
    replica_count_source: 'default',
    reserved_usage_cpt: 75,
    platform_fee_cpt: 7,
    required_max_cpt: 82,
  },
  error: null,
};

const cases = [
  { name: 'preserves a cap entered while waiting', edit: async (page) => {
    await page.locator('#max-cpt').fill('20');
  }, expectedCap: '20', quoteVisible: true },
  { name: 'ignores a reply for edited source', edit: async (page) => {
    await page.locator('#task-source').fill('return 600;');
  }, expectedCap: '0', quoteVisible: false },
  { name: 'ignores a reply for edited input', edit: async (page) => {
    await page.locator('#task-input').fill('{"changed":true}');
  }, expectedCap: '0', quoteVisible: false },
  { name: 'ignores a reply for edited resources', edit: async (page) => {
    await page.getByText('Advanced: computer requirements', { exact: true }).click();
    await page.locator('#cpu-score').fill('500');
  }, expectedCap: '0', quoteVisible: false },
  { name: 'fills a cap cleared while waiting', initialCap: '150', edit: async (page) => {
    await page.locator('#max-cpt').fill('0');
  }, expectedCap: '82', quoteVisible: true },
  { name: 'still fills an untouched empty cap', edit: async () => {},
    expectedCap: '82', quoteVisible: true },
];

for (const delayedPath of ['/api/tasks/dry-run', '/api/tasks/quote']) {
  for (const scenario of cases) {
    test(`${scenario.name} during ${delayedPath}`, async ({ page }) => {
      const requested = deferred();
      const released = deferred();
      const unexpected = [];
      await page.route('**/api/**', async (route) => {
        const pathname = new URL(route.request().url()).pathname;
        if (pathname === delayedPath) {
          requested.resolve();
          await released.promise;
        }
        const responses = {
          '/api/startup-status': { state: 'ready', phase: 'ready' },
          '/api/login': { success: true, token: 'fixture-token' },
          '/api/vpn/bootstrap': { success: true, state: 'ready' },
          '/api/tasks': { success: true, tasks: [] },
          '/api/balance': { success: true, balance: '10000' },
          '/api/tasks/quote': { success: true, quoted_cpt: 10 },
          '/api/tasks/dry-run': quote,
        };
        if (!(pathname in responses)) unexpected.push(pathname);
        await route.fulfill({ status: pathname in responses ? 200 : 404,
          json: responses[pathname] ?? { success: false } });
      });

      try {
        await page.goto(uiUrl, { waitUntil: 'domcontentloaded' });
        await page.getByLabel('Username', { exact: true }).fill('test-user');
        await page.getByLabel('Password', { exact: true }).fill('test-only');
        await page.getByRole('button', { name: 'Sign in', exact: true }).click();
        await expect(page.getByText('Signed in and ready', { exact: true })).toBeVisible();
        await page.getByRole('tab', { name: 'Submit', exact: true }).click();
        await page.locator('#task-source').fill('return 60;');
        await page.locator('#task-input').fill('{}');
        await page.locator('#max-cpt').fill(scenario.initialCap ?? '0');
        await page.getByRole('button', { name: 'Check and price', exact: true }).click();
        await requested.promise;
        await scenario.edit(page);
        released.resolve();
        await expect(page.getByRole('button', { name: 'Check and price', exact: true })).toBeEnabled();
        await expect(page.locator('#max-cpt')).toHaveValue(scenario.expectedCap);
        await expect(page.locator('.dry-run-result')).toHaveCount(scenario.quoteVisible ? 1 : 0);
        expect(unexpected).toEqual([]);
      } finally {
        released.resolve();
      }
    });
  }
}
