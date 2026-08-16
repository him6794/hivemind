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
    ['home', 'login', 'register', 'account', 'security', 'docs', 'terms']
  );
  // The point of pinning the route list: the official site is a product and
  // account surface, so an operations console must never appear in it.
  for (const forbidden of ['dashboard', 'tasks', 'workers', 'nodes']) {
    assert.ok(
      !site.routes.some((route) => route.id === forbidden),
      `official site must not expose a ${forbidden} route`
    );
  }
  assert.match(site.hero.title, /compute|account|deploy/i);
  assert.ok(zhSite.hero.title.length > 8);
  assert.ok(zhSite.hero.body.length > 20);
  assert.doesNotMatch(publicCopy, /repository|repo|runtime|endpoint|JWT|gRPC|nodepool|master API|contract/i);
  assert.doesNotMatch(publicCopy, /\/api\/tasks|\/api\/workers|register-worker/i);
  assert.doesNotMatch(publicCopy, /嚙罵?徑?悴?院???/);
  assert.ok(site.sections.features.length >= 4);
});

test('public copy explains cross-account worker scheduling and prices every function form', () => {
  for (const locale of ['en', 'zh']) {
    const site = getSiteDefinition(locale);
    const featureCopy = site.sections.features.map((feature) => `${feature.title}\n${feature.body}`).join('\n');
    const quickstartCopy = site.sections.docs.quickstart
      .map((item) => `${item.title}\n${item.body}`)
      .join('\n');
    const termsCopy = site.sections.terms.groups
      .flatMap((group) => [group.title, ...group.items])
      .join('\n');
    const publicCopy = `${featureCopy}\n${quickstartCopy}\n${termsCopy}`;

    if (locale === 'en') {
      assert.match(publicCopy, /another (Hivemind )?user/i);
      assert.match(publicCopy, /not (a )?guaranteed destination|not necessarily your own/i);
    } else {
      assert.match(publicCopy, /其他.*(使用者|用戶)/);
      assert.match(publicCopy, /不保證.*(自己|自有).*worker/i);
    }

    const rows = site.sections.docs.billing.functionRows;
    assert.ok(rows, `${locale} billing must publish function-level pricing`);
    assert.deepEqual(
      rows.map((row) => row.id),
      ['len', 'get', 'contains', 'user-function', 'print'],
      `${locale} billing must cover every supported function form`
    );
    for (const row of rows) {
      assert.match(row.price, /6 CPT|6 usage units|6 點|6 單位/i, `${locale} ${row.id} needs its call price`);
      assert.ok(row.note.length > 20, `${locale} ${row.id} needs a pricing explanation`);
    }
  }
});

test('documentation page renders the function-level pricing table', () => {
  const docsSource = fs.readFileSync(
    new URL('./src/components/pages/docs-page.tsx', import.meta.url),
    'utf8'
  );

  assert.match(docsSource, /docs\.billing\.functionRows\.map/);
  assert.match(docsSource, /Function-level pricing/);
  assert.match(docsSource, /函式級計價/);
  assert.match(docsSource, /row\.price/);
  assert.match(docsSource, /row\.note/);
  assert.match(docsSource, /docs\.billing\.examples\.map/);
  assert.match(docsSource, /docs\.billing\.platforms\.map/);
});

test('billing docs publish receipt-backed examples and a platform support matrix', () => {
  for (const locale of ['en', 'zh']) {
    const docs = getSiteDefinition(locale).sections.docs;
    const examples = docs.billing.examples;

    assert.ok(examples.length >= 3, `${locale} billing needs worked receipt examples`);
    for (const example of examples) {
      assert.ok(example.program.length > 10, `${locale} ${example.id} needs the program`);
      assert.ok(example.receiptUsageUnits > 0, `${locale} ${example.id} needs receipt usage`);
      assert.equal(
        example.totalCpt,
        example.receiptUsageUnits + 1,
        `${locale} ${example.id} must apply the one-CPT invocation charge`
      );
      assert.ok(example.breakdown.length > 20, `${locale} ${example.id} needs a billing breakdown`);
    }

    assert.deepEqual(
      docs.billing.platforms.map((platform) => platform.id),
      ['linux', 'macos', 'wsl', 'windows-native'],
      `${locale} docs must publish all supported host paths`
    );
    assert.ok(
      docs.billing.platforms.find((platform) => platform.id === 'windows-native').proof.includes('fail'),
      `${locale} docs must disclose native Windows proof behavior`
    );
  }
});

