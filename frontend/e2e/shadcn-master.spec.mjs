import { expect, test } from '@playwright/test';
import { waitForStableDisplay } from './shadcn-display.mjs';

const masterUiUrl = process.env.HIVEMIND_MASTER_UI_URL || 'http://127.0.0.1:4173';
const existingTaskId = 'browser-open-task';
const submittedTaskId = 'browser-submitted-task';
const sessionKey = 'hivemind.master.session.v1';
const themeKey = 'hivemind.master.theme.v1';
const browserDiagnostics = new WeakMap();

function createTask(taskId, overrides = {}) {
  return {
    task_id: taskId,
    status: 'RUNNING',
    status_message: 'Running the browser fixture task',
    runtime: 'managed-function-v1',
    wall_time_ms: 1250,
    worker_id: 'fixture-worker-01',
    provider_user: 'fixture-provider',
    dispatch_status: 'EXECUTING',
    usage_units: 3.5,
    max_cpt: 25,
    billed_amount: 0,
    billing_settled: false,
    retry_count: 0,
    ...overrides,
  };
}

function addBrowserDiagnostics(page) {
  const diagnostics = {
    consoleErrors: [],
    pageErrors: [],
    failedRequests: [],
    badResponses: [],
  };
  browserDiagnostics.set(page, diagnostics);
  page.on('console', (message) => {
    if (message.type() === 'error') diagnostics.consoleErrors.push(message.text());
  });
  page.on('pageerror', (error) => diagnostics.pageErrors.push(error.message));
  page.on('requestfailed', (request) => {
    diagnostics.failedRequests.push(`${request.method()} ${request.url()}: ${request.failure()?.errorText || 'request failed'}`);
  });
  page.on('response', (response) => {
    if (response.status() >= 400) {
      diagnostics.badResponses.push(`${response.status()} ${response.request().method()} ${response.url()}`);
    }
  });
}

async function installMockApi(page, { startupFailure = false } = {}) {
  const fixture = {
    startupCalls: 0,
    stopCalls: 0,
    submissions: [],
    logsRead: [],
    unhandledRequests: [],
    tasks: [createTask(existingTaskId)],
  };

  const corsHeaders = {
    'access-control-allow-origin': '*',
    'access-control-allow-methods': 'GET, POST, OPTIONS',
    'access-control-allow-headers': 'authorization,content-type',
  };

  await page.route('**/api/**', async (route) => {
    const request = route.request();
    const { pathname } = new URL(request.url());
    const method = request.method();

    if (method === 'OPTIONS') {
      await route.fulfill({ status: 204, headers: corsHeaders });
      return;
    }

    const fulfillJson = (payload, status = 200) => route.fulfill({
      status,
      json: payload,
      headers: corsHeaders,
    });

    if (method === 'GET' && pathname === '/api/startup-status') {
      fixture.startupCalls += 1;
      if (startupFailure) {
        if (fixture.startupCalls === 1) {
          await new Promise((resolve) => setTimeout(resolve, 250));
          await fulfillJson({ state: 'initializing', phase: 'resources' });
        } else {
          await fulfillJson({ state: 'failed', phase: 'services', code: 'fixture_startup_failure' });
        }
        return;
      }
      await fulfillJson({ state: 'ready', phase: 'ready' });
      return;
    }

    if (method === 'POST' && pathname === '/api/login') {
      await fulfillJson({ success: true, token: 'browser-fixture-token' });
      return;
    }

    if (method === 'POST' && pathname === '/api/vpn/bootstrap') {
      await fulfillJson({ success: true, state: 'ready' });
      return;
    }

    if (method === 'GET' && pathname === '/api/tasks') {
      await fulfillJson({ success: true, tasks: fixture.tasks });
      return;
    }

    if (method === 'GET' && pathname === '/api/balance') {
      await fulfillJson({ success: true, balance: '12.50' });
      return;
    }

    if (method === 'POST' && pathname === '/api/tasks') {
      const payload = request.postDataJSON();
      fixture.submissions.push(payload);
      fixture.tasks.push(createTask(payload.task_id, {
        runtime: payload.runtime,
        status: 'QUEUED',
        status_message: 'Accepted by the browser fixture',
        wall_time_ms: 0,
        max_cpt: payload.max_cpt,
      }));
      await fulfillJson({ success: true, task_id: payload.task_id });
      return;
    }

    const logMatch = pathname.match(/^\/api\/tasks\/([^/]+)\/log$/);
    if (method === 'GET' && logMatch) {
      const taskId = decodeURIComponent(logMatch[1]);
      fixture.logsRead.push(taskId);
      await fulfillJson({ success: true, log: 'Browser fixture output: managed result 42' });
      return;
    }

    const stopMatch = pathname.match(/^\/api\/tasks\/([^/]+)\/stop$/);
    if (method === 'POST' && stopMatch) {
      const taskId = decodeURIComponent(stopMatch[1]);
      fixture.stopCalls += 1;
      const task = fixture.tasks.find((candidate) => candidate.task_id === taskId);
      if (task) {
        task.status = 'CANCELLED';
        task.status_message = 'Stopped by the browser fixture';
      }
      await fulfillJson({ success: true, task_id: taskId });
      return;
    }

    fixture.unhandledRequests.push(`${method} ${pathname}`);
    await fulfillJson({ success: false, message: `Unhandled browser fixture request: ${method} ${pathname}` }, 500);
  });

  return fixture;
}

