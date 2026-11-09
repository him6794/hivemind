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
  page.on('console', (message) => {
    if (message.type() !== 'error') return;
    // Every page load checks the session with GET /api/account, and a visitor who is signed out gets a 401 there.
    // The browser logs that failed request as an error. Only that expected response is left out.
    if (message.text().includes('status of 401') && message.location().url.endsWith('/api/account')) return;
    diagnostics.errors.push(message.text());
  });
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
  // Every page load checks the session with GET /api/account. A visitor who is not signed in gets 401, which is expected.
  await page.route('**/api/account', (route) => route.fulfill({ status: 401, json: { success: false, code: 'authentication_required', message: 'Sign in to continue.' } }));
  return diagnostics;
}

async function visit(page, route) {
  await page.goto(`${siteUrl}/#/${route === 'home' ? '' : route}`);
  // The account page needs a session, so a visitor who is signed out is sent to the sign-in page.
  const shown = route === 'account' ? 'login' : route;
  await expect(page.getByRole('heading', { level: 1, name: headings[shown], exact: true })).toBeVisible();
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
  let signedIn = false;
  let releaseBalance;
  const balanceGate = new Promise((resolve) => { releaseBalance = resolve; });
  let balanceOpen = false;
  await page.route('**/api/login', async (route) => {
    calls.push({ path: '/api/login', body: route.request().postDataJSON() });
    signedIn = true;
    // The real sign-in sets the session as an HttpOnly cookie and returns only the name.
    await route.fulfill({ json: { success: true, message: 'Login successful', status_message: 'Login successful', username: 'browser-user' } });
  });
  await page.route('**/api/logout', async (route) => {
    calls.push({ path: '/api/logout' });
    signedIn = false;
    await route.fulfill({ json: { success: true, message: 'Signed out', status_message: 'Signed out' } });
  });
  // Every page load checks the session with GET /api/account. Before sign-in it answers 401. Once signed in,
  // the account page reads the balance from the same route, and those requests wait for the gate.
  await page.route('**/api/account', async (route) => {
    calls.push({ path: '/api/account', authorization: route.request().headers().authorization });
    if (!signedIn) {
      await route.fulfill({ status: 401, json: { success: false, code: 'authentication_required', message: 'Sign in to continue.' } });
      return;
    }
    // Signed-in account requests wait until the test releases the gate, so the loading state is visible.
    if (!balanceOpen) await balanceGate;
    await route.fulfill({ json: { username: 'browser-user', email: 'browser@example.com', emailVerified: true, balance: 125.5, welcomeGrantStatus: 'granted' } });
  });
  // Turnstile is replaced by a stub that reports a token at once, so the test needs no network.
  await page.route('https://challenges.cloudflare.com/turnstile/v0/api.js**', (route) => route.fulfill({
    contentType: 'application/javascript',
    body: 'window.turnstile = { render(container, options) { options.callback("e2e-captcha-token"); return "e2e-widget"; }, reset() {}, remove() {} };',
  }));
  await visit(page, 'login');
  await page.getByLabel('Username', { exact: true }).fill('browser-user');
  await page.getByLabel('Password', { exact: true }).fill('browser-password');
  await page.getByRole('button', { name: 'Sign in', exact: true }).click();
  await expect(page.getByRole('heading', { name: 'Account Center', exact: true })).toBeVisible();
  await expect(page.getByLabel('Loading balance')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Refresh', exact: true })).toBeDisabled();
  balanceOpen = true;
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
  expect(calls.find((call) => call.path === '/api/login').body.captchaToken).toBe('e2e-captcha-token');
  // The page never attaches the session token: the browser sends the HttpOnly cookie on its own.
  expect(calls.filter((call) => call.path === '/api/account').every((call) => call.authorization === undefined)).toBe(true);
  expect(diagnostics).toEqual({ errors: [], resources: [], unexpectedApi: [] });
});

