import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { defineConfig } from '@playwright/test';

const directory = path.dirname(fileURLToPath(import.meta.url));
const evidence = path.resolve(
  process.env.HIVEMIND_E2E_EVIDENCE_DIR || path.join(directory, '..', 'test_logs', 'frontend-ui'),
);
const surface = process.env.HIVEMIND_UI_SURFACE;
if (surface && !['master', 'worker', 'website'].includes(surface)) {
  throw new Error('HIVEMIND_UI_SURFACE must be master, worker, or website');
}

export default defineConfig({
  testDir: './e2e',
  testMatch: surface ? `shadcn-${surface}.spec.mjs` : 'shadcn-*.spec.mjs',
  timeout: 60_000,
  expect: { timeout: 10_000 },
  fullyParallel: false,
  workers: 1,
  outputDir: path.join(evidence, surface ? `${surface}-test-results` : 'test-results'),
  reporter: [['list'], ['json', { outputFile: path.join(evidence, surface ? `${surface}-results.json` : 'results.json') }]],
  use: {
    viewport: { width: 1440, height: 1000 },
    screenshot: 'only-on-failure',
    trace: 'retain-on-failure',
    actionTimeout: 10_000,
  },
  webServer: [
    {
      role: 'master',
      command: 'node node_modules/vite/bin/vite.js preview --host 127.0.0.1 --port 4173 --strictPort',
      cwd: path.join(directory, 'master-ui'),
      url: 'http://127.0.0.1:4173',
      reuseExistingServer: true,
    },
    {
      role: 'worker',
      command: 'node node_modules/vite/bin/vite.js preview --host 127.0.0.1 --port 4174 --strictPort',
      cwd: path.join(directory, 'worker-ui'),
      url: 'http://127.0.0.1:4174',
      reuseExistingServer: true,
    },
    {
      role: 'website',
      command: 'node node_modules/next/dist/bin/next start --hostname 127.0.0.1 --port 4175',
      cwd: directory,
      url: 'http://127.0.0.1:4175',
      reuseExistingServer: true,
      timeout: 60_000,
    },
  ]
    .filter(({ role }) => !surface || role === surface)
    .map(({ role, ...server }) => server),
});