test('task observability is exposed by the trusted API and rendered by Master UI', () => {
  const proto = fs.readFileSync(new URL('../proto/hivemind.proto', import.meta.url), 'utf8');
  const nodeManager = fs.readFileSync(
    new URL('../hivemind-rs/crates/node-manager/src/grpc.rs', import.meta.url),
    'utf8'
  );
  const masterApi = fs.readFileSync(
    new URL('../hivemind-rs/crates/master-api/src/handlers.rs', import.meta.url),
    'utf8'
  );
  const masterUi = fs.readFileSync(new URL('./master-ui/src/App.jsx', import.meta.url), 'utf8');

  for (const field of ['worker_id', 'provider_user', 'dispatch_status', 'usage_units', 'max_cpt']) {
    assert.match(proto, new RegExp(`\\b${field}\\b`), `proto must expose ${field}`);
    assert.match(nodeManager, new RegExp(`\\b${field}\\b`), `nodepool must populate ${field}`);
    assert.match(masterApi, new RegExp(`\\b${field}\\b`), `Master API must forward ${field}`);
    assert.match(masterUi, new RegExp(field), `Master UI must render ${field}`);
  }
  assert.match(masterUi, /Final charge|Billed|結算/);
  assert.match(masterUi, /Provider|provider_user/);
  assert.match(masterUi, /Redispatch|dispatch_status/);
});

// Documentation drifts away from the system silently: nothing breaks when a
// published limit stops matching the one that is actually enforced. Read the
// real constants and compare, so the docs page cannot quietly become fiction.
function evaluateRustLiteral(raw, name) {
  const value = raw
    .replace(/_/g, '')
    .split('*')
    .map((part) => Number(part.trim()))
    .reduce((product, part) => product * part, 1);
  assert.ok(Number.isFinite(value) && value > 0, `could not evaluate ${name}`);
  return value;
}

/** `pub const NAME: usize = 64 * 1024;` */
function readRustConst(source, name) {
  const match = source.match(new RegExp(`${name}\\s*:\\s*[a-z0-9]+\\s*=\\s*([0-9_ *]+?)\\s*;`));
  assert.ok(match, `could not read const ${name} from the Rust source`);
  return evaluateRustLiteral(match[1], name);
}

/** `max_ops: 1_000_000,` inside a struct literal */
function readRustField(block, name) {
  const match = block.match(new RegExp(`\\b${name}\\s*:\\s*([0-9_][0-9_ *]*?)\\s*,`));
  assert.ok(match, `could not read field ${name} from the Rust source`);
  return evaluateRustLiteral(match[1], name);
}

test('published limits mirror the constants the system actually enforces', () => {
  const proto = fs.readFileSync(
    new URL('../hivemind-rs/crates/proto/src/lib.rs', import.meta.url),
    'utf8'
  );
  const runtime = fs.readFileSync(
    new URL('../executor-rs/crates/managed-function-runtime/src/lib.rs', import.meta.url),
    'utf8'
  );

  const defaults = runtime.match(/impl Default for ExecutionLimits \{[\s\S]*?\n\}/);
  assert.ok(defaults, 'could not locate ExecutionLimits::default()');
  const defaultBlock = defaults[0];

  // Checked per row, not against the whole list: two limits that happen to
  // share a number would otherwise hide each other's drift.
  const expected = {
    taskId: readRustConst(proto, 'TASK_ID_MAX_BYTES'),
    taskSource: readRustConst(proto, 'MANAGED_TASK_SOURCE_MAX_BYTES'),
    jsonInput: readRustConst(proto, 'MANAGED_JSON_INPUT_MAX_BYTES'),
    budget: readRustConst(proto, 'MANAGED_BUDGET_MAX_USAGE_UNITS'),
    ops: readRustField(defaultBlock, 'max_ops'),
    callDepth: readRustField(defaultBlock, 'max_call_depth'),
    output: readRustField(defaultBlock, 'max_output_bytes'),
    loops: readRustField(defaultBlock, 'max_loop_iterations'),
    items: readRustField(defaultBlock, 'max_collection_items'),
    valueDepth: readRustField(defaultBlock, 'max_value_depth'),
  };

  for (const locale of ['en', 'zh']) {
    const limits = getSiteDefinition(locale).sections.docs.limits;
    for (const [id, value] of Object.entries(expected)) {
      const row = limits.find((limit) => limit.id === id);
      assert.ok(row, `${locale} docs must publish a limit row for '${id}'`);
      assert.ok(
        row.value.includes(value.toLocaleString('en-US')),
        `${locale} docs publish '${id}' as "${row.value}", but the system enforces ${value.toLocaleString('en-US')}`
      );
    }
  }
});

