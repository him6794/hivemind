import { expect, test } from '@playwright/test';
import { readFile } from 'node:fs/promises';

const nativeBridge = await readFile(new URL('../../hivemind-rs/crates/hivemind-bin/src/local_ui_bridge.js', import.meta.url), 'utf8');

async function loadNativeFallback(page, port) {
  const themeKey = port === 4173 ? 'hivemind.master.theme.v1' : 'hivemind.worker.theme';
  const source = nativeBridge
    .replace('__HIVEMIND_ORIGIN__', JSON.stringify(`http://127.0.0.1:${port}`))
    .replace('__HIVEMIND_NONCE__', JSON.stringify('native-fallback-fixture'));
  await page.addInitScript({ content: `
    window.localStorage.setItem(${JSON.stringify(themeKey)}, 'dark');
    window.__windowActions = [];
    window.__hostOutcome = 'success';
    window.chrome ??= {};
    window.chrome.webview = { postMessage(message) {
      const { action, id } = JSON.parse(message);
      window.__windowActions.push(action);
      const finish = () => window.dispatchEvent(new CustomEvent('hivemind:host-response', {
        detail: { id, ok: window.__hostOutcome !== 'failure' }
      }));
      if (window.__hostOutcome === 'pending') window.__finishWindowAction = finish;
      else queueMicrotask(finish);
    } };
    ${source}
  ` });
  await page.route('**/*.js', (route) => route.fulfill({ status: 404, body: '' }));
  await page.goto(`http://127.0.0.1:${port}/`);
}

