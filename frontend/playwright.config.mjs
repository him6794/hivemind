import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { defineConfig } from '@playwright/test';

const frontendDirectory = path.dirname(fileURLToPath(import.meta.url));
const evidenceDirectory = path.resolve(
  process.env.HIVEMIND_E2E_EVIDENCE_DIR
    || path.join(frontendDirectory, '..', '.omo', 'evidence', 'task-8-release-grade-frontends-app-and-site'),
);

function installedBrowserChannel() {
  if (process.env.HIVEMIND_PLAYWRIGHT_CHANNEL) {
    return process.env.HIVEMIND_PLAYWRIGHT_CHANNEL;
  }
  if (process.platform !== 'win32') {
    return undefined;
  }

  const candidates = [
    {
      channel: 'msedge',
      executable: path.join(process.env['PROGRAMFILES(X86)'] || '', 'Microsoft', 'Edge', 'Application', 'msedge.exe'),
    },
    {
      channel: 'chrome',
      executable: path.join(process.env.PROGRAMFILES || '', 'Google', 'Chrome', 'Application', 'chrome.exe'),
    },
  ];
  return candidates.find(({ executable }) => executable && fs.existsSync(executable))?.channel;
}

const channel = installedBrowserChannel();

export default defineConfig({
  testDir: './e2e',
  fullyParallel: false,
  workers: 1,
  timeout: 180_000,
  expect: {
    timeout: 15_000,
  },
  outputDir: path.join(evidenceDirectory, 'test-results'),
  reporter: [
    ['list'],
    ['json', { outputFile: path.join(evidenceDirectory, 'playwright-results.json') }],
  ],
  use: {
    ...(channel ? { channel } : {}),
    acceptDownloads: true,
    actionTimeout: 15_000,
    navigationTimeout: 30_000,
    screenshot: 'only-on-failure',
    trace: 'retain-on-failure',
    video: 'retain-on-failure',
    viewport: { width: 1440, height: 1000 },
  },
});
