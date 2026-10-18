import assert from 'node:assert/strict';
import { spawn, execFile } from 'node:child_process';
import { generateKeyPairSync } from 'node:crypto';
import { createServer as createHttpServer } from 'node:http';
import { createServer, connect } from 'node:net';
import { cp, mkdir, mkdtemp, readFile, rename, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { promisify } from 'node:util';
import { nativeTestEnvironment, until, windowAction } from './native-ui-smoke.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const evidence = path.resolve(root, process.env.HIVEMIND_NATIVE_EVIDENCE_DIR || 'test_logs/native-locale');
const exec = promisify(execFile);
const safeKeys = new Set(['systemroot', 'windir', 'comspec', 'path', 'temp', 'tmp', 'userprofile', 'appdata', 'programdata', 'programfiles', 'programfiles(x86)', 'number_of_processors', 'processor_architecture']);
const baseEnv = Object.fromEntries(Object.entries(process.env).filter(([key]) => safeKeys.has(key.toLowerCase())));
const owned = [];
const signIn = (role, locale) => locale === 'zh' ? role === 'master' ? '登入' : '登入並連線' : role === 'master' ? 'Sign in' : 'Sign in and connect';
const preference = (env, role, port) => path.join(env.LOCALAPPDATA, 'Hivemind', 'ui-preferences', role, String(port), 'locale.v1');
const bundle = (role) => {
  const value = process.env[`HIVEMIND_NATIVE_${role.toUpperCase()}_BUNDLE`];
  assert.ok(value, `Set HIVEMIND_NATIVE_${role.toUpperCase()}_BUNDLE to a fresh complete bundle`);
  return path.resolve(value);
};

async function freePort() {
  const server = createServer();
  await new Promise((resolve, reject) => { server.once('error', reject); server.listen(0, '127.0.0.1', resolve); });
  const port = server.address().port;
  await new Promise((resolve) => server.close(resolve));
  return port;
}
async function listening(port) {
  return new Promise((resolve) => {
    const socket = connect({ host: '127.0.0.1', port });
    const finish = (value) => { socket.destroy(); resolve(value); };
    socket.once('connect', () => finish(true));
    socket.once('error', () => finish(false));
    socket.setTimeout(1000, () => finish(false));
  });
}
async function helperFor(parent, role) {
  const { stdout } = await exec('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command',
    `ConvertTo-Json -Compress -InputObject @(Get-CimInstance Win32_Process -Filter "ParentProcessId = ${parent.pid}" | Where-Object { $_.Name -eq 'hivemind-${role}-ui.exe' } | Select-Object -ExpandProperty ProcessId)`,
  ], { timeout: 15_000 });
  const helpers = JSON.parse(stdout.trim());
  assert.ok(helpers.length <= 1);
  return helpers.length ? { pid: helpers[0] } : null;
}
async function text(helper, label) {
  await until(async () => {
    try { return (await windowAction(helper, 'inspect')).controls.some((control) => !control.offscreen && control.name === label); }
    catch { return false; }
  }, label);
}
async function control(helper, args) {
  const state = await windowAction(helper, 'state');
  await exec('powershell.exe', ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', path.join(root, 'scripts/native-ui-locale-control.ps1'),
    '-ClientProcessId', String(helper.pid), '-WindowHandle', String(state.windowHandle), ...args,
  ], { timeout: 30_000 });
}
const select = (helper, sequence) => control(helper, ['-Sequence', sequence]);
const press = (helper, label) => control(helper, ['-Action', 'press', '-Button', label]);
function start(executable, args, cwd, env) {
  const child = spawn(executable, args, { cwd, env, stdio: ['pipe', 'pipe', 'pipe'], windowsHide: true });
  const entry = { child, executable, output: '', spawnError: null };
  child.stdout.on('data', (chunk) => { entry.output += chunk; });
  child.stderr.on('data', (chunk) => { entry.output += chunk; });
  child.on('error', (error) => { entry.spawnError = error.message; });
  owned.push(entry);
  return child;
}
async function launch(role, directory, env, port, upstreamPort, locale) {
  const endpoint = `http://127.0.0.1:${upstreamPort}`;
  const child = start(path.join(directory, `hivemind-${role}.exe`), [], directory, {
    ...env, NODEPOOL_GRPC_ENDPOINT: endpoint, NODEPOOL_GRPC_ADDR: `127.0.0.1:${upstreamPort}`,
    MASTER_WEBSITE_API_BASE: endpoint, WORKER_WEBSITE_API_BASE: endpoint, WEBSITE_API_BASE: endpoint,
    MASTER_HTTP_ADDR: `127.0.0.1:${port}`, WORKER_CONTROL_HTTP_ADDR: `127.0.0.1:${port}`, WORKER_GRPC_ADDR: '127.0.0.1:0',
    MASTER_UI_DIR: path.join(directory, 'master-ui'), WORKER_UI_DIR: path.join(directory, 'worker-ui'),
    WORKER_EXECUTION_PUBLIC_KEY_PEM: generateKeyPairSync('ed25519').publicKey.export({ type: 'spki', format: 'pem' }),
    WORKER_ID: 'native-locale-fixture', HIVEMIND_VPN_STATE_ROOT: directory,
    TORRENT_API_DIR: path.join(directory, 'api'), TORRENT_BT_DIR: path.join(directory, 'bt'), UPDATE_ENABLED: 'false', RUST_LOG: 'info',
  });
  let helper;
  await until(async () => {
    assert.equal(child.exitCode, null, `${role} exited during startup`);
    try {
      const response = await fetch(`http://127.0.0.1:${port}/api/startup-status`, { signal: AbortSignal.timeout(1000) });
      if ((await response.json()).state !== 'ready') return false;
      helper = await helperFor(child, role);
      return !!helper;
    } catch { return false; }
  }, `${role} native startup`, 90_000);
  await text(helper, signIn(role, locale));
  return { child, helper, role, port };
}
async function quit(client, locale, alreadyOpen = false) {
  if (!alreadyOpen) await windowAction(client.helper, 'close');
  const label = locale === 'zh' ? '結束' : 'Quit';
  await text(client.helper, label);
  await press(client.helper, label);
  await until(() => client.child.exitCode !== null, 'explicit Quit exits backend', 15_000);
  assert.equal(client.child.exitCode, 0);
  await until(async () => !await listening(client.port), 'Quit closes the local listener');
  await until(async () => !await helperFor(client.child, client.role), 'Quit closes the helper');
}
async function exactPreference(env, role, port, value) {
  assert.deepEqual(await readFile(preference(env, role, port)), Buffer.from(value));
}
async function copyBundle(role, directory) {
  await cp(bundle(role), directory, { recursive: true });
  await writeFile(path.join(directory, '.env'), '# isolated native locale test\n', { flag: 'wx' });
}