test('website legacy accounts bind an email before verification and keep transfers disabled', async ({ page }) => {
  const diagnostics = await prepare(page);
  let email = '';
  const requests = [];
  await page.route('**/api/account', (route) => route.fulfill({ json: {
    success: true, username: 'legacy-user', email, emailVerified: false, balance: 2000, welcomeGrantStatus: 'granted',
  } }));
  await page.route('**/api/bind-email', async (route) => {
    const body = route.request().postDataJSON();
    requests.push(body);
    email = body.email;
    await route.fulfill({ json: { success: true } });
  });
  await page.route('https://challenges.cloudflare.com/turnstile/v0/api.js**', (route) => route.fulfill({
    contentType: 'application/javascript',
    body: 'window.turnstile = { render(container, options) { options.callback("e2e-captcha-token"); return "e2e-widget"; }, reset() {}, remove() {} };',
  }));
  await page.goto(`${siteUrl}/#/account`);
  await expect(page.getByRole('heading', { name: 'Account Center', exact: true })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Send verification email again', exact: true })).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Review transfer', exact: true })).toBeDisabled();
  await page.locator('#bind-email').fill('legacy@example.com');
  await page.locator('#bind-email-password').fill('legacy-password');
  await page.getByRole('button', { name: 'Add email and send verification', exact: true }).click();
  await expect(page.getByRole('button', { name: 'Send verification email again', exact: true })).toBeVisible();
  await expect(page.locator('#bind-email')).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Review transfer', exact: true })).toBeDisabled();
  await expect(page.getByText('2000.00', { exact: true })).toBeVisible();
  expect(requests).toEqual([{ email: 'legacy@example.com', currentPassword: 'legacy-password', captchaToken: 'e2e-captcha-token' }]);
  expect(diagnostics).toEqual({ errors: [], resources: [], unexpectedApi: [] });
});

test('website reset accepts a short Unicode password that meets the byte policy', async ({ page }) => {
  const diagnostics = await prepare(page);
  const requests = [];
  await page.route('**/api/reset-password', async (route) => {
    requests.push(route.request().postDataJSON());
    await route.fulfill({ json: { success: true } });
  });
  await page.goto(`${siteUrl}/#/reset-password/valid-reset-token`);
  await page.locator('#reset-password-new').fill('密碼測試');
  await page.locator('#reset-password-confirm').fill('密碼測試');
  await page.getByRole('button', { name: 'Set new password', exact: true }).click();
  await expect(page.getByText('Your password has been reset. Sign in with the new password.', { exact: true })).toBeVisible();
  expect(requests).toEqual([{ resetToken: 'valid-reset-token', newPassword: '密碼測試' }]);
  expect(diagnostics).toEqual({ errors: [], resources: [], unexpectedApi: [] });
});

