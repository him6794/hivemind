import { expect, test } from '@playwright/test';

const applications = [
  {
    name: 'official site',
    url: process.env.HIVEMIND_SITE_URL || 'http://127.0.0.1:8080',
    heading: /./,
  },
  {
    name: 'Master UI',
    url: process.env.HIVEMIND_MASTER_UI_URL || 'http://127.0.0.1:3000',
    heading: 'Master UI',
  },
  {
    name: 'Worker UI',
    url: process.env.HIVEMIND_WORKER_UI_URL || 'http://127.0.0.1:3001',
    heading: 'Worker UI',
  },
];

test.describe('hosted frontend browser smoke', () => {
  for (const application of applications) {
    test(`${application.name} renders its primary screen`, async ({ page }) => {
      await page.goto(application.url, { waitUntil: 'domcontentloaded' });
      const heading = page.getByRole('heading', { level: 1 }).first();
      await expect(heading).toBeVisible();
      if (typeof application.heading === 'string') {
        await expect(heading).toHaveText(application.heading);
      }
    });
  }
});