async function signIn(page) {
  await page.goto(masterUiUrl);
  await expect(page.getByRole('heading', { level: 1, name: 'Master console' })).toBeVisible();
  await page.getByLabel('Username').fill('browser-qa');
  await page.getByLabel('Password').fill('fixture-password');
  await page.getByRole('button', { name: 'Sign in', exact: true }).click();
  await expect(page.getByText('Signed in and ready', { exact: true })).toBeVisible();
  await expect(page.getByRole('tab', { name: 'Tasks' })).toHaveAttribute('aria-selected', 'true');
}

async function assertNoHorizontalOverflow(page) {
  const overflow = await page.evaluate(() => ({
    document: document.documentElement.scrollWidth > window.innerWidth,
    body: document.body.scrollWidth > window.innerWidth,
  }));
  expect(overflow, `horizontal overflow at ${await page.evaluate(() => window.innerWidth)}px`).toEqual({
    document: false,
    body: false,
  });
}

async function attachScreenshot(page, testInfo, name) {
  await waitForStableDisplay(page);
  await testInfo.attach(name, {
    body: await page.screenshot({ fullPage: true }),
    contentType: 'image/png',
  });
}

test.beforeEach(async ({ page }) => {
  addBrowserDiagnostics(page);
});

test.afterEach(async ({ page }, testInfo) => {
  const diagnostics = browserDiagnostics.get(page);
  if (!diagnostics) return;
  await testInfo.attach('browser-diagnostics.json', {
    body: Buffer.from(JSON.stringify(diagnostics, null, 2)),
    contentType: 'application/json',
  });
  expect(diagnostics.consoleErrors, 'browser console errors').toEqual([]);
  expect(diagnostics.pageErrors, 'uncaught page errors').toEqual([]);
  expect(diagnostics.failedRequests, 'failed network requests').toEqual([]);
  expect(diagnostics.badResponses, 'HTTP resource/API failures').toEqual([]);
});

