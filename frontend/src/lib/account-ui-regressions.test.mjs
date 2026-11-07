import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import vm from 'node:vm';
import ts from 'typescript';
import { validateResetPasswordInput } from './auth-policy.mjs';

const require = createRequire(import.meta.url);

function loadTsModule(path, dependencies, extra = '') {
  const source = readFileSync(new URL(path, import.meta.url), 'utf8');
  const { outputText } = ts.transpileModule(source + extra, {
    compilerOptions: {
      module: ts.ModuleKind.CommonJS,
      target: ts.ScriptTarget.ES2022,
      jsx: ts.JsxEmit.ReactJSX,
    },
  });
  const module = { exports: {} };
  vm.runInNewContext(outputText, {
    module,
    exports: module.exports,
    require(name) {
      if (Object.hasOwn(dependencies, name)) return dependencies[name];
      if (name.startsWith('@/')) return {};
      return require(name);
    },
  }, { filename: path });
  return module.exports;
}

function loadStore(getAccount, logoutUser = async () => {}) {
  return loadTsModule('../store/app-store.ts', {
    '@/lib/hivemind-api': { getAccount, logoutUser },
    '@/lib/docs/structure.mjs': { DEFAULT_DOCS_SLUG: 'overview', docsSlugFrom: (value) => value },
    '@/lib/route-hash-policy.mjs': { TOKEN_ROUTE_IDS: ['verify-email', 'reset-password'] },
  }).useAppStore;
}

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

test('a stale session rejection cannot clear a newer successful sign-in', async () => {
  const response = deferred();
  const store = loadStore(() => response.promise);
  const restoring = store.getState().restoreSession();
  store.getState().setAuth({ username: 'new-session' });
  store.getState().navigate('account');
  response.reject(new Error('401'));
  await restoring;
  assert.equal(store.getState().user?.username, 'new-session');
  assert.equal(store.getState().route, 'account');
  assert.equal(store.getState().sessionChecked, true);
});

test('a stale successful restore cannot sign the visitor back in after logout', async () => {
  const response = deferred();
  const store = loadStore(() => response.promise);
  const restoring = store.getState().restoreSession();
  await store.getState().logout();
  response.resolve({ username: 'old-session' });
  await restoring;
  assert.equal(store.getState().user, null);
  assert.equal(store.getState().route, 'home');
  assert.equal(store.getState().sessionChecked, true);
});

test('only the most recent session restore may publish its result', async () => {
  const first = deferred();
  let calls = 0;
  const store = loadStore(() => ++calls === 1 ? first.promise : Promise.reject(new Error('401')));
  const older = store.getState().restoreSession();
  await store.getState().restoreSession();
  first.resolve({ username: 'old-session' });
  await older;
  assert.equal(store.getState().user, null);
});

function loadVerifier(verifyEmail) {
  return loadTsModule('../components/pages/verify-email-page.tsx', {
    '@/lib/hivemind-api': { verifyEmail },
  }, '\nexport { verificationFor };\n').verificationFor;
}

test('email verification retries a failed request but retains a successful single-use result', async () => {
  let calls = 0;
  const verificationFor = loadVerifier(async () => {
    calls += 1;
    if (calls === 1) throw new Error('503');
    return 'granted';
  });
  await assert.rejects(verificationFor('token'), /503/);
  assert.equal(await verificationFor('token'), 'granted');
  assert.equal(await verificationFor('token'), 'granted');
  assert.equal(calls, 2);
});

test('concurrent verification effects share the same pending request', async () => {
  const response = deferred();
  let calls = 0;
  const verificationFor = loadVerifier(() => { calls += 1; return response.promise; });
  const first = verificationFor('token');
  const second = verificationFor('token');
  assert.equal(first, second);
  response.resolve('granted');
  assert.equal(await first, 'granted');
  assert.equal(calls, 1);
});

test('reset-password fields defer length validation to the shared UTF-8 byte policy', () => {
  const source = readFileSync(new URL('../components/pages/reset-password-page.tsx', import.meta.url), 'utf8');
  assert.equal(validateResetPasswordInput('密碼測試', '密碼測試').ok, true);
  for (const id of ['reset-password-new', 'reset-password-confirm']) {
    const input = source.match(new RegExp(`<Input\\b[^>]*id="${id}"[^>]*(?:=>[^>]*)*>`));
    // The complete line also covers attributes after an arrow in onChange.
    const line = source.split('\n').find((line) => line.includes(`id="${id}"`));
    assert.ok(input && line, `missing ${id}`);
    assert.doesNotMatch(line, /minLength/);
  }
});
