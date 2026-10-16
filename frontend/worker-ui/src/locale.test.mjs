import test from 'node:test';
import assert from 'node:assert/strict';
import { LOCALES, createFormatters, errorMessage, localeError, localeTag, message, normalizeLocale, readLocale, saveLocale, t } from './locale.mjs';
import { getWorkerSetupErrorMessage, WorkerFetchTimeoutError, WorkerLoginError, WorkerStartupError } from './workerStartup.mjs';
import { PROVIDER_CREDITS_DETAIL, PROVIDER_CREDITS_LABEL } from './workerDashboard.mjs';

test('English is the default and only supported locales are accepted', () => {
  for (const value of [null, undefined, '', 'fr', 'zh-TW', '__proto__']) {
    assert.equal(normalizeLocale(value), 'en');
  }
  assert.equal(readLocale(null), 'en');
  assert.equal(localeTag('zh'), 'zh-TW');
  assert.deepEqual(LOCALES.map(({ code }) => code), ['en', 'zh']);
});

test('Worker preferences never modify Master locale or session data', () => {
  const values = new Map([['hivemind.master.locale.v1', 'en'], ['session', 'unchanged']]);
  const storage = { getItem: (key) => values.get(key), setItem: (key, value) => values.set(key, value) };
  saveLocale('zh', storage);
  assert.equal(values.get('hivemind.worker.locale.v1'), 'zh');
  assert.equal(readLocale(storage), 'zh');
  assert.equal(values.get('hivemind.master.locale.v1'), 'en');
  assert.equal(values.get('session'), 'unchanged');
  saveLocale('unsupported', storage);
  assert.equal(readLocale(storage), 'en');
});

test('blocked preference storage does not prevent rendering', () => {
  const storage = { getItem() { throw new Error('blocked'); }, setItem() { throw new Error('blocked'); } };
  assert.equal(readLocale(storage), 'en');
  assert.doesNotThrow(() => saveLocale('zh', storage));
  assert.doesNotThrow(() => saveLocale('zh', null));
});

test('translation fallback and deferred interpolation preserve zero and raw strings', () => {
  assert.equal(t('en', 'Worker console'), 'Worker console');
  assert.equal(t('zh', 'Worker console'), 'Worker 控制台');
  assert.equal(t('fr', 'Worker console'), 'Worker console');
  assert.equal(t('zh', 'Future label'), 'Future label');
  assert.equal(t('zh', { en: 'Fallback' }), 'Fallback');
  assert.equal(t('zh', { en: 'Hello', zh: '你好' }), '你好');
  assert.equal(t('zh', '__proto__'), '__proto__');
  const entry = message('Sampled {time}', { time: message('Unknown') });
  assert.equal(t('en', entry), 'Sampled Unknown');
  assert.equal(t('zh', entry), '取樣時間 未知');
  assert.equal(t('zh', message('{count} listed', { count: 0 })), '列出 0 項');
  assert.equal(t('zh', '{missing}', {}), '{missing}');
  assert.equal(t('zh', message('{suite} · measured locally', { suite: 'Unknown' })), 'Unknown · 本機實測');
});

test('diagnostics and identifiers are never looked up as UI messages', () => {
  for (const raw of ['Unknown', 'RUNNING', 'Worker console', '{"name":"Score"}', 'raw <script>']) {
    assert.equal(t('zh', { raw }), raw);
    assert.equal(t('zh', errorMessage(new Error(raw))), raw);
  }
  assert.equal(t('zh', errorMessage(null)), '請求失敗');
  assert.equal(t('zh', errorMessage(localeError('Benchmark request failed ({status})', { status: 503 }))), '效能測試請求失敗（503）');
});

test('metrics keep unavailable distinct from a measured zero in both languages', () => {
  for (const locale of ['en', 'zh']) {
    const { formatMetric } = createFormatters(locale);
    for (const value of [null, undefined, NaN, Infinity]) assert.equal(formatMetric(value, '%'), t(locale, 'Unknown'));
    assert.equal(formatMetric(0, '%'), '0 %');
    assert.equal(formatMetric(0, 'CPT', 2), '0 CPT');
    assert.equal(formatMetric(0, 'cores', 0), locale === 'zh' ? '0 核心' : '0 cores');
    assert.equal(formatMetric(123.45, 'CPT', 2), '123.45 CPT');
  }
});

test('timestamps follow the selected locale and retain invalid raw values', () => {
  const value = '2026-09-30T12:00:00.000Z';
  for (const locale of ['en', 'zh']) {
    const { formatTimestamp } = createFormatters(locale);
    assert.equal(formatTimestamp(null), t(locale, 'Unknown'));
    assert.equal(formatTimestamp('raw timestamp'), 'raw timestamp');
    assert.equal(formatTimestamp(value), new Intl.DateTimeFormat(localeTag(locale), { dateStyle: 'medium', timeStyle: 'short' }).format(new Date(value)));
  }
});

test('startup and setup errors stay translatable without changing control-flow types', () => {
  const timeout = new WorkerFetchTimeoutError();
  assert.equal(t('zh', errorMessage(timeout)), '本機 Worker 請求逾時。');
  const startup = new WorkerStartupError('failed', 'raw_failure_code');
  assert.equal(startup.code, 'raw_failure_code');
  assert.equal(t('zh', errorMessage(startup)), 'Worker 服務無法啟動。');
  const scenarios = [
    [timeout, { kind: 'login' }],
    [timeout, { kind: 'login', loginEstablished: true }],
    [new Error('raw'), { kind: 'restore' }],
    [new Error('raw'), { kind: 'login', loginEstablished: true }],
    ...[401, 403, 429, 500].map((status) => [new WorkerLoginError('response', status), { kind: 'login' }]),
    [new WorkerLoginError('network'), { kind: 'login' }],
    [new WorkerLoginError('invalid-response'), { kind: 'login' }],
    [new Error('raw'), { kind: 'login' }],
  ];
  for (const [error, options] of scenarios) {
    const english = getWorkerSetupErrorMessage(error, options);
    assert.equal(t('en', english), english);
    assert.notEqual(t('zh', english), english);
  }
  for (const label of [PROVIDER_CREDITS_LABEL, PROVIDER_CREDITS_DETAIL]) assert.notEqual(t('zh', label), label);
});
