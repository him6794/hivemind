import { expect, test } from '@playwright/test';
import { waitForStableDisplay } from './shadcn-display.mjs';
import { nativeLocaleContract } from './native-locale-contract.mjs';

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
    runtime: 'hcode',
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

// What the Master answers for a function that finished: 25 units per replica, so 82 CPT in all.
function dryRunQuote() {
  return {
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
    semantics_manifest_sha256: 'f7593109a627205b49172b9cf38251bc618a4a03d2885829280449390e2dee2e',
  };
}

function dryRunSyntaxError() {
  return {
    success: false,
    message: 'The program failed',
    output: '',
    output_bytes: 0,
    output_truncated: false,
    exceeds_consensus_result_limit: false,
    consensus_result_limit_bytes: 262144,
    usage_units: null,
    quote: null,
    error: { code: 'parse_error', message: 'unexpected token', line: 2, column: 9, cause: 'program', usage_units: null },
    semantics_manifest_sha256: 'f7593109a627205b49172b9cf38251bc618a4a03d2885829280449390e2dee2e',
  };
}

// The Nodepool price of the resources a task reserves: 10 CPT per host plus CPU, memory, and so on.
function resourcePrice(request) {
  const hosts = Math.max(1, request.host_count || 1);
  return hosts * (10
    + Math.floor((request.cpu_score || 0) / 10)
    + Math.floor((request.gpu_score || 0) / 5)
    + (request.memory_gb || 0) * 2
    + (request.gpu_memory_gb || 0) * 3
    + (request.storage_gb || 0));
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
    loginCalls: 0,
    bootstrapCalls: 0,
    log: 'Browser fixture output: managed result 42',
    stopCalls: 0,
    submissions: [],
    dryRuns: [],
    quotes: [],
    dryRunAnswer: dryRunQuote,
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
      fixture.loginCalls += 1;
      await fulfillJson({ success: true, token: 'browser-fixture-token' });
      return;
    }

    if (method === 'POST' && pathname === '/api/vpn/bootstrap') {
      fixture.bootstrapCalls += 1;
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

    if (method === 'POST' && pathname === '/api/tasks/dry-run') {
      const payload = request.postDataJSON();
      fixture.dryRuns.push(payload);
      await fulfillJson(fixture.dryRunAnswer(payload));
      return;
    }

    if (method === 'POST' && pathname === '/api/tasks/quote') {
      const payload = request.postDataJSON();
      fixture.quotes.push(payload);
      await fulfillJson({ success: true, quoted_cpt: resourcePrice(payload), currency: 'CPT' });
      return;
    }

    const logMatch = pathname.match(/^\/api\/tasks\/([^/]+)\/log$/);
    if (method === 'GET' && logMatch) {
      const taskId = decodeURIComponent(logMatch[1]);
      fixture.logsRead.push(taskId);
      await fulfillJson({ success: true, log: fixture.log });
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
  nativeLocaleContract({ role: 'master', url: masterUiUrl, installMockApi });

  test('starts from a template, prices it with a dry run, and covers the price of the resources', async ({ page }) => {
    const fixture = await installMockApi(page);
    await signIn(page);
    await page.getByRole('tab', { name: 'Submit' }).click();

    await page.getByLabel('Start from a template').selectOption('sum-list');
    await expect(page.getByLabel('Function source (hcode)')).toHaveValue(/^let total = 0;/);
    await expect(page.getByLabel('Input data (JSON)')).toHaveValue('{"values": [10, 20, 30]}');
    await expect(page.getByLabel('CPU score')).toBeHidden();

    await page.getByRole('button', { name: 'Check and price' }).click();
    await expect(page.getByText('Each replica uses 25 units. 3 replicas use 75 CPT, plus a 7 CPT platform fee: 82 CPT in total.')).toBeVisible();
    const cap = page.getByLabel('Task charge cap (CPT, fee included)');
    await expect(cap).toHaveValue('82');
    expect(fixture.dryRuns).toHaveLength(1);
    expect(fixture.dryRuns[0].torrent).toBe('{"values": [10, 20, 30]}');
    expect(fixture.dryRuns[0].task_source).toMatch(/^let total = 0;/);
    expect(fixture.quotes).toHaveLength(1);
    expect(fixture.quotes[0]).toMatchObject({ runtime: 'hcode', max_cpt: 1 });

    // Resources that cost more than the work change the cap the task needs, and hide the old answer.
    await page.getByText('Advanced: computer requirements').click();
    await page.getByLabel('CPU score').fill('250');
    await page.getByLabel('Host count').fill('3');
    await expect(page.getByText('Output preview')).toBeHidden();
    await page.getByRole('button', { name: 'Check and price' }).click();
    await expect(page.getByText('The computer requirements you chose are priced at 105 CPT')).toBeVisible();
    await expect(page.getByText('Your charge cap of 82 CPT is below the 105 CPT price of the computer requirements you chose')).toBeVisible();
    expect(fixture.quotes[1]).toMatchObject({ cpu_score: 250, host_count: 3 });
    await page.getByRole('button', { name: 'Use 105 CPT as the charge cap' }).click();
    await expect(cap).toHaveValue('105');

    await page.getByLabel('Task ID').fill(submittedTaskId);
    await page.getByRole('button', { name: 'Send task', exact: true }).click();
    await expect(page.getByText(`Task submitted: ${submittedTaskId}`, { exact: true })).toBeVisible();
    expect(fixture.submissions).toHaveLength(1);
    expect(fixture.submissions[0]).toMatchObject({ runtime: 'hcode', max_cpt: 105, cpu_score: 250, host_count: 3 });
    expect(fixture.unhandledRequests).toEqual([]);
  });

  test('shows a syntax error with its line and column, and refuses a dry run without an input', async ({ page }) => {
    const fixture = await installMockApi(page);
    fixture.dryRunAnswer = dryRunSyntaxError;
    await signIn(page);
    await page.getByRole('tab', { name: 'Submit' }).click();

    const check = page.getByRole('button', { name: 'Check and price' });
    await page.getByLabel('Function source (hcode)').fill('let x = 1;\nlet y = ;');
    await expect(check).toBeDisabled();
    await page.getByLabel('Input data (JSON)').fill('{}');
    await check.click();
    await expect(page.getByText('The function failed: parse_error: unexpected token (line 2, column 9)')).toBeVisible();
    await expect(page.getByLabel('Task charge cap (CPT, fee included)')).toHaveValue('0');
    expect(fixture.quotes).toEqual([]);

    await page.getByLabel('Function source (hcode)').fill('let y = 2;');
    await expect(page.getByText('The function failed')).toBeHidden();
    expect(fixture.unhandledRequests).toEqual([]);
  });

  test('shows the output of a finished task and saves all of it', async ({ page }) => {
    const fixture = await installMockApi(page);
    const longOutput = Array.from({ length: 400 }, (_, index) => `line ${index}`).join('\n');
    fixture.tasks.push(createTask('browser-done-task', {
      status: 'COMPLETED',
      status_message: 'Completed',
      output: longOutput,
      billed_amount: 82,
      billing_settled: true,
    }));
    await signIn(page);

    const row = page.locator('.task-list > li').filter({ hasText: 'browser-done-task' });
    await expect(row.locator('.task-output-block pre')).toContainText('line 0');
    await expect(row.locator('.task-output-block pre')).not.toContainText('line 399');
    await expect(row.getByText(`Showing the first 1000 of ${longOutput.length} characters.`, { exact: false })).toBeVisible();
    // A running task has no output yet, so it still points to the log.
    await expect(page.locator('.task-list > li').filter({ hasText: existingTaskId }).getByText('Output in Log')).toBeVisible();

    const [download] = await Promise.all([
      page.waitForEvent('download'),
      row.getByRole('button', { name: 'Save output' }).click(),
    ]);
    expect(download.suggestedFilename()).toBe('browser-done-task-output.txt');
    const saved = await (await import('node:fs/promises')).readFile(await download.path(), 'utf8');
    expect(saved).toBe(longOutput);
  });

  test('switches to Traditional Chinese without resetting drafts, sessions or raw task output', async ({ page }) => {
    const fixture = await installMockApi(page);
    fixture.log = 'Sign in';
    fixture.tasks[0].status_message = 'Sign in';
    fixture.tasks[0].usage_units = 0;
    fixture.tasks.push(createTask('unknown-usage', { usage_units: null }));
    await page.goto(masterUiUrl);
    await page.getByLabel('Username').fill('locale-qa');
    await page.getByLabel('Password').fill('fixture-password');
    await page.getByRole('combobox', { name: 'Language' }).selectOption('zh');
    await expect(page.locator('html')).toHaveAttribute('lang', 'zh-TW');
    await expect(page.getByLabel('使用者名稱')).toHaveValue('locale-qa');
    await expect(page.getByLabel('密碼')).toHaveValue('fixture-password');
    await page.getByRole('button', { name: '登入', exact: true }).click();
    await expect(page.getByText('已登入並準備就緒', { exact: true })).toBeVisible();
    const row = page.locator('.task-list > li').filter({ hasText: existingTaskId });
    await expect(row.locator('.task-message')).toHaveText('Sign in');
    await expect(row).toContainText('0 CPT（未含平台費）');
    await expect(page.locator('.task-list > li').filter({ hasText: 'unknown-usage' })).toContainText('未知');
    await row.getByRole('button', { name: '日誌', exact: true }).click();
    await expect(page.locator('.detail-pane[aria-label="任務日誌"] pre')).toHaveText('Sign in');
    await page.getByRole('tab', { name: '提交', exact: true }).click();
    const source = 'get(input, "text");';
    const input = '{"text":"繁體中文 Sign in"}';
    await page.getByLabel('任務 ID', { exact: true }).fill('locale-task');
    await page.getByLabel('函式原始碼（hcode）', { exact: true }).fill(source);
    await page.getByLabel('輸入資料（JSON）').fill(input);
    await page.getByLabel('任務收費上限（CPT，含平台費）').fill('1000');
    await page.getByRole('combobox', { name: '語言' }).selectOption('en');
    await expect(page.getByLabel('Function source (hcode)')).toHaveValue(source);
    await expect(page.getByLabel('Input data (JSON)')).toHaveValue(input);
    await expect(page.getByLabel('Task ID', { exact: true })).toHaveValue('locale-task');
    expect(fixture.loginCalls).toBe(1);
    expect(fixture.bootstrapCalls).toBe(1);
    await page.getByRole('combobox', { name: 'Language' }).selectOption('zh');
    await page.getByRole('button', { name: '傳送任務', exact: true }).click();
    await expect(page.getByText('任務已提交：locale-task', { exact: true })).toBeVisible();
    expect(fixture.submissions).toHaveLength(1);
    expect(fixture.submissions[0]).toMatchObject({ runtime: 'hcode', task_source: source, torrent: input, max_cpt: 1000 });
    await page.getByRole('tab', { name: '任務', exact: true }).click();
    await page.setViewportSize({ width: 320, height: 850 });
    await assertNoHorizontalOverflow(page);
    await page.reload();
    await expect(page.locator('html')).toHaveAttribute('lang', 'zh-TW');
    await expect(page.getByRole('button', { name: '登出', exact: true })).toBeVisible();
    await expect(page.getByText('已登入並準備就緒', { exact: true })).toBeVisible();
    expect(fixture.loginCalls).toBe(1);
    expect(fixture.unhandledRequests).toEqual([]);
  });

  test('translates an existing startup error without restarting startup', async ({ page }) => {
    const fixture = await installMockApi(page, { startupFailure: true });
    await page.goto(masterUiUrl);
    await expect(page.locator('.startup-message')).toContainText('Hivemind could not start');
    const calls = fixture.startupCalls;
    await page.getByRole('combobox', { name: 'Language' }).selectOption('zh');
    await expect(page.locator('.startup-message')).toHaveText('Hivemind 無法啟動 (fixture_startup_failure)。請關閉後重新開啟應用程式。');
    expect(fixture.startupCalls).toBe(calls);
  });

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
    await page.getByLabel('Function source (hcode)').fill('Draft instructions retained across tabs');
    await page.getByLabel('Input data (JSON)').fill('{"draft":true}');
    await submitTab.press('ArrowLeft');
    await expect(tasksTab).toHaveAttribute('aria-selected', 'true');
    await tasksTab.press('ArrowRight');
    await expect(submitTab).toHaveAttribute('aria-selected', 'true');
    await expect(page.getByLabel('Function source (hcode)')).toHaveValue('Draft instructions retained across tabs');
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
    await page.getByLabel('Function source (hcode)').fill('return input.value;');
    await page.getByLabel('Input data (JSON)').fill('{"value":42}');
    await page.getByLabel('Task charge cap (CPT, fee included)').fill('12');
    await page.getByRole('button', { name: 'Send task', exact: true }).click();
    await expect(page.getByText(`Task submitted: ${submittedTaskId}`, { exact: true })).toBeVisible();
    expect(fixture.submissions).toHaveLength(1);
    expect(fixture.submissions[0]).toMatchObject({
      task_id: submittedTaskId,
      runtime: 'hcode',
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
