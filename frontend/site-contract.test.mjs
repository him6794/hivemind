import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import { getSiteDefinition } from './src/lib/hivemind-site-data.mjs';

test('site definition exposes only official website and account-center routes', () => {
  const site = getSiteDefinition('en');
  const zhSite = getSiteDefinition('zh');
  const publicCopy = [
    site.brand.strap,
    site.hero.badge,
    site.hero.title,
    site.hero.body,
    ...site.hero.bullets,
    ...site.sections.stats.flatMap((item) => [item.value, item.label]),
    ...site.sections.features.flatMap((item) => [item.title, item.body]),
    ...site.sections.workflow.flatMap((item) => [item.title, item.body]),
    ...zhSite.hero.bullets,
    zhSite.brand.strap,
    zhSite.hero.badge,
    zhSite.hero.title,
    zhSite.hero.body,
    ...zhSite.sections.stats.flatMap((item) => [item.value, item.label]),
    ...zhSite.sections.features.flatMap((item) => [item.title, item.body]),
    ...zhSite.sections.workflow.flatMap((item) => [item.title, item.body]),
  ].join('\n');

  assert.equal(site.brand.name, 'Hivemind');
  assert.deepEqual(
    site.routes.map((route) => route.id),
    ['home', 'login', 'register', 'account', 'security', 'docs']
  );
  assert.match(site.hero.title, /compute|account|deploy/i);
  assert.ok(zhSite.hero.title.length > 8);
  assert.ok(zhSite.hero.body.length > 20);
  assert.doesNotMatch(publicCopy, /repository|repo|runtime|endpoint|JWT|gRPC|nodepool|master API|contract/i);
  assert.doesNotMatch(publicCopy, /\/api\/tasks|\/api\/workers|register-worker/i);
  assert.doesNotMatch(publicCopy, /嚙罵?徑?悴?院???/);
  assert.ok(site.sections.features.length >= 4);
});

test('browser API adapter stays on same-origin website-backend account endpoints', () => {
  const source = fs.readFileSync(new URL('./src/lib/hivemind-api.ts', import.meta.url), 'utf8');

  assert.match(source, /fetch\(`\$\{apiBase\}\$\{path\}`/);
  assert.doesNotMatch(source, /NEXT_PUBLIC_API_BASE/);
  assert.match(source, /\/api\/register/);
  assert.match(source, /\/api\/login/);
  assert.match(source, /\/api\/balance/);
  assert.doesNotMatch(source, /\/api\/tasks(\/|`|'|"|\b)/);
  assert.doesNotMatch(source, /\/api\/workers(\/|`|'|"|\b)/);
  assert.doesNotMatch(source, /register-worker/);
});

test('website backend only exposes account-center routes through user service', () => {
  const source = fs.readFileSync(new URL('./src/app/api/[...path]/route.ts', import.meta.url), 'utf8');

  assert.match(source, /UserService/);
  assert.match(source, /RegisterUser/);
  assert.match(source, /Login/);
  assert.match(source, /GetBalance/);
  assert.match(source, /WEBSITE_NODEPOOL_GRPC_ADDR/);
  assert.doesNotMatch(source, /process\.env\.NODEPOOL_GRPC_ADDR/);
  assert.doesNotMatch(source, /MasterNodeService|NodeManagerService/);
  assert.doesNotMatch(source, /\/tasks|\/workers|register-worker/);
  assert.doesNotMatch(source, /UploadTask|QuoteTask|StopTask|GetAllUserTasks|ListWorkers|RegisterWorkerNode/);
});

test('app shell does not wire official website navigation to master or worker operations', () => {
  const pageSource = fs.readFileSync(new URL('./src/app/page.tsx', import.meta.url), 'utf8');
  const storeSource = fs.readFileSync(new URL('./src/store/app-store.ts', import.meta.url), 'utf8');

  assert.doesNotMatch(pageSource, /DashboardPage|TasksPage|WorkersPage/);
  assert.doesNotMatch(pageSource, /"dashboard"|"tasks"|"workers"/);
  assert.doesNotMatch(storeSource, /"dashboard"|"tasks"|"workers"/);
});

test('top-level documentation describes official site as account center, not an operations console', () => {
  const readme = fs.readFileSync(new URL('../README.md', import.meta.url), 'utf8');

  assert.match(readme, /\*\*Official Site\*\* \(`frontend\/`\).*account center/i);
  assert.match(readme, /WEBSITE_NODEPOOL_GRPC_ADDR/);
  assert.match(readme, /Official Site\s+\|\s+8080\s+\|\s+Public product site and account center/i);
  assert.doesNotMatch(readme, /\*\*Official Site\*\* \(`frontend\/`\).*task submission/i);
  assert.doesNotMatch(readme, /Official Site\s+\|\s+8080\s+\|.*nginx/i);
});

test('official-site onboarding validates input, keeps bearer tokens in memory, and exposes logout', () => {
  const storeSource = fs.readFileSync(new URL('./src/store/app-store.ts', import.meta.url), 'utf8');
  const loginSource = fs.readFileSync(new URL('./src/components/pages/login-page.tsx', import.meta.url), 'utf8');
  const registerSource = fs.readFileSync(new URL('./src/components/pages/register-page.tsx', import.meta.url), 'utf8');
  const accountSource = fs.readFileSync(new URL('./src/components/pages/account-page.tsx', import.meta.url), 'utf8');
  const navbarSource = fs.readFileSync(new URL('./src/components/site/navbar.tsx', import.meta.url), 'utf8');

  assert.doesNotMatch(storeSource, /zustand\/middleware|persist\s*\(/);
  assert.match(storeSource, /clearLegacyAuthStorage/);
  assert.match(loginSource, /validateLoginInput/);
  assert.match(loginSource, /htmlFor="login-username"/);
  assert.match(loginSource, /htmlFor="login-password"/);
  assert.match(registerSource, /validateRegistrationInput/);
  assert.match(registerSource, /htmlFor="register-username"/);
  assert.match(registerSource, /htmlFor="register-confirm-password"/);
  assert.match(accountSource, /parseAccountBalance/);
  assert.match(navbarSource, /logout/);
});

test('release browser QA covers account, worker registration, and task lifecycle surfaces', () => {
  const packageJson = JSON.parse(fs.readFileSync(new URL('./package.json', import.meta.url), 'utf8'));
  const configSource = fs.readFileSync(new URL('./playwright.config.mjs', import.meta.url), 'utf8');
  const flowSource = fs.readFileSync(new URL('./e2e/release-flow.spec.mjs', import.meta.url), 'utf8');

  assert.equal(
    packageJson.scripts['test:e2e'],
    'node node_modules/@playwright/test/cli.js test',
  );
  assert.ok(packageJson.devDependencies['@playwright/test']);
  assert.match(configSource, /HIVEMIND_E2E_EVIDENCE_DIR/);
  assert.match(configSource, /msedge|chrome/);
  assert.match(flowSource, /official site/i);
  assert.match(flowSource, /Account Center/);
  assert.match(flowSource, /Worker UI/);
  assert.match(flowSource, /Login and register/);
  assert.match(flowSource, /Master UI/);
  assert.match(flowSource, /Upload Task/);
  assert.match(flowSource, /Log/);
  assert.match(flowSource, /Result/);
  assert.match(flowSource, /Download/);
  assert.match(flowSource, /Cancel/);
});