async function runRole(role) {
  const directory = await mkdtemp(path.join(os.tmpdir(), `hivemind-native-locale-${role}-`));
  const env = await nativeTestEnvironment(baseEnv);
  const out = path.join(evidence, role);
  await mkdir(out);
  let location = path.join(directory, 'original');
  await copyBundle(role, location);
  const port = await freePort();
  const ports = new Set([port]);
  let upstreamConnections = 0;
  const upstream = createServer((socket) => { upstreamConnections++; socket.destroy(); });
  await new Promise((resolve, reject) => { upstream.once('error', reject); upstream.listen(0, '127.0.0.1', resolve); });
  const upstreamPort = upstream.address().port;
  const result = { role, bundle: bundle(role), directory, localAppData: env.LOCALAPPDATA, port, checks: [], launches: [] };
  let current;
  const launchHere = async (locale, clientRole = role, clientPort = port, clientLocation = location, clientEnv = env) => {
    current = await launch(clientRole, clientLocation, clientEnv, clientPort, upstreamPort, locale);
    ports.add(clientPort);
    result.launches.push({ role: clientRole, port: clientPort, directory: clientLocation, backendPid: current.child.pid, helperPid: current.helper.pid, locale });
    return current;
  };
  const check = (label) => { result.checks.push(label); console.log(`PASS ${role}: ${label}`); };
  const capture = (name) => windowAction(current.helper, 'screenshot', ['-Screenshot', path.join(out, `${name}.png`)]);
  try {
    await launchHere('en');
    await assert.rejects(readFile(preference(env, role, port)), { code: 'ENOENT' });
    await select(current.helper, '1');
    await text(current.helper, signIn(role, 'zh'));
    await capture('selected-zh');
    await quit(current, 'zh');
    await exactPreference(env, role, port, 'zh');
    await launchHere('zh');
    await capture('restarted-zh');
    check('Chinese persists through complete Quit and same-directory same-port restart');
    await select(current.helper, '0');
    await quit(current, 'en');
    await exactPreference(env, role, port, 'en');
    await launchHere('en');
    await capture('restarted-en');
    check('English persists after switching back and completely restarting');
    await select(current.helper, '1,0,1');
    await quit(current, 'zh');
    await exactPreference(env, role, port, 'zh');
    check('rapid selections followed by Quit preserve the final selection');

    const moved = path.join(directory, 'moved');
    await rename(location, moved);
    location = moved;
    await launchHere('zh');
    await capture('moved-directory-zh');
    await quit(current, 'zh');
    check('moving the extracted bundle does not change the language preference');

    const otherPort = await freePort();
    assert.notEqual(otherPort, port);
    await launchHere('en', role, otherPort);
    await assert.rejects(readFile(preference(env, role, otherPort)), { code: 'ENOENT' });
    await quit(current, 'en');
    await exactPreference(env, role, port, 'zh');
    check('another UI port defaults to English without overwriting the original preference');

    const otherRole = role === 'master' ? 'worker' : 'master';
    const otherLocation = path.join(directory, otherRole);
    await copyBundle(otherRole, otherLocation);
    await launchHere('en', otherRole, port, otherLocation);
    await assert.rejects(readFile(preference(env, otherRole, port)), { code: 'ENOENT' });
    await select(current.helper, '1,0');
    await quit(current, 'en');
    await exactPreference(env, otherRole, port, 'en');
    await exactPreference(env, role, port, 'zh');
    await launchHere('zh');
    await quit(current, 'zh');
    check('Master and Worker preferences remain independent even at the same port and root');

    const failedEnv = await nativeTestEnvironment(baseEnv);
    await mkdir(preference(failedEnv, role, port), { recursive: true });
    await launchHere('en', role, port, location, failedEnv);
    await text(current.helper, 'Could not read the language preference. Using English.');
    await select(current.helper, '1');
    await text(current.helper, '語言已切換，但無法確認是否已儲存。');
    await windowAction(current.helper, 'close');
    await text(current.helper, '結束');
    await press(current.helper, '結束');
    await text(current.helper, '語言可能未儲存，再按結束可離開。');
    assert.equal(current.child.exitCode, null);
    await capture('failed-save-quit-warning');
    await quit(current, 'zh', true);
    check('real filesystem failure is visible; first Quit warns and second explicit Quit exits');
    assert.equal(upstreamConnections, 0);
    check('no account login, live Nodepool connection, registration or task used');
    result.success = true;
  } catch (error) {
    result.success = false;
    result.failure = error.stack || String(error);
    if (current?.child.exitCode === null) await capture('failure').catch(() => {});
    throw error;
  } finally {
    for (const entry of owned) {
      if (entry.child.exitCode === null && entry.child.pid) {
        await exec('taskkill.exe', ['/PID', String(entry.child.pid), '/T', '/F'], { timeout: 15_000 }).catch(() => {});
      }
    }
    result.residualPorts = [];
    for (const tested of ports) if (await listening(tested)) result.residualPorts.push(tested);
    if (result.residualPorts.length) { result.success = false; process.exitCode = 1; }
    result.upstreamConnections = upstreamConnections;
    await writeFile(path.join(out, 'result.json'), JSON.stringify(result, null, 2));
    await new Promise((resolve) => upstream.close(resolve));
  }
  return result;
}

