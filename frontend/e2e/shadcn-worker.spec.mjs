import { expect, test } from '@playwright/test';
import { waitForStableDisplay } from './shadcn-display.mjs';

const workerUiUrl = process.env.HIVEMIND_WORKER_UI_URL || 'http://127.0.0.1:4174';
const sessionKey = 'hivemind.worker.session.v1';
const themeKey = 'hivemind.worker.theme';
const diagnosticsByPage = new WeakMap();

function createDashboard(overrides = {}) {
  return {
    success: true,
    worker_id: 'fixture-worker-01',
    sampled_at: '2026-09-30T12:00:00.000Z',
    stale: false,
    host: {
      cpu_cores: 0,
      cpu_usage_percent: null,
      memory_total_gb: 16,
      memory_available_gb: 12,
      memory_usage_percent: 25,
      gpu_count: 0,
      gpu_utilization_percent: null,
      vram_total_mb: null,
      vram_available_mb: null,
      storage_total_gb: 512,
      storage_available_gb: 256,
    },
    assignments: [{
      task_id: 'fixture-task-with-a-long-id-that-must-wrap-within-the-assignment-card',
      submitter: 'browser-fixture',
      status: 'RUNNING',
      max_cpt: 25,
      reported_usage_cpt: 0,
      usage_basis: 'worker-reported',
      usage_updated_at: '2026-09-30T11:59:00.000Z',
    }],
    settled_provider_credits_cpt: 123.45,
    currency: 'CPT',
    ...overrides,
  };
}

function createProfile() {
  return {
    worker_id: 'fixture-worker-01',
    ip: '',
    location: 'local',
    cpu_cores: 8,
    memory_gb: 32,
    cpu_score: 1200,
    gpu_score: 0,
    gpu_memory_gb: 0,
    gpu_name: '',
    storage_total_gb: 512,
    storage_available_gb: 256,
  };
}

async function installMockApi(page, { startupScenario = 'ready' } = {}) {
  let releaseInitialStartup;
  const initialStartupGate = new Promise((resolve) => { releaseInitialStartup = resolve; });
  const fixture = {
    calls: [],
    flowEvents: [],
    startupCalls: 0,
    dashboardCalls: 0,
    failNextDashboard: false,
    failNextProfile: false,
    unhandledRequests: [],
    startupScenario,
    releaseInitialStartup: () => releaseInitialStartup(),
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
    fixture.calls.push({
      method,
      pathname,
      authorization: request.headers().authorization || '',
      body: request.postDataJSON?.() || null,
    });

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
      if (fixture.startupScenario === 'failure-then-retry') {
        if (fixture.startupCalls === 1) {
          await initialStartupGate;
          await fulfillJson({ state: 'initializing', phase: 'resources' });
        } else if (fixture.startupCalls === 2) {
          await fulfillJson({ state: 'failed', phase: 'services', code: 'fixture_startup_failure' });
        } else {
          await fulfillJson({ state: 'ready', phase: 'ready' });
        }
      } else {
        await fulfillJson({ state: 'ready', phase: 'ready' });
      }
      return;
    }

    if (method === 'POST' && pathname === '/api/login') {
      fixture.flowEvents.push('login');
      await fulfillJson({ success: true, token: 'browser-fixture-token' });
      return;
    }

    if (method === 'POST' && pathname === '/api/vpn/bootstrap') {
      fixture.flowEvents.push('vpn');
      await fulfillJson({ success: true, state: 'ready' });
      return;
    }

    if (method === 'GET' && pathname === '/api/worker-info') {
      fixture.flowEvents.push('profile');
      if (fixture.failNextProfile) {
        fixture.failNextProfile = false;
        await fulfillJson({ success: false, message: 'fixture profile unavailable' });
      } else {
        await fulfillJson({ success: true, profile: createProfile() });
      }
      return;
    }

    if (method === 'POST' && pathname === '/api/register-worker') {
      fixture.flowEvents.push('registration');
      await fulfillJson({ success: true, worker_id: 'fixture-worker-01' });
      return;
    }

    if (method === 'GET' && pathname === '/api/worker-dashboard') {
      fixture.dashboardCalls += 1;
      if (fixture.failNextDashboard) {
        fixture.failNextDashboard = false;
        await fulfillJson({ success: false, message: 'fixture dashboard refresh failure' });
      } else {
        await fulfillJson(createDashboard());
      }
      return;
    }

    fixture.unhandledRequests.push(`${method} ${pathname}`);
    await fulfillJson({ success: false, message: `Unhandled browser fixture request: ${method} ${pathname}` });
  });

  return fixture;
}

