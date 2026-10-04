import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { expect, test } from '@playwright/test';

const frontendDirectory = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const repositoryRoot = path.resolve(frontendDirectory, '..');
const evidenceDirectory = path.resolve(
  process.env.HIVEMIND_E2E_EVIDENCE_DIR
    || path.join(repositoryRoot, 'test_logs', 'frontend-e2e'),
);
const officialSiteUrl = process.env.HIVEMIND_SITE_URL || 'http://127.0.0.1:8080';
const masterUiUrl = process.env.HIVEMIND_MASTER_UI_URL || 'http://127.0.0.1:3000';
const workerUiUrl = process.env.HIVEMIND_WORKER_UI_URL || 'http://127.0.0.1:3001';
const taskSourceCode = 'return "Hello from Hivemind sample task";';
const taskInputJson = 'null';
const runSuffix = Date.now().toString(36);
const username = `qa${runSuffix}`.slice(0, 28);
const password = `HiveQA!${runSuffix}`;
const completedTaskId = `qa-complete-${runSuffix}`;
const cancelledTaskId = `qa-cancel-${runSuffix}`;
const evidenceLogPath = path.join(evidenceDirectory, 'release-flow-actions.txt');

function evidencePath(filename) {
  return path.join(evidenceDirectory, filename);
}

function recordEvidence(message) {
  fs.appendFileSync(evidenceLogPath, `${new Date().toISOString()} ${message}\n`, 'utf8');
}

async function useEnglish(page) {
  await page.goto(`${officialSiteUrl}/#/`);
  await page.getByRole('button', { name: 'Switch language', exact: true }).click();
  await page.getByRole('menuitemradio', { name: 'English', exact: true }).click();
}

