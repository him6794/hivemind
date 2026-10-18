import test from 'node:test';
import assert from 'node:assert/strict';
import { LOCALES, createLocaleAdapter, errorMessage, localeError, localeTag, message, normalizeLocale, readLocale, saveLocale, t } from './locale.mjs';
import { taskListResponseError, taskResponseError } from './taskResponsePolicy.mjs';
import { testNativeLocaleAdapter } from '../../desktop/localeAdapter.contract.mjs';

testNativeLocaleAdapter(createLocaleAdapter);

const storageKey = 'hivemind.master.locale.v1';

test('English is the default and unsupported locale values fall back safely', () => {
  for (const value of [null, undefined, '', 'fr', 'zh-TW', '__proto__']) {
    assert.equal(normalizeLocale(value), 'en');
  }
  assert.equal(readLocale(null), 'en');
  assert.equal(localeTag('zh'), 'zh-TW');
  assert.deepEqual(LOCALES.map(({ code }) => code), ['en', 'zh']);
});

test('locale preferences use the Master namespace without changing other data', () => {
  const values = new Map([['hivemind.worker.locale.v1', 'en'], ['session', 'unchanged']]);
  const storage = { getItem: (key) => values.get(key), setItem: (key, value) => values.set(key, value) };
  saveLocale('zh', storage);
  assert.equal(values.get(storageKey), 'zh');
  assert.equal(readLocale(storage), 'zh');
  assert.equal(values.get('hivemind.worker.locale.v1'), 'en');
  assert.equal(values.get('session'), 'unchanged');
  saveLocale('unsupported', storage);
  assert.equal(readLocale(storage), 'en');
});

test('blocked preference storage never prevents rendering or changing language', () => {
  const storage = { getItem() { throw new Error('blocked'); }, setItem() { throw new Error('blocked'); } };
  assert.equal(readLocale(storage), 'en');
  assert.doesNotThrow(() => saveLocale('zh', storage));
  assert.doesNotThrow(() => saveLocale('zh', null));
});

test('messages resolve in either language and missing translations retain English', () => {
  assert.equal(t('en', 'Sign in'), 'Sign in');
  assert.equal(t('zh', 'Sign in'), '登入');
  assert.equal(t('fr', 'Sign in'), 'Sign in');
  assert.equal(t('zh', 'A future label'), 'A future label');
  assert.equal(t('zh', { en: 'Fallback' }), 'Fallback');
  assert.equal(t('zh', { en: 'Hello', zh: '你好' }), '你好');
  assert.equal(t('zh', '__proto__'), '__proto__');
});

test('message descriptors resolve at render time and retain zero values', () => {
  const entry = message('Charged {amount} CPT{note}', { amount: 0, note: message(' (fee included)') });
  assert.equal(t('en', entry), 'Charged 0 CPT (fee included)');
  assert.equal(t('zh', entry), '已收取 0 CPT（含平台費）');
  assert.equal(t('zh', message('Updated {seconds}s ago', { seconds: 0 })), '0 秒前更新');
  assert.equal(t('zh', '{missing}', {}), '{missing}');
});

test('raw logs, JSON, source and diagnostics are not interpreted as translation keys', () => {
  for (const raw of ['Sign in', 'FAILED', '{"text":"Sign in"}', 'let total = 0;\n總計', '<script>raw</script>']) {
    assert.equal(t('zh', { raw }), raw);
    assert.equal(t('zh', message('Login failed: {detail}', { detail: raw })), `登入失敗：${raw}`);
    assert.equal(t('zh', errorMessage(new Error(raw))), raw);
  }
});

test('UI errors remain translatable while Error.message stays suitable for diagnostics', () => {
  const error = localeError('Server error ({status}). Please try again later.', { status: 503 });
  assert.equal(error.message, 'Server error (503). Please try again later.');
  assert.equal(t('zh', errorMessage(error)), '伺服器錯誤（503），請稍後重試。');
  assert.equal(t('zh', errorMessage(null)), '任務請求失敗');
});

test('response errors distinguish local fallbacks from server-owned diagnostics', () => {
  assert.equal(taskResponseError({ success: true }), null);
  assert.equal(taskListResponseError({ success: true, tasks: [] }), null);
  assert.equal(t('zh', errorMessage(taskResponseError({}, 'Log unavailable'))), '無法取得日誌');
  assert.equal(t('zh', errorMessage(taskListResponseError({ success: true, tasks: null }))), '任務清單回應無效');
  for (const field of ['message', 'status_message', 'log']) {
    assert.equal(t('zh', errorMessage(taskResponseError({ [field]: 'Log unavailable' }, 'Result unavailable'))), 'Log unavailable');
  }
  assert.equal(t('zh', errorMessage(taskListResponseError({ message: 'Failed to load tasks' }))), 'Failed to load tasks');
  assert.ok(taskResponseError({ success: true }, 'Log unavailable', false));
});