function installBrowserDiagnostics(page) {
  const diagnostics = { consoleErrors: [], pageErrors: [], failedRequests: [], badResponses: [] };
  diagnosticsByPage.set(page, diagnostics);
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

async function signIn(page) {
  await page.getByLabel('Username').fill('browser-qa');
  await page.getByLabel('Password').fill('fixture-password');
  await page.getByRole('button', { name: 'Sign in and connect' }).click();
  await expect(page.getByText('Account-wide settled provider credits', { exact: true })).toBeVisible();
  await expect(page.getByText('This computer is connected and ready to receive tasks.', { exact: true })).toHaveCount(0);
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
  installBrowserDiagnostics(page);
});

test.afterEach(async ({ page }, testInfo) => {
  const diagnostics = diagnosticsByPage.get(page);
  if (diagnostics) {
    await testInfo.attach('worker-browser-diagnostics.json', {
      body: Buffer.from(JSON.stringify(diagnostics, null, 2)),
      contentType: 'application/json',
    });
    expect(diagnostics.consoleErrors, 'browser console errors').toEqual([]);
    expect(diagnostics.pageErrors, 'uncaught page errors').toEqual([]);
    expect(diagnostics.failedRequests, 'failed network requests').toEqual([]);
    expect(diagnostics.badResponses, 'HTTP resource/API failures').toEqual([]);
  }
});