test.describe.serial('release browser flow across the official site, Worker console, and Master console', () => {
  test.beforeAll(() => {
    fs.mkdirSync(evidenceDirectory, { recursive: true });
    fs.writeFileSync(
      evidenceLogPath,
      `${new Date().toISOString()} release browser QA started for user ${username}\n`,
      'utf8',
    );
  });

  test('official site validates registration, exposes account state, and signs out cleanly', async ({ page }) => {
    await page.addInitScript(() => {
      window.localStorage.setItem(
        'hivemind-site-auth',
        JSON.stringify({ state: { token: 'legacy-bearer-token', user: { username: 'legacy' } } }),
      );
    });
    await useEnglish(page);
    await expect.poll(() => page.evaluate(() => window.localStorage.getItem('hivemind-site-auth'))).toBeNull();
    await page.goto(`${officialSiteUrl}/#/register`);
    await expect(page.getByRole('heading', {
      level: 1,
      name: 'Create a Hivemind account and get started.',
      exact: true,
    })).toBeVisible();

    await page.getByLabel('Username').fill(username);
    await page.getByLabel('Password', { exact: true }).fill(password);
    await page.getByLabel('Confirm password').fill(`${password}-mismatch`);
    await page.getByRole('button', { name: 'Create account' }).click();
    await expect(page.getByText('Passwords do not match.', { exact: true })).toBeVisible();
    await page.screenshot({
      path: evidencePath('site-registration-validation.png'),
      fullPage: true,
    });
    recordEvidence('PASS official site displayed controlled mismatched-password validation.');

    await page.getByLabel('Confirm password').fill(password);
    await page.getByRole('button', { name: 'Create account' }).click();
    await expect(page).toHaveURL(/#\/account$/);
    await expect(page.getByRole('heading', { level: 1, name: 'Account Center', exact: true })).toBeVisible();
    await expect(page.getByText(username, { exact: true })).toBeVisible();
    await expect(page.getByText('CPT balance', { exact: true })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Open docs', exact: true })).toBeVisible();
    await expect(page.getByText(/^\d+\.\d{2}$/)).toBeVisible();

    const sensitiveStorageKeys = await page.evaluate(() => (
      Object.keys(window.localStorage).filter((key) => /token|auth|session/i.test(key))
    ));
    expect(sensitiveStorageKeys).toEqual([]);
    await page.screenshot({
      path: evidencePath('site-account.png'),
      fullPage: true,
    });
    recordEvidence('PASS official site registered the account, loaded balance, and kept bearer auth out of localStorage.');

    await page.evaluate(() => {
      window.localStorage.setItem(
        'hivemind-site-auth',
        JSON.stringify({ state: { token: 'legacy-bearer-token' } }),
      );
    });
    await page.getByRole('button', { name: 'Sign out', exact: true }).click();
    await expect(page).toHaveURL(/#\/$/);
    await expect.poll(() => page.evaluate(() => window.localStorage.getItem('hivemind-site-auth'))).toBeNull();
    await page.goto(`${officialSiteUrl}/#/login`);
    await page.getByLabel('Username').fill(username);
    await page.getByLabel('Password').fill(`${password}-wrong`);
    await page.getByRole('button', { name: 'Sign in' }).click();
    await expect(page.getByText('Invalid credentials', { exact: true })).toBeVisible();

    await page.getByLabel('Password').fill(password);
    await page.getByRole('button', { name: 'Sign in' }).click();
    await expect(page).toHaveURL(/#\/account$/);
    await expect(page.getByText(username, { exact: true })).toBeVisible();
    recordEvidence('PASS official site rejected bad credentials and accepted the correct login.');
  });

  test('Worker console registers capacity and Master console completes, cancels, inspects, and downloads tasks', async ({ page }) => {
    await page.goto(workerUiUrl);
    await expect(page.getByRole('heading', { name: 'Worker console', level: 1 })).toBeVisible();
    await expect(page.getByRole('heading', { name: 'Sign in to connect' })).toBeVisible();
    await page.getByLabel('Username').fill(username);
    await page.getByLabel('Password').fill(password);
    await page.getByRole('button', { name: 'Sign in and connect' }).click();
    await expect(page.getByText('This computer is ready to receive tasks.', { exact: true })).toBeVisible({ timeout: 45_000 });
    await page.getByRole('tab', { name: 'Computer' }).click();
    const connectionStatus = page.getByRole('status').filter({
      hasText: 'This computer is connected and ready to receive tasks.',
    });
    await expect(connectionStatus).toBeVisible();
    await expect(connectionStatus.getByText('Ready', { exact: true })).toBeVisible();
    await expect(connectionStatus.getByText('Computer ID', { exact: true })).toBeVisible();
    await page.screenshot({
      path: evidencePath('worker-connected.png'),
      fullPage: true,
    });
    recordEvidence('PASS Worker console signed in, checked local capacity, and connected this computer.');

    await page.goto(masterUiUrl);
    await expect(page.getByRole('heading', { name: 'Master console', level: 1 })).toBeVisible();
    await page.getByLabel('Username').fill(username);
    await page.getByLabel('Password').fill(password);
    await page.getByRole('button', { name: 'Sign in' }).click();
    await expect(page.getByText('Signed in and ready', { exact: true })).toBeVisible();

    await page.getByRole('tab', { name: 'Submit' }).click();
    await page.getByLabel('Task ID').fill(cancelledTaskId);
    await page.getByLabel('Task instructions').fill(taskSourceCode);
    await page.getByLabel('Input data (JSON)').fill(taskInputJson);
    await page.getByLabel('CPU score').fill('1201');
    await page.getByLabel('Max CPT').fill('200');
    await page.getByRole('button', { name: 'Send task' }).click();
    await page.getByRole('tab', { name: 'Tasks' }).click();
    const cancelledRow = page.locator('.task-row').filter({ hasText: cancelledTaskId });
    await expect(cancelledRow).toBeVisible();
    await cancelledRow.getByRole('button', { name: 'Cancel', exact: true }).click();
    const cancelDialog = page.getByRole('alertdialog');
    await expect(cancelDialog).toBeVisible();
    await expect(cancelDialog).toContainText(cancelledTaskId);
    await expect(cancelDialog.getByRole('heading', { name: 'Cancel this task?' })).toBeVisible();
    await cancelDialog.getByRole('button', { name: 'Cancel task', exact: true }).click();
    await expect(page.getByText(`Task cancelled: ${cancelledTaskId}`, { exact: true })).toBeVisible();
    await expect(cancelledRow.getByText('CANCELLED', { exact: true })).toBeVisible();
    recordEvidence('PASS Master console submitted and cancelled an unschedulable task.');

    await page.getByRole('tab', { name: 'Submit' }).click();
    await page.getByLabel('Task ID').fill(completedTaskId);
    await page.getByLabel('Task instructions').fill(taskSourceCode);
    await page.getByLabel('Input data (JSON)').fill(taskInputJson);
    await page.getByLabel('CPU score').fill('0');
    await page.getByLabel('Max CPT').fill('100');
    await page.getByRole('button', { name: 'Send task' }).click();
    await page.getByRole('tab', { name: 'Tasks' }).click();
    const completedRow = page.locator('.task-row').filter({ hasText: completedTaskId });
    await expect(completedRow).toBeVisible();
    await expect(completedRow.getByText('COMPLETED', { exact: true })).toBeVisible({ timeout: 120_000 });

    await completedRow.getByRole('button', { name: 'Log' }).click();
    await expect(page.locator('pre').filter({ hasText: 'Hello from Hivemind sample task' })).toBeVisible();
    await expect(completedRow.getByText('Output in Log', { exact: true })).toBeVisible();

    const downloadPromise = page.waitForEvent('download');
    await completedRow.getByRole('button', { name: 'Download' }).click();
    const download = await downloadPromise;
    const suggestedFilename = download.suggestedFilename();
    expect(path.basename(suggestedFilename)).toBe(suggestedFilename);
    expect(suggestedFilename).toMatch(/^[A-Za-z0-9][A-Za-z0-9._-]*$/);
    expect(suggestedFilename).toContain(completedTaskId);
    const downloadedArtifactPath = evidencePath('master-downloaded-artifact.txt');
    await download.saveAs(downloadedArtifactPath);
    expect(fs.readFileSync(downloadedArtifactPath, 'utf8')).toContain('Hello from Hivemind sample task');

    await page.screenshot({
      path: evidencePath('master-completed-task.png'),
      fullPage: true,
    });
    recordEvidence(`PASS Master console completed a task, loaded the task log, and downloaded safe artifact '${suggestedFilename}'.`);

    await cancelledRow.getByRole('button', { name: 'Download' }).click();
    await expect(page.getByText('Download failed: Artifact not found', { exact: true })).toBeVisible();
    await page.screenshot({
      path: evidencePath('master-missing-artifact.png'),
      fullPage: true,
    });
    recordEvidence('PASS Master console surfaced a controlled missing-artifact failure without stale content.');
  });
});
