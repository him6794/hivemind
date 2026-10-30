import { expect, test } from '@playwright/test';

const siteUrl = process.env.HIVEMIND_SITE_URL || 'http://127.0.0.1:4175';
const headings = {
  home: 'Run compute tasks on spare computers',
  login: 'Sign in to Hivemind',
  register: 'Create your account',
  account: 'Account Center',
  pricing: 'You set the limit. Confirmed work sets the charge.',
  share: 'Share the spare capacity of your computer.',
  download: 'Download and install',
  security: 'How the network confirms charges',
  faq: 'Status and questions',
  docs: 'Introduction',
  terms: 'Usage rules and service limits',
};

async function prepare(page, theme = 'light', locale = 'en') {
  const diagnostics = { errors: [], resources: [], unexpectedApi: [] };
  page.on('pageerror', (error) => diagnostics.errors.push(error.message));
  page.on('console', (message) => { if (message.type() === 'error') diagnostics.errors.push(message.text()); });
  page.on('requestfailed', (request) => {
    if (['script', 'stylesheet', 'font', 'image'].includes(request.resourceType())) {
      diagnostics.resources.push(`${request.url()}: ${request.failure()?.errorText}`);
    }
  });
  page.on('response', (response) => {
    if (response.status() >= 400 && ['script', 'stylesheet', 'font', 'image'].includes(response.request().resourceType())) {
      diagnostics.resources.push(`${response.status()} ${response.url()}`);
    }
  });
  await page.addInitScript(({ theme, locale }) => {
    localStorage.setItem('theme', theme);
    localStorage.setItem('hivemind-site-locale', JSON.stringify({ state: { locale }, version: 0 }));
  }, { theme, locale });
  await page.route('**/api/**', async (route) => {
    diagnostics.unexpectedApi.push(new URL(route.request().url()).pathname);
    await route.fulfill({ status: 200, json: { success: false, message: 'Unexpected website API in browser fixture' } });
  });
  return diagnostics;
}

async function visit(page, route) {
  await page.goto(`${siteUrl}/#/${route === 'home' ? '' : route}`);
  await expect(page.getByRole('heading', { level: 1, name: headings[route], exact: true })).toBeVisible();
}

async function checkDisplay(page) {
  for (const element of await page.locator('main [style*="opacity"]').all()) {
    await expect(element).toHaveCSS('opacity', '1');
  }
  // The root wrapper clips overflow, so the document width alone cannot show content that runs past the
  // screen. Measure the main column too.
  const state = await page.evaluate(() => {
    const main = document.querySelector('main');
    return {
      overflow: document.documentElement.scrollWidth > document.documentElement.clientWidth + 1,
      contentOverflow: Boolean(main) && main.scrollWidth > main.clientWidth + 1,
      brokenImages: [...document.images].filter((image) => !image.complete || image.naturalWidth === 0).map((image) => image.src),
      headings: document.querySelectorAll('h1').length,
    };
  });
  expect(state).toEqual({ overflow: false, contentOverflow: false, brokenImages: [], headings: 1 });
  const heading = await page.getByRole('heading', { level: 1 }).boundingBox();
  // The site header is the first <header> on the page. Page sections can add their own.
  const header = await page.locator('header').count() ? await page.locator('header').first().boundingBox() : null;
  if (header) expect(heading.y).toBeGreaterThanOrEqual(header.y + header.height);
}

async function capture(page, testInfo, name) {
  await testInfo.attach(name, { body: await page.screenshot({ animations: 'disabled' }), contentType: 'image/png' });
}