test('published failure codes are codes the runtime can actually return', () => {
  const runtime = fs.readFileSync(
    new URL('../executor-rs/crates/managed-function-runtime/src/lib.rs', import.meta.url),
    'utf8'
  );
  const realCodes = new Set(
    [...runtime.matchAll(/RuntimeError::new\(\s*"([a-z_]+)"/g)].map((match) => match[1])
  );
  assert.ok(realCodes.size > 5, 'expected to find the runtime failure codes');

  for (const locale of ['en', 'zh']) {
    for (const failure of getSiteDefinition(locale).sections.docs.failures) {
      assert.ok(
        realCodes.has(failure.code),
        `${locale} docs publish failure code '${failure.code}' that the runtime never returns`
      );
    }
  }
});

test('usage rules state the CPT limitation the product documentation records', () => {
  const limitations = fs.readFileSync(
    new URL('../docs/PUBLIC_NETWORK_LIMITATIONS.md', import.meta.url),
    'utf8'
  );
  assert.match(limitations, /CPT as an internal quota\/budget unit only/i);

  const en = getSiteDefinition('en').sections.terms;
  const zh = getSiteDefinition('zh').sections.terms;
  const enText = [en.summary, ...en.groups.flatMap((group) => [group.title, ...group.items])].join('\n');
  const zhText = [zh.summary, ...zh.groups.flatMap((group) => [group.title, ...group.items])].join('\n');

  assert.match(enText, /not money/i);
  assert.match(enText, /no conversion/i);
  assert.match(enText, /no dispute resolution/i);
  assert.match(enText, /service level agreement/i);
  assert.ok(zhText.includes('不是貨幣'));
  assert.ok(zhText.includes('沒有'));

  // The trust page has to keep admitting what verification does not cover.
  // These are the gaps an operator would otherwise discover the hard way.
  for (const locale of ['en', 'zh']) {
    const caveats = getSiteDefinition(locale).sections.security.caveats.join('\n');
    assert.match(
      caveats,
      /self-declared|自行申報/,
      `${locale} trust page must disclose that worker capability numbers are self-declared`
    );
    assert.match(
      caveats,
      /Windows/,
      `${locale} trust page must disclose that a native Windows worker cannot prove`
    );
  }

  // CPT is quota, not money, so the public copy must never sell it as earnings.
  // The marketplace those words imply is not built yet either.
  const publicCopyFor = (locale) => {
    const site = getSiteDefinition(locale);
    return [
      site.brand.strap,
      site.hero.title,
      site.hero.body,
      ...site.hero.bullets,
      ...site.sections.stats.flatMap((item) => [item.value, item.label]),
      ...site.sections.features.flatMap((item) => [item.title, item.body]),
      ...site.sections.workflow.flatMap((item) => [item.title, item.body]),
    ].join('\n');
  };
  assert.doesNotMatch(publicCopyFor('en'), /\b(income|profit|payout|cash|revenue)\b/i);
  for (const word of ['賺', '收入', '獲利', '金錢']) {
    assert.ok(
      !publicCopyFor('zh').includes(word),
      `zh public copy must not present CPT as money via '${word}'`
    );
  }
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
  assert.match(flowSource, /Submit Task/);
  assert.match(flowSource, /Log/);
  assert.match(flowSource, /Result/);
  assert.match(flowSource, /Download/);
  assert.match(flowSource, /Cancel/);
  assert.match(flowSource, /getByLabel\('CPU score'\)\.fill\('1201'\)/);
  assert.match(flowSource, /getByLabel\('Max CPT'\)\.fill\('200'\)/);
  assert.match(flowSource, /getByLabel\('Max CPT'\)\.fill\('100'\)/);
  assert.doesNotMatch(flowSource, /getByLabel\('Max CPT'\)\.fill\('1000000'\)/);
});
