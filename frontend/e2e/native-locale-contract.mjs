import { expect, test } from '@playwright/test';

async function installNativeLocaleFixture(page, role) {
  await page.addInitScript(({ role }) => {
    const storageKey = `hivemind.${role}.locale.v1`;
    localStorage.setItem(storageKey, 'zh');
    const accesses = [];
    for (const method of ['getItem', 'setItem']) {
      const original = Storage.prototype[method];
      Storage.prototype[method] = function (key, ...args) {
        if (key === storageKey) accesses.push(method);
        return original.call(this, key, ...args);
      };
    }
    let resolveRead;
    let rejectRead;
    const initialRead = new Promise((resolve, reject) => { resolveRead = resolve; rejectRead = reject; });
    const fixture = { reads: 0, writes: [], accesses, closeActions: [], failQuit: false, resolveRead, rejectRead: () => rejectRead(new Error('fixture read failure')) };
    window.__nativeLocaleFixture = fixture;
    window.__HIVEMIND_DESKTOP__ = Object.freeze({
      ready: async () => {},
      requestClose: async () => {},
      resolveClose: async (action) => {
        fixture.closeActions.push(action);
        if (action === 'quit' && fixture.failQuit) {
          fixture.failQuit = false;
          throw Object.assign(new Error('fixture unconfirmed save'), { code: 'locale-not-saved' });
        }
      },
      readLocale: () => { fixture.reads += 1; return initialRead; },
      writeLocale: (locale) => new Promise((resolve, reject) => {
        fixture.writes.push({ locale, resolve, reject: () => reject(new Error('fixture write failure')) });
      }),
    });
  }, { role });
}

async function settleWrite(page, index, ok = true) {
  await expect.poll(() => page.evaluate(() => window.__nativeLocaleFixture.writes.length)).toBeGreaterThan(index);
  await page.evaluate(({ index, ok }) => window.__nativeLocaleFixture.writes[index][ok ? 'resolve' : 'reject'](), { index, ok });
}

async function assertNoStorageOrDefaultWrite(page) {
  expect(await page.evaluate(() => ({
    accesses: window.__nativeLocaleFixture.accesses,
    writes: window.__nativeLocaleFixture.writes.map(({ locale }) => locale),
  }))).toEqual({ accesses: [], writes: [] });
}