test.describe('production Worker shadcn UI regression', () => {
  test('shows startup progress, handles a startup failure, and retries successfully', async ({ page }, testInfo) => {
    const fixture = await installMockApi(page, { startupScenario: 'failure-then-retry' });
    await page.goto(workerUiUrl);

    await expect(page.locator('.startup-card')).toBeVisible();
    await expect(page.locator('.startup-title')).toHaveText('Starting Worker');
    await attachScreenshot(page, testInfo, 'worker-startup-loading.png');
    fixture.releaseInitialStartup();
    await expect(page.locator('.startup-title')).toHaveText('Worker setup needs attention', { timeout: 10_000 });
    await expect(page.getByRole('alert')).toContainText('Worker startup failed (fixture_startup_failure)');
    await expect(page.getByRole('button', { name: 'Check again', exact: true })).toBeVisible();
    expect(fixture.startupCalls).toBe(2);
    await attachScreenshot(page, testInfo, 'worker-startup-failure.png');

    await page.getByRole('button', { name: 'Check again', exact: true }).click();
    await expect(page.getByRole('heading', { name: 'Sign in to connect' })).toBeVisible();
    expect(fixture.startupCalls).toBe(3);
    expect(fixture.unhandledRequests).toEqual([]);
    await attachScreenshot(page, testInfo, 'worker-startup-recovered.png');
  });

  test('signs in through VPN, profile and registration, then preserves honest dashboard data', async ({ page }, testInfo) => {
    const fixture = await installMockApi(page);
    await page.setViewportSize({ width: 1440, height: 1000 });
    await page.goto(workerUiUrl);
    await expect(page.getByRole('heading', { name: 'Sign in to connect' })).toBeVisible();
    await signIn(page);

    const loginIndex = fixture.flowEvents.indexOf('login');
    expect(fixture.flowEvents.slice(loginIndex)).toEqual(['login', 'vpn', 'profile', 'registration']);
    const requests = fixture.calls;
    const loginRequest = requests.find((call) => call.pathname === '/api/login');
    const registrationRequest = requests.find((call) => call.pathname === '/api/register-worker');
    expect(loginRequest?.body).toEqual({ username: 'browser-qa', password: 'fixture-password' });
    expect(registrationRequest?.authorization).toBe('Bearer browser-fixture-token');
    expect(registrationRequest?.body).toMatchObject({ username: 'browser-qa', worker_id: 'fixture-worker-01' });

    const storage = await page.evaluate((key) => ({
      localKeys: Object.keys(window.localStorage),
      sensitiveLocalKeys: Object.keys(window.localStorage).filter((name) => /token|auth|session/i.test(name)),
      session: JSON.parse(window.sessionStorage.getItem(key) || '{}'),
    }), sessionKey);
    expect(storage.localKeys).toContain(themeKey);
    expect(storage.sensitiveLocalKeys).toEqual([]);
    expect(storage.session).toEqual({ token: 'browser-fixture-token', username: 'browser-qa' });

    await expect(page.getByText('Account-wide settled provider credits', { exact: true })).toBeVisible();
    await expect(page.getByText('Account-wide total; not a per-worker payout.', { exact: true })).toBeVisible();
    await expect(page.locator('.credit-value')).toHaveText('123.45');
    await expect(page.getByText('Processor', { exact: true })).toBeVisible();
    await expect(page.getByText('Memory', { exact: true })).toBeVisible();
    await expect(page.getByText('Graphics', { exact: true })).toBeVisible();
    await expect(page.getByText('Storage', { exact: true })).toBeVisible();
    await expect(page.getByRole('meter', { name: 'CPU usage' })).toHaveCount(0);
    await expect(page.getByText('Telemetry unavailable').first()).toBeVisible();
    await expect(page.getByRole('meter', { name: 'Memory usage' })).toHaveAttribute('aria-valuenow', '25');
    await expect(page.getByRole('meter', { name: 'Memory usage' })).toHaveAttribute('aria-valuetext', '25 %');
    await expect(page.locator('.resource-card').filter({ hasText: 'Processor' })).toContainText('0 cores');
    await expect(page.locator('.resource-card').filter({ hasText: 'Graphics' })).toContainText('0');
    await expect(page.getByText('fixture-task-with-a-long-id-that-must-wrap-within-the-assignment-card')).toBeVisible();

    await assertNoHorizontalOverflow(page);
    await expect(page.locator('img')).toHaveCount(0);
    await attachScreenshot(page, testInfo, 'worker-desktop-light.png');

    await page.getByRole('button', { name: 'Switch to dark theme' }).click();
    await expect.poll(() => page.evaluate(() => document.documentElement.classList.contains('dark'))).toBe(true);
    await expect.poll(() => page.evaluate((key) => window.localStorage.getItem(key), themeKey)).toBe('dark');
    await assertNoHorizontalOverflow(page);
    await attachScreenshot(page, testInfo, 'worker-desktop-dark.png');

    const overviewTab = page.getByRole('tab', { name: 'Overview' });
    const computerTab = page.getByRole('tab', { name: 'Computer' });
    await overviewTab.focus();
    await overviewTab.press('ArrowRight');
    await expect(computerTab).toHaveAttribute('aria-selected', 'true');
    await expect(computerTab).toBeFocused();
    await expect(page.getByText('This computer', { exact: true })).toBeVisible();
    await expect(page.getByText('This computer is connected and ready to receive tasks.', { exact: true })).toBeVisible();
    await computerTab.press('ArrowLeft');
    await expect(overviewTab).toHaveAttribute('aria-selected', 'true');

    await page.emulateMedia({ reducedMotion: 'reduce' });
    const reducedMotion = await page.locator('.resource-card').first().evaluate((element) => ({
      preference: matchMedia('(prefers-reduced-motion: reduce)').matches,
      animationName: getComputedStyle(element).animationName,
    }));
    expect(reducedMotion).toEqual({ preference: true, animationName: 'none' });

    await page.setViewportSize({ width: 320, height: 850 });
    await expect.poll(() => page.evaluate(() => window.innerWidth)).toBe(320);
    await assertNoHorizontalOverflow(page);
    await attachScreenshot(page, testInfo, 'worker-mobile-320-dark.png');

    await page.getByRole('button', { name: 'Switch to light theme' }).click();
    await expect.poll(() => page.evaluate(() => document.documentElement.classList.contains('dark'))).toBe(false);
    await assertNoHorizontalOverflow(page);
    await attachScreenshot(page, testInfo, 'worker-mobile-320-light.png');

    await page.getByRole('button', { name: 'Sign out', exact: true }).click();
    await expect(page.getByRole('heading', { name: 'Sign in to connect' })).toBeVisible();
    await expect(page.getByRole('tab', { name: 'Overview' })).toHaveCount(0);
    const cleared = await page.evaluate((key) => ({
      session: window.sessionStorage.getItem(key),
      localTheme: window.localStorage.getItem('hivemind.worker.theme'),
    }), sessionKey);
    expect(cleared).toEqual({ session: null, localTheme: 'light' });
    expect(fixture.unhandledRequests).toEqual([]);
  });

  test('retains sign-in and retries setup after the local profile is unavailable', async ({ page }, testInfo) => {
    const fixture = await installMockApi(page);
    await page.goto(workerUiUrl);
    await expect(page.getByRole('heading', { name: 'Sign in to connect' })).toBeVisible();
    fixture.failNextProfile = true;
    await page.getByLabel('Username').fill('browser-qa');
    await page.getByLabel('Password').fill('fixture-password');
    await page.getByRole('button', { name: 'Sign in and connect' }).click();

    const setupAlert = page.getByRole('alert').filter({ hasText: 'Session needs attention' });
    await expect(setupAlert).toContainText('Sign-in succeeded, but worker setup is incomplete. Retry connection when ready.');
    expect(fixture.calls.filter((call) => call.pathname === '/api/register-worker')).toHaveLength(0);
    expect(await page.evaluate((key) => JSON.parse(sessionStorage.getItem(key)), sessionKey)).toEqual({
      token: 'browser-fixture-token', username: 'browser-qa',
    });
    await attachScreenshot(page, testInfo, 'worker-profile-unavailable.png');

    await setupAlert.getByRole('button', { name: 'Retry connection', exact: true }).click();
    await expect(setupAlert).toHaveCount(0);
    await expect(page.getByText('This computer is ready to receive tasks.', { exact: true })).toBeVisible();
    expect(fixture.calls.filter((call) => call.pathname === '/api/login')).toHaveLength(1);
    const registration = fixture.calls.filter((call) => call.pathname === '/api/register-worker');
    expect(registration).toHaveLength(1);
    expect(registration[0].authorization).toBe('Bearer browser-fixture-token');
    expect(fixture.unhandledRequests).toEqual([]);
    await attachScreenshot(page, testInfo, 'worker-profile-recovered.png');
  });

  test('retains the last dashboard snapshot when a refresh fails', async ({ page }, testInfo) => {
    const fixture = await installMockApi(page);
    await page.goto(workerUiUrl);
    await expect(page.getByRole('heading', { name: 'Sign in to connect' })).toBeVisible();
    await signIn(page);
    await expect(page.locator('.dashboard-freshness')).toContainText('Current snapshot');

    fixture.failNextDashboard = true;
    await page.getByRole('button', { name: 'Refresh', exact: true }).click();
    await expect(page.getByText('Refresh failed; showing the last received snapshot.')).toBeVisible();
    await expect(page.locator('.dashboard-freshness')).toContainText('Stale snapshot');
    await expect(page.locator('.credit-value')).toHaveText('123.45');
    await expect(page.getByText('fixture-task-with-a-long-id-that-must-wrap-within-the-assignment-card')).toBeVisible();
    expect(fixture.dashboardCalls).toBe(2);
    await assertNoHorizontalOverflow(page);
    await attachScreenshot(page, testInfo, 'worker-stale-dashboard.png');
    expect(fixture.unhandledRequests).toEqual([]);
  });
});