test.describe('production Master shadcn UI regression', () => {
  test('shows startup loading then a controlled backend startup failure', async ({ page }, testInfo) => {
    const fixture = await installMockApi(page, { startupFailure: true });
    await page.goto(masterUiUrl);

    await expect(page.locator('.startup-screen .startup-message')).toHaveText('Preparing the console');
    await expect(page.locator('.startup-screen .startup-message')).toHaveText(
      'Hivemind could not start (fixture_startup_failure). Close and reopen the application.',
      { timeout: 10_000 },
    );
    expect(fixture.startupCalls).toBeGreaterThanOrEqual(2);
    await expect(page.locator('img')).toHaveCount(0);
    await attachScreenshot(page, testInfo, 'master-startup-failure.png');
  });

  test('supports keyboard tabs, retained drafts, task output, explicit cancel confirmation, and submission', async ({ page }, testInfo) => {
    const fixture = await installMockApi(page);
    await page.setViewportSize({ width: 1440, height: 1000 });
    await signIn(page);

    const storageState = await page.evaluate((key) => ({
      localKeys: Object.keys(window.localStorage),
      sensitiveLocalKeys: Object.keys(window.localStorage).filter((name) => /token|auth|session/i.test(name)),
      session: JSON.parse(window.sessionStorage.getItem(key) || '{}'),
    }), sessionKey);
    expect(storageState.localKeys).toContain(themeKey);
    expect(storageState.sensitiveLocalKeys).toEqual([]);
    expect(storageState.session.token).toBe('browser-fixture-token');

    const tasksTab = page.getByRole('tab', { name: 'Tasks' });
    const submitTab = page.getByRole('tab', { name: 'Submit' });
    await tasksTab.focus();
    await tasksTab.press('ArrowRight');
    await expect(submitTab).toHaveAttribute('aria-selected', 'true');
    await page.getByLabel('Task instructions').fill('Draft instructions retained across tabs');
    await page.getByLabel('Input data (JSON)').fill('{"draft":true}');
    await submitTab.press('ArrowLeft');
    await expect(tasksTab).toHaveAttribute('aria-selected', 'true');
    await tasksTab.press('ArrowRight');
    await expect(submitTab).toHaveAttribute('aria-selected', 'true');
    await expect(page.getByLabel('Task instructions')).toHaveValue('Draft instructions retained across tabs');
    await expect(page.getByLabel('Input data (JSON)')).toHaveValue('{"draft":true}');

    await submitTab.press('ArrowLeft');
    const taskRow = page.locator('.task-list > li').filter({ hasText: existingTaskId });
    await expect(taskRow).toBeVisible();
    await taskRow.getByRole('button', { name: 'Log' }).click();
    await expect(page.locator('.detail-pane[aria-label="Task log"] pre'))
      .toHaveText('Browser fixture output: managed result 42');
    expect(fixture.logsRead).toEqual([existingTaskId]);

    await taskRow.getByRole('button', { name: 'Cancel', exact: true }).click();
    const cancelDialog = page.getByRole('alertdialog');
    await expect(cancelDialog).toBeVisible();
    await expect(cancelDialog).toContainText(existingTaskId);
    await page.keyboard.press('Escape');
    await expect(cancelDialog).toBeHidden();
    expect(fixture.stopCalls, 'dismissing cancellation must not send a stop request').toBe(0);

    await taskRow.getByRole('button', { name: 'Cancel', exact: true }).click();
    await page.getByRole('button', { name: 'Cancel task', exact: true }).click();
    await expect.poll(() => fixture.stopCalls).toBe(1);
    await expect(taskRow.getByText('CANCELLED', { exact: true })).toBeVisible();
    expect(fixture.stopCalls, 'explicit confirmation sends exactly one stop request').toBe(1);

    await tasksTab.press('ArrowRight');
    await expect(submitTab).toHaveAttribute('aria-selected', 'true');
    await page.getByLabel('Task ID').fill(submittedTaskId);
    await page.getByLabel('Task instructions').fill('return input.value;');
    await page.getByLabel('Input data (JSON)').fill('{"value":42}');
    await page.getByLabel('Task charge cap (CPT, fee included)').fill('12');
    await page.getByRole('button', { name: 'Send task', exact: true }).click();
    await expect(page.getByText(`Task submitted: ${submittedTaskId}`, { exact: true })).toBeVisible();
    expect(fixture.submissions).toHaveLength(1);
    expect(fixture.submissions[0]).toMatchObject({
      task_id: submittedTaskId,
      runtime: 'managed-function-v1',
      task_source: 'return input.value;',
      torrent: '{"value":42}',
      max_cpt: 12,
    });

    await submitTab.press('ArrowLeft');
    const submittedRow = page.locator('.task-list > li').filter({ hasText: submittedTaskId });
    await expect(submittedRow).toBeVisible();
    await expect(submittedRow.getByText('QUEUED')).toBeVisible();

    await assertNoHorizontalOverflow(page);
    await expect(page.locator('img')).toHaveCount(0);
    await attachScreenshot(page, testInfo, 'master-desktop-light.png');

    await page.getByRole('button', { name: 'Switch to dark theme' }).click();
    await expect.poll(() => page.evaluate(() => document.documentElement.classList.contains('dark'))).toBe(true);
    await assertNoHorizontalOverflow(page);
    await attachScreenshot(page, testInfo, 'master-desktop-dark.png');
  });

  test('fits a 320px viewport in light and dark themes', async ({ page }, testInfo) => {
    await installMockApi(page);
    await signIn(page);
    await page.setViewportSize({ width: 320, height: 850 });
    await expect.poll(() => page.evaluate(() => window.innerWidth)).toBe(320);
    await expect.poll(() => page.evaluate(() => document.documentElement.classList.contains('dark'))).toBe(false);
    await assertNoHorizontalOverflow(page);
    await expect(page.locator('img')).toHaveCount(0);
    await attachScreenshot(page, testInfo, 'master-mobile-320-light.png');

    await page.getByRole('button', { name: 'Switch to dark theme' }).click();
    await expect.poll(() => page.evaluate(() => document.documentElement.classList.contains('dark'))).toBe(true);
    await assertNoHorizontalOverflow(page);
    await attachScreenshot(page, testInfo, 'master-mobile-320-dark.png');
  });
});