for (const theme of ['light', 'dark']) {
  test(`website routes render with ${theme} tokens at desktop, tablet and 320px`, async ({ page }, testInfo) => {
    const diagnostics = await prepare(page, theme);
    for (const width of [1440, 768, 320]) {
      await page.setViewportSize({ width, height: 1000 });
      for (const route of Object.keys(headings)) {
        await visit(page, route);
        await checkDisplay(page);
        const colors = await page.evaluate((expected) => {
          const canvas = document.createElement('canvas');
          canvas.width = canvas.height = 1;
          const context = canvas.getContext('2d');
          const sample = (color) => {
            context.fillStyle = color;
            context.fillRect(0, 0, 1, 1);
            return [...context.getImageData(0, 0, 1, 1).data];
          };
          return {
            actual: sample(getComputedStyle(document.documentElement).getPropertyValue('--background').trim()),
            expected: sample(expected),
          };
        }, theme === 'dark' ? 'oklch(0.145 0 0)' : 'oklch(1 0 0)');
        expect(colors.actual).toEqual(colors.expected);
        if ((width === 320 || width === 1440) && ['home', 'login', 'docs'].includes(route)) {
          await capture(page, testInfo, `website-${route}-${width}-${theme}.png`);
        }
      }
    }
    expect(diagnostics).toEqual({ errors: [], resources: [], unexpectedApi: [] });
  });
}