// This same-origin helper fixture seeds synthetic tokens first. It is separate
// from the real-bundle persistence checks above and never contacts Nodepool.
async function storageIsolation(role) {
  const directory = await mkdtemp(path.join(os.tmpdir(), `hivemind-native-storage-${role}-`));
  await copyBundle(role, directory);
  const env = await nativeTestEnvironment(baseEnv);
  const reports = [];
  let seed = true;
  const server = createHttpServer((request, response) => {
    if (request.url === '/report' && request.method === 'POST') {
      let body = '';
      request.on('data', (chunk) => { body += chunk; if (body.length > 2048) request.destroy(); });
      request.on('end', () => { reports.push(JSON.parse(body)); response.writeHead(204); response.end(); });
      return;
    }
    response.writeHead(200, { 'content-type': 'text/html; charset=utf-8' });
    response.end(`<!doctype html><html><body><h1>Native storage fixture</h1><p id="status">Loading</p><script>
(async () => {
  const key = 'hivemind.${role}.session.v1';
  const before = { session: sessionStorage.getItem(key), local: localStorage.getItem('synthetic-session-marker') };
  const language = await window.__HIVEMIND_DESKTOP__.readLocale();
  if (${seed}) {
    sessionStorage.setItem(key, JSON.stringify({ token: 'synthetic-native-test-token', username: 'test-only' }));
    localStorage.setItem('synthetic-session-marker', 'synthetic-native-test-token');
    await window.__HIVEMIND_DESKTOP__.writeLocale('zh');
  }
  const after = { session: Boolean(sessionStorage.getItem(key)), local: Boolean(localStorage.getItem('synthetic-session-marker')) };
  document.getElementById('status').textContent = ${seed} ? 'Synthetic session seeded' : 'Restart storage checked';
  await fetch('/report', { method: 'POST', body: JSON.stringify({ before, after, language }) });
})().catch((error) => { document.getElementById('status').textContent = error.message; });
</script></body></html>`);
  });
  await new Promise((resolve, reject) => { server.once('error', reject); server.listen(0, '127.0.0.1', resolve); });
  const port = server.address().port;
  try {
    for (let iteration = 0; iteration < 2; iteration++) {
      const child = start(path.join(directory, `hivemind-${role}-ui.exe`), [`http://127.0.0.1:${port}/`], directory, env);
      await text(child, iteration === 0 ? 'Synthetic session seeded' : 'Restart storage checked');
      await until(() => reports.length === iteration + 1, 'storage fixture result');
      await windowAction(child, 'screenshot', ['-Screenshot', path.join(evidence, role, `storage-fixture-${iteration}.png`)]);
      await windowAction(child, 'close');
      await text(child, 'Quit');
      await press(child, 'Quit');
      await until(() => child.exitCode !== null, 'storage helper Quit');
      assert.equal(child.exitCode, 0);
      seed = false;
    }
    assert.deepEqual(reports[0], { before: { session: null, local: null }, after: { session: true, local: true }, language: null });
    assert.deepEqual(reports[1], { before: { session: null, local: null }, after: { session: false, local: false }, language: 'zh' });
    await exactPreference(env, role, port, 'zh');
    await writeFile(path.join(evidence, role, 'storage-isolation.json'), JSON.stringify({ success: true, fixture: true, localAppData: env.LOCALAPPDATA, reports }, null, 2));
    console.log(`PASS ${role}: seeded synthetic tokens and general storage disappear across helper restart while language remains`);
  } finally {
    for (const entry of owned) if (entry.child.exitCode === null && entry.child.pid) await exec('taskkill.exe', ['/PID', String(entry.child.pid), '/T', '/F'], { timeout: 15_000 }).catch(() => {});
    await new Promise((resolve) => server.close(resolve));
  }
}

if (process.platform !== 'win32') throw new Error('Native locale smoke testing requires Windows.');
await mkdir(evidence, { recursive: true });
const results = [];
try {
  for (const role of ['master', 'worker']) {
    results.push(await runRole(role));
    await storageIsolation(role);
  }
} finally {
  await writeFile(path.join(evidence, 'processes.json'), JSON.stringify(owned.map(({ child, executable, output, spawnError }) => ({ pid: child.pid, exitCode: child.exitCode, executable, output, spawnError })), null, 2));
}
assert.equal(results.length, 2);
assert.ok(results.every((result) => result.success));
console.log('Native locale and incognito-storage checks passed for both packaged roles.');