// Browser contract tests deliberately mock native replies; process-restart evidence
// comes from scripts/native-ui-locale-smoke.mjs, not this fixture.
export function nativeLocaleContract({ role, url, installMockApi }) {
  test('native locale initialization gates startup without using browser language storage', async ({ page }) => {
    const fixture = await installMockApi(page);
    await installNativeLocaleFixture(page, role);
    await page.goto(url);
    await expect.poll(() => page.evaluate(() => window.__nativeLocaleFixture.reads)).toBe(1);
    await expect(page.getByRole('combobox')).toHaveCount(0);
    expect(fixture.startupCalls).toBe(0);
    await page.evaluate(() => window.__nativeLocaleFixture.resolveRead('zh'));
    await expect(page.getByRole('combobox', { name: '語言' })).toHaveValue('zh');
    await expect(page.locator('html')).toHaveAttribute('lang', 'zh-TW');
    await expect.poll(() => fixture.startupCalls).toBeGreaterThan(0);
    await assertNoStorageOrDefaultWrite(page);
  });

  for (const result of ['missing', 'failed', 'corrupt']) {
    test(`native ${result} preference defaults to English without writing it back`, async ({ page }) => {
      await installMockApi(page);
      await installNativeLocaleFixture(page, role);
      await page.goto(url);
      await page.evaluate((result) => {
        const fixture = window.__nativeLocaleFixture;
        if (result === 'failed') fixture.rejectRead();
        else fixture.resolveRead(result === 'missing' ? null : 'fr');
      }, result);
      await expect(page.getByRole('combobox', { name: 'Language' })).toHaveValue('en');
      await expect(page.getByLabel('Username')).toBeVisible();
      const error = page.getByText('Could not read the language preference. Using English.', { exact: true });
      if (result === 'missing') await expect(error).toHaveCount(0);
      else await expect(error).toBeVisible();
      await assertNoStorageOrDefaultWrite(page);
    });
  }

  test('native read timeout leaves the UI usable and a late reply cannot replace a selection', async ({ page }) => {
    await installMockApi(page);
    await installNativeLocaleFixture(page, role);
    await page.goto(url);
    await expect(page.getByText('Could not read the language preference. Using English.', { exact: true })).toBeVisible();
    await assertNoStorageOrDefaultWrite(page);
    await page.getByLabel('Username').fill('late-read-draft');
    await page.getByRole('combobox', { name: 'Language' }).selectOption('zh');
    await settleWrite(page, 0);
    await page.evaluate(() => window.__nativeLocaleFixture.resolveRead('en'));
    await expect(page.getByRole('combobox', { name: '語言' })).toHaveValue('zh');
    await expect(page.getByLabel('使用者名稱')).toHaveValue('late-read-draft');
    await expect(page.getByRole('combobox')).toHaveAttribute('aria-busy', 'false');
  });

  test('native unconfirmed save keeps the translated Quit dialog open for an explicit retry', async ({ page }, testInfo) => {
    await installMockApi(page);
    await installNativeLocaleFixture(page, role);
    await page.goto(url);
    await page.evaluate(() => window.__nativeLocaleFixture.resolveRead('zh'));
    await expect(page.getByLabel('使用者名稱')).toBeVisible();
    await page.evaluate(() => {
      window.__nativeLocaleFixture.failQuit = true;
      window.dispatchEvent(new Event('hivemind:close-requested'));
    });
    const dialog = page.getByRole('alertdialog');
    await dialog.getByRole('button', { name: '結束', exact: true }).click();
    await expect(dialog.getByRole('alert')).toHaveText('語言可能未儲存，再按結束可離開。');
    await expect(page.locator('html')).toHaveAttribute('data-window-state', 'visible');
    await expect(dialog.getByRole('button', { name: '結束', exact: true })).toBeEnabled();
    await testInfo.attach(`${role}-native-unsaved-warning.png`, { body: await page.screenshot(), contentType: 'image/png' });
    expect(await page.evaluate(() => window.__nativeLocaleFixture.closeActions)).toEqual(['quit']);
    await dialog.getByRole('button', { name: '結束', exact: true }).click();
    await expect(dialog).toBeHidden();
    expect(await page.evaluate(() => window.__nativeLocaleFixture.closeActions)).toEqual(['quit', 'quit']);
  });

  test('native rapid selections use the newest ACK and preserve drafts and authentication', async ({ page }) => {
    const fixture = await installMockApi(page);
    await installNativeLocaleFixture(page, role);
    await page.goto(url);
    await page.evaluate(() => window.__nativeLocaleFixture.resolveRead(null));
    await page.getByLabel('Username').fill('native-locale-qa');
    await page.getByLabel('Password').fill('fixture-password');
    await page.getByRole('combobox', { name: 'Language' }).selectOption('zh');
    await page.getByRole('combobox', { name: '語言' }).selectOption('en');
    await page.getByRole('combobox', { name: 'Language' }).selectOption('zh');
    await expect(page.getByRole('combobox')).toHaveAttribute('aria-busy', 'true');
    await settleWrite(page, 0, false);
    await expect(page.getByRole('combobox')).toHaveAttribute('aria-busy', 'true');
    await settleWrite(page, 2);
    await expect(page.getByRole('combobox')).toHaveAttribute('aria-busy', 'false');
    await settleWrite(page, 1, false);
    await expect(page.getByText('語言已切換，但無法確認是否已儲存。', { exact: true })).toHaveCount(0);
    await expect(page.getByLabel('使用者名稱')).toHaveValue('native-locale-qa');
    await expect(page.getByLabel('密碼')).toHaveValue('fixture-password');

    await page.getByRole('combobox', { name: '語言' }).selectOption('en');
    await settleWrite(page, 3, false);
    await expect(page.getByText('Language changed, but saving could not be confirmed.', { exact: true })).toBeVisible();
    await page.getByRole('combobox', { name: 'Language' }).selectOption('zh');
    await settleWrite(page, 4);
    await expect(page.getByText('語言已切換，但無法確認是否已儲存。', { exact: true })).toHaveCount(0);

    await page.getByRole('button', { name: role === 'master' ? '登入' : '登入並連線', exact: true }).click();
    await expect(page.getByText(role === 'master' ? '已登入並準備就緒' : '此電腦已準備好接收任務。', { exact: true })).toBeVisible();
    const flow = () => role === 'master'
      ? { login: fixture.loginCalls, bootstrap: fixture.bootstrapCalls, startup: fixture.startupCalls }
      : { events: [...fixture.flowEvents], startup: fixture.startupCalls };
    const before = flow();
    if (role === 'master') {
      await page.getByRole('tab', { name: '提交', exact: true }).click();
      await page.getByLabel('任務程式', { exact: true }).fill('get(input, "text");');
      await page.getByLabel('輸入資料（JSON）').fill('{"text":"原生語言草稿"}');
    } else {
      await page.getByRole('tab', { name: '電腦', exact: true }).click();
    }
    await page.getByRole('combobox', { name: '語言' }).selectOption('en');
    await settleWrite(page, 5);
    if (role === 'master') {
      await expect(page.getByLabel('Task instructions')).toHaveValue('get(input, "text");');
      await expect(page.getByLabel('Input data (JSON)')).toHaveValue('{"text":"原生語言草稿"}');
    } else {
      await expect(page.getByRole('tab', { name: 'Computer', exact: true })).toHaveAttribute('aria-selected', 'true');
    }
    expect(flow()).toEqual(before);
    expect(await page.evaluate(() => window.__nativeLocaleFixture.accesses)).toEqual([]);
    expect(await page.evaluate(() => window.__nativeLocaleFixture.reads)).toBe(1);
  });
}