test('website registration validation, then the verification notice, without signing in', async ({ page }) => {
  const diagnostics = await prepare(page);
  const calls = [];
  // The email step asks the server whether an account uses the address. Only the taken addresses answer taken.
  await page.route('**/api/check-email', async (route) => {
    const body = route.request().postDataJSON();
    const taken = body.email === 'taken@example.com' || body.email === 'unverified@example.com';
    await route.fulfill({ json: { success: true, available: !taken, verified: body.email === 'taken@example.com' } });
  });
  await page.route('**/api/register', async (route) => {
    calls.push({ path: '/api/register', body: route.request().postDataJSON() });
    await route.fulfill({ json: { success: true, message: 'Registration successful', status_message: 'Registration successful' } });
  });
  // Turnstile is replaced by a stub that reports a token at once, so the test needs no network.
  await page.route('https://challenges.cloudflare.com/turnstile/v0/api.js**', (route) => route.fulfill({
    contentType: 'application/javascript',
    body: 'window.turnstile = { render(container, options) { options.callback("e2e-captcha-token"); return "e2e-widget"; }, reset() {}, remove() {} };',
  }));
  await visit(page, 'register');
  // Step 1: the email. A value that is not an address stays on this step.
  await expect(page.getByText('Step 1 of 4 · Email', { exact: true })).toBeVisible();
  await page.getByLabel('Email', { exact: true }).fill('not-an-address');
  await page.getByRole('button', { name: 'Next', exact: true }).click();
  await expect(page.getByText('Enter a valid email address.', { exact: true })).toBeVisible();
  await page.getByLabel('Email', { exact: true }).fill('browser@example.com');
  await page.getByLabel('Email', { exact: true }).press('Enter');
  // Step 2: the username. Enter moves on.
  await expect(page.getByRole('heading', { level: 1, name: 'Choose a username', exact: true })).toBeVisible();
  await page.getByLabel('Username', { exact: true }).fill('browser-user');
  await page.getByRole('button', { name: 'Next', exact: true }).click();
  // Step 3: the password, entered twice. A mismatch stays on this step.
  await expect(page.getByRole('heading', { level: 1, name: 'Set a password', exact: true })).toBeVisible();
  await page.getByLabel('Password', { exact: true }).fill('browser-password');
  await page.getByLabel('Confirm password', { exact: true }).fill('wrong-password');
  await page.getByRole('button', { name: 'Next', exact: true }).click();
  await expect(page.getByText('Passwords do not match.', { exact: true })).toBeVisible();
  expect(calls).toEqual([]);
  await page.getByLabel('Confirm password', { exact: true }).fill('browser-password');
  await page.getByRole('button', { name: 'Next', exact: true }).click();
  // Step 4: the review shows what was entered. Back returns to the password step with its values kept.
  await expect(page.getByRole('heading', { level: 1, name: 'Check your details', exact: true })).toBeVisible();
  await expect(page.getByText('browser@example.com', { exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'Back', exact: true }).click();
  await expect(page.getByLabel('Confirm password', { exact: true })).toHaveValue('browser-password');
  await page.getByRole('button', { name: 'Next', exact: true }).click();
  // The human verification sits on the review, and the account is sent from there.
  await expect(page.getByRole('heading', { level: 1, name: 'Check your details', exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'Create account', exact: true }).click();
  await expect(page.getByRole('heading', { name: 'Check your email to verify the account', exact: true })).toBeVisible();
  await expect(page.getByText('We sent a verification email to browser@example.com.', { exact: true })).toBeVisible();
  // The account is not signed in here, so no login or balance request follows the registration.
  expect(calls).toEqual([{
    path: '/api/register',
    body: { username: 'browser-user', password: 'browser-password', email: 'browser@example.com', captchaToken: 'e2e-captcha-token' },
  }]);
  expect(diagnostics).toEqual({ errors: [], resources: [], unexpectedApi: [] });
});

test('website sign-up checks the email on the first step, as soon as it is typed', async ({ page }) => {
  const diagnostics = await prepare(page);
  const checks = [];
  await page.route('**/api/check-email', async (route) => {
    const body = route.request().postDataJSON();
    checks.push(body.email);
    const taken = body.email === 'taken@example.com' || body.email === 'unverified@example.com';
    await route.fulfill({ json: { success: true, available: !taken, verified: body.email === 'taken@example.com' } });
  });
  await visit(page, 'register');
  await page.getByLabel('Email', { exact: true }).fill('taken@example.com');
  // The answer appears on this step, without pressing Next.
  await expect(page.getByText('This email address has already been used.', { exact: true })).toBeVisible();
  // Next does not pass an address that is taken.
  await page.getByRole('button', { name: 'Next', exact: true }).click();
  await expect(page.getByRole('heading', { level: 1, name: 'Create your account', exact: true })).toBeVisible();
  await expect(page.getByText('This email address has already been used.', { exact: true })).toBeVisible();
  // An address that an account registered but never verified is refused too, with the way to finish that account.
  await page.getByLabel('Email', { exact: true }).fill('unverified@example.com');
  await expect(page.getByText(
    'This address is not verified yet. If you created the account, sign in to resend the email. Otherwise, do not verify it. The reservation expires 24 hours after binding; you can register again after that.',
    { exact: true }
  )).toBeVisible();
  await page.getByLabel('Email', { exact: true }).fill('free@example.com');
  await expect(page.getByText('This address is available.', { exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'Next', exact: true }).click();
  await expect(page.getByRole('heading', { level: 1, name: 'Choose a username', exact: true })).toBeVisible();
  // Each address is asked once. Pressing Next did not ask again.
  expect(checks).toEqual(['taken@example.com', 'unverified@example.com', 'free@example.com']);
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
    ['first-task', 'Your first task'],
    ['loops', 'Loops and accumulators'],
    ['master-api', 'Submit through the Master API'],
    ['templates-numbers', 'Numbers and lists'],
    ['templates-records', 'Records and decisions'],
    ['billing', 'How charges work'],
    ['limits', 'Limits'],
    ['api-reference', 'API reference'],
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