test('website login, balance loading/refresh and medium-width logout stay account-only', async ({ page }, testInfo) => {
  const diagnostics = await prepare(page);
  const calls = [];
  let releaseBalance;
  const balanceGate = new Promise((resolve) => { releaseBalance = resolve; });
  let balanceCalls = 0;
  await page.route('**/api/login', async (route) => {
    calls.push({ path: '/api/login', body: route.request().postDataJSON() });
    await route.fulfill({ json: { success: true, token: 'website-browser-only-token' } });
  });
  await page.route('**/api/balance', async (route) => {
    calls.push({ path: '/api/balance', authorization: route.request().headers().authorization });
    balanceCalls += 1;
    if (balanceCalls === 1) await balanceGate;
    await route.fulfill({ json: balanceCalls < 3 ? { balance: '125.50' } : { message: 'Fixture refresh failed' }, status: balanceCalls < 3 ? 200 : 503 });
  });
  await visit(page, 'login');
  await page.getByLabel('Username', { exact: true }).fill('browser-user');
  await page.getByLabel('Password', { exact: true }).fill('browser-password');
  await page.getByRole('button', { name: 'Sign in', exact: true }).click();
  await expect(page.getByRole('heading', { name: 'Account Center', exact: true })).toBeVisible();
  await expect(page.getByLabel('Loading balance')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Refresh', exact: true })).toBeDisabled();
  releaseBalance();
  await expect(page.getByText('125.50', { exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'Refresh', exact: true }).click();
  await expect(page.getByRole('button', { name: 'Refresh', exact: true })).toBeEnabled();
  await capture(page, testInfo, 'website-account.png');
  await page.setViewportSize({ width: 820, height: 900 });
  await page.getByRole('button', { name: 'Toggle menu', exact: true }).click();
  const menu = page.getByRole('dialog');
  await expect(menu).toBeVisible();
  await menu.getByRole('button', { name: 'Sign out', exact: true }).click();
  await expect(page.getByRole('heading', { name: headings.home, exact: true })).toBeVisible();
  await expect(menu).toBeHidden();
  const storage = await page.evaluate(() => [...Object.keys(localStorage), ...Object.keys(sessionStorage)].some((key) => /auth|token|session/i.test(key)));
  expect(storage).toBe(false);
  expect(calls.filter((call) => call.path === '/api/login')).toHaveLength(1);
  expect(calls.filter((call) => call.path === '/api/balance').every((call) => call.authorization === 'Bearer website-browser-only-token')).toBe(true);
  expect(diagnostics).toEqual({ errors: [], resources: [], unexpectedApi: [] });
});

test('website registration validation and automatic login use only website account endpoints', async ({ page }) => {
  const diagnostics = await prepare(page);
  const calls = [];
  for (const path of ['/api/register', '/api/login', '/api/balance']) {
    await page.route(`**${path}`, async (route) => {
      calls.push({ path, body: route.request().postData() ? route.request().postDataJSON() : null });
      await route.fulfill({ json: path === '/api/login' ? { success: true, token: 'register-browser-token' } : path === '/api/balance' ? { balance: 0 } : { success: true } });
    });
  }
  await visit(page, 'register');
  await page.getByLabel('Username', { exact: true }).fill('browser-user');
  await page.getByLabel('Password', { exact: true }).fill('browser-password');
  await page.getByLabel('Confirm password', { exact: true }).fill('wrong-password');
  await page.getByRole('button', { name: 'Create account', exact: true }).click();
  await expect(page.getByText('Passwords do not match.', { exact: true })).toBeVisible();
  expect(calls).toEqual([]);
  await page.getByLabel('Confirm password', { exact: true }).fill('browser-password');
  await page.getByRole('button', { name: 'Create account', exact: true }).click();
  await expect(page.getByRole('heading', { name: 'Account Center', exact: true })).toBeVisible();
  await expect(page.getByText('0.00', { exact: true })).toBeVisible();
  expect(calls.map((call) => call.path)).toEqual(['/api/register', '/api/login', '/api/balance']);
  expect(diagnostics).toEqual({ errors: [], resources: [], unexpectedApi: [] });
});

test('website keyboard navigation, locale menu and reduced motion work without clipped dialogs', async ({ page }, testInfo) => {
  const diagnostics = await prepare(page);
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.setViewportSize({ width: 320, height: 568 });
  await visit(page, 'home');
  await page.getByRole('button', { name: 'Switch language', exact: true }).click();
  await page.getByRole('menuitemradio', { name: '中文', exact: true }).click();
  // The home page has more than one create-account button; any of them proves the locale switched.
  await expect(page.getByRole('main').getByRole('button', { name: '建立帳號', exact: true }).first()).toBeVisible();
  await page.getByRole('button', { name: '切換語言', exact: true }).click();
  await page.getByRole('menuitemradio', { name: 'English', exact: true }).click();
  await page.keyboard.press('Control+k');
  const dialog = page.getByRole('dialog');
  await expect(dialog).toBeVisible();
  await expect(page.getByLabel('Search pages')).toBeFocused();
  await page.getByLabel('Search pages').fill('docs');
  await page.keyboard.press('Enter');
  await expect(page.getByRole('heading', { name: headings.docs, exact: true })).toBeVisible();
  await expect(dialog).toBeHidden();
  await page.getByRole('button', { name: 'Toggle menu', exact: true }).click();
  await expect(dialog).toBeVisible();
  const bounds = await dialog.boundingBox();
  expect(bounds.x).toBeGreaterThanOrEqual(0);
  expect(bounds.x + bounds.width).toBeLessThanOrEqual(321);
  expect(bounds.y + bounds.height).toBeLessThanOrEqual(569);
  await page.keyboard.press('Escape');
  await expect(dialog).toBeHidden();
  await expect(page.getByRole('button', { name: 'Toggle menu', exact: true })).toBeFocused();
  await capture(page, testInfo, 'website-mobile-reduced-motion.png');
  await checkDisplay(page);
  expect(diagnostics).toEqual({ errors: [], resources: [], unexpectedApi: [] });
});

test('a direct documentation address opens the same page of the single-page site', async ({ page }) => {
  const diagnostics = await prepare(page);
  await page.goto(`${siteUrl}/docs/loops`);
  await expect(page.getByRole('heading', { level: 1, name: 'Loops and accumulators', exact: true })).toBeVisible();
  expect(new URL(page.url()).hash).toBe('#/docs/loops');
  expect(diagnostics).toEqual({ errors: [], resources: [], unexpectedApi: [] });
});

test('documentation tutorial, template, and reference pages render at desktop and 320px', async ({ page }) => {
  const diagnostics = await prepare(page);
  const pages = [
    ['loops', 'Loops and accumulators'],
    ['templates-numbers', 'Numbers and lists'],
    ['billing', 'How charges work'],
    ['master-api', 'Submit through the Master API'],
    ['ui-forms', 'Forms'],
    ['ui-layout', 'Layout and data'],
    ['ui-charts', 'Charts'],
    ['ui-animations', 'Animations'],
  ];
  for (const width of [1440, 320]) {
    await page.setViewportSize({ width, height: 1000 });
    for (const [slug, title] of pages) {
      await page.goto(`${siteUrl}/#/docs/${slug}`);
      await expect(page.getByRole('heading', { level: 1, name: title, exact: true })).toBeVisible();
      await checkDisplay(page);
    }
  }
  expect(diagnostics).toEqual({ errors: [], resources: [], unexpectedApi: [] });
});