for (const [role, port] of [['Master', 4173], ['Worker', 4174]]) {
  test(`${role} native close presentation is coalesced, keyboard accessible and retryable`, async ({ page }, testInfo) => {
    const errors = [];
    page.on('pageerror', (error) => errors.push(error.message));
    await page.emulateMedia({ reducedMotion: 'reduce' });
    await page.setViewportSize({ width: 320, height: 568 });
    await page.addInitScript(() => {
      window.__windowActions = [];
      window.__hostOutcome = 'success';
      window.__HIVEMIND_DESKTOP__ = Object.freeze({
        ready: async () => {},
        requestClose: async () => window.dispatchEvent(new Event('hivemind:close-requested')),
        resolveClose: async (action) => {
          window.__windowActions.push(action);
          if (window.__hostOutcome === 'failure') throw new Error('Fixture host failure');
          if (window.__hostOutcome === 'pending') await new Promise((resolve) => { window.__finishWindowAction = resolve; });
        },
      });
    });
    await page.route('**/api/**', async (route) => {
      if (route.request().method() === 'OPTIONS') {
        await route.fulfill({ status: 204, headers: { 'access-control-allow-origin': '*', 'access-control-allow-methods': 'GET,POST,OPTIONS', 'access-control-allow-headers': '*' } });
        return;
      }
      await route.fulfill({ json: { state: 'ready', phase: 'ready' }, headers: { 'access-control-allow-origin': '*' } });
    });
    await page.goto(`http://127.0.0.1:${port}/`);
    const username = page.getByLabel('Username', { exact: true });
    await expect(username).toBeVisible();
    await username.focus();
    await page.evaluate(() => window.__HIVEMIND_DESKTOP__.requestClose());
    const dialog = page.getByRole('alertdialog');
    await expect(dialog).toBeVisible();
    await expect(dialog.getByRole('heading', { name: `Keep ${role} running?`, exact: true })).toBeVisible();
    await page.evaluate(() => window.dispatchEvent(new Event('hivemind:close-requested')));
    await expect(dialog).toHaveCount(1);
    for (let index = 0; index < 5; index += 1) {
      await page.keyboard.press('Tab');
      expect(await page.evaluate(() => !!document.activeElement?.closest('[role="alertdialog"]'))).toBe(true);
    }
    const bounds = await dialog.boundingBox();
    expect(bounds.x).toBeGreaterThanOrEqual(0);
    expect(bounds.y).toBeGreaterThanOrEqual(0);
    expect(bounds.x + bounds.width).toBeLessThanOrEqual(321);
    expect(bounds.y + bounds.height).toBeLessThanOrEqual(569);
    await testInfo.attach(`${role.toLowerCase()}-native-close-320.png`, { body: await page.screenshot({ animations: 'disabled' }), contentType: 'image/png' });
    await page.keyboard.press('Escape');
    await expect(dialog).toBeHidden();
    await expect(username).toBeFocused();
    expect(await page.evaluate(() => window.__windowActions)).toEqual(['cancel']);

    await page.evaluate(() => { window.__hostOutcome = 'failure'; window.dispatchEvent(new Event('hivemind:close-requested')); });
    await dialog.getByRole('button', { name: 'Quit', exact: true }).click();
    await expect(dialog.getByRole('alert')).toHaveText('Could not close the window. Try again.');
    await expect(dialog.getByRole('button', { name: 'Quit', exact: true })).toBeEnabled();
    expect(await page.evaluate(() => document.documentElement.dataset.windowState)).toBe('visible');

    await page.evaluate(() => { window.__hostOutcome = 'pending'; });
    await dialog.getByRole('button', { name: 'Keep in background', exact: true }).click();
    await expect(dialog.getByRole('button', { name: 'Quit', exact: true })).toBeDisabled();
    await page.evaluate(() => window.dispatchEvent(new Event('hivemind:close-requested')));
    await page.keyboard.press('Escape');
    await expect(dialog).toBeVisible();
    expect(await page.evaluate(() => window.__windowActions)).toEqual(['cancel', 'quit', 'background']);
    await page.evaluate(() => window.__finishWindowAction());
    await expect(dialog).toBeHidden();
    await expect(username).toBeFocused();
    await page.evaluate(() => window.dispatchEvent(new Event('hivemind:window-shown')));
    expect(await page.evaluate(() => document.documentElement.classList.contains('desktop-enter'))).toBe(true);
    expect(errors).toEqual([]);
  });

  test(`${role} failed-bundle fallback stays centered and readable despite global CSS`, async ({ page }, testInfo) => {
    await page.emulateMedia({ reducedMotion: 'reduce' });
    await loadNativeFallback(page, port);
    await expect(page.locator('html')).toHaveClass(/dark/);
    for (const [width, height] of [[1440, 900], [320, 360]]) {
      await page.setViewportSize({ width, height });
      for (const theme of ['light', 'dark']) {
        await page.evaluate((dark) => document.documentElement.classList.toggle('dark', dark), theme === 'dark');
        await expect.poll(() => page.evaluate(() => {
          const probe = document.createElement('span');
          probe.style.cssText = 'color:var(--foreground);background:var(--card)';
          document.body.append(probe);
          const expected = getComputedStyle(probe);
          const panel = document.querySelector('.fallback-card, .fallback-panel');
          const matches = getComputedStyle(document.querySelector('h1')).color === expected.color
            && getComputedStyle(panel).backgroundColor === expected.backgroundColor;
          probe.remove();
          return matches;
        })).toBe(true);
        await page.evaluate(() => window.dispatchEvent(new Event('hivemind:close-requested')));
        const dialog = page.getByRole('dialog', { name: 'Keep client running?', exact: true });
        await expect(dialog).toBeVisible();
        const bounds = await dialog.boundingBox();
        expect(Math.abs(bounds.x + bounds.width / 2 - width / 2)).toBeLessThan(2);
        expect(Math.abs(bounds.y + bounds.height / 2 - height / 2)).toBeLessThan(2);
        expect(bounds.width).toBeLessThanOrEqual(width - 32);
        expect(bounds.height).toBeLessThanOrEqual(height - 32);
        expect(await dialog.evaluate((element) => element.scrollWidth <= element.clientWidth)).toBe(true);
        await expect.poll(() => dialog.evaluate((element) => {
          const probe = document.createElement('span');
          probe.style.cssText = 'color:var(--foreground);background:var(--background)';
          document.body.append(probe);
          const expected = getComputedStyle(probe);
          const actual = getComputedStyle(element);
          const matches = actual.color === expected.color && actual.backgroundColor === expected.backgroundColor;
          probe.remove();
          return matches;
        })).toBe(true);
        await testInfo.attach(`${role.toLowerCase()}-native-fallback-${width}-${theme}.png`, {
          body: await page.screenshot(), contentType: 'image/png',
        });
        await page.keyboard.press('Escape');
        await expect(dialog).toBeHidden();
      }
    }
    expect(await page.evaluate(() => window.__windowActions)).toEqual(['cancel', 'cancel', 'cancel', 'cancel']);
  });

  test(`${role} fallback animates, prevents duplicate pending actions and recovers after failure`, async ({ page }) => {
    const errors = [];
    page.on('pageerror', (error) => errors.push(error.message));
    await page.emulateMedia({ reducedMotion: 'no-preference' });
    await loadNativeFallback(page, port);
    const entrance = await page.evaluate(() => {
      window.__hostOutcome = 'failure';
      window.dispatchEvent(new Event('hivemind:close-requested'));
      return document.getElementById('hivemind-close-fallback').getAnimations().map((animation) => animation.effect.getTiming().duration);
    });
    expect(entrance).toEqual([180]);
    const dialog = page.getByRole('dialog', { name: 'Keep client running?', exact: true });
    await dialog.evaluate(async (element) => Promise.allSettled(element.getAnimations().map((animation) => animation.finished)));
    await dialog.getByRole('button', { name: 'Quit', exact: true }).click();
    await expect(dialog.getByText('Could not close the window. Try again.', { exact: true })).toBeVisible();
    await expect(dialog.getByRole('button', { name: 'Quit', exact: true })).toBeEnabled();
    expect(await page.evaluate(() => getComputedStyle(document.body).opacity)).toBe('1');

    await page.evaluate(() => { window.__hostOutcome = 'pending'; });
    await dialog.getByRole('button', { name: 'Keep in background', exact: true }).click();
    await expect.poll(() => page.evaluate(() => window.__windowActions)).toEqual(['quit', 'background']);
    await expect(dialog.getByRole('button', { name: 'Cancel', exact: true })).toBeDisabled();
    await page.keyboard.press('Escape');
    await page.evaluate(() => window.dispatchEvent(new Event('hivemind:close-requested')));
    await expect(dialog).toHaveCount(1);
    expect(await page.evaluate(() => window.__windowActions)).toEqual(['quit', 'background']);
    await page.evaluate(() => window.__finishWindowAction());
    await expect(dialog).toBeHidden();
    expect(await page.evaluate(() => getComputedStyle(document.body).opacity)).toBe('1');

    await page.emulateMedia({ reducedMotion: 'reduce' });
    const reduced = await page.evaluate(() => {
      window.__hostOutcome = 'success';
      window.dispatchEvent(new Event('hivemind:close-requested'));
      return document.getElementById('hivemind-close-fallback').getAnimations().length;
    });
    expect(reduced).toBe(0);
    await dialog.getByRole('button', { name: 'Quit', exact: true }).click();
    await expect(dialog).toBeHidden();
    expect(await page.evaluate(() => window.__windowActions)).toEqual(['quit', 'background', 'quit']);
    expect(errors).toEqual([]);
  });
}
