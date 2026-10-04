import assert from 'node:assert/strict';
import { spawn, execFile } from 'node:child_process';
import { generateKeyPairSync } from 'node:crypto';
import { copyFile, cp, mkdir, mkdtemp, writeFile } from 'node:fs/promises';
import { createServer } from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { promisify } from 'node:util';
import { until, windowAction } from './native-ui-smoke.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const evidence = path.join(root, process.env.HIVEMIND_NATIVE_EVIDENCE_DIR || 'test_logs/native-ui');
const exec = promisify(execFile);
const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

async function findHelper(parent, role) {
  const { stdout } = await exec('powershell.exe', [
    '-NoProfile', '-NonInteractive', '-Command',
    `ConvertTo-Json -Compress -InputObject @(Get-CimInstance Win32_Process -Filter "ParentProcessId = ${parent.pid}" | Where-Object { $_.Name -eq 'hivemind-${role}-ui.exe' } | Select-Object -ExpandProperty ProcessId)`,
  ]);
  const helpers = JSON.parse(stdout.trim());
  assert.ok(helpers.length <= 1, `Expected at most one ${role} helper for backend ${parent.pid}`);
  return helpers.length ? { pid: helpers[0] } : null;
}

async function inspectUntil(helper, text, label) {
  await until(async () => {
    const state = await windowAction(helper, 'inspect');
    return state.controls.some((control) => !control.offscreen && control.name === text);
  }, label);
}

async function activateExisting(role, directory, env, result, parent, helper, statusUrl) {
  const child = spawn(path.join(directory, `hivemind-${role}.exe`), [], {
    cwd: directory, env, stdio: ['ignore', 'pipe', 'pipe'],
  });
  let output = '';
  child.stdout.on('data', (chunk) => { output += chunk; });
  child.stderr.on('data', (chunk) => { output += chunk; });
  const exited = new Promise((resolve, reject) => {
    child.once('exit', (code) => resolve(code));
    child.once('error', reject);
  });
  try {
    const code = await Promise.race([
      exited,
      delay(15_000).then(() => { throw new Error(`${role} duplicate launch did not exit`); }),
    ]);
    result.activations ||= [];
    result.activations.push({ directory, processId: child.pid, code, output });
    assert.equal(code, 0, `${role} duplicate launch must activate rather than bind ports: ${output}`);
    assert.match(output, /Activated existing .* client/);
    await until(async () => {
      const state = await windowAction(helper, 'state');
      return state.visible && !state.minimized;
    }, `${role} duplicate launch restores original window`);
    const state = await windowAction(helper, 'state');
    assert.equal(state.windowHandle, result.originalWindow.windowHandle, 'Activation must reuse the original HWND');
    assert.equal(await findHelper(child, role), null, 'Duplicate launch must not spawn another helper');
    assert.equal(parent.exitCode, null);
    assert.deepEqual(await findHelper(parent, role), helper, 'Duplicate launch must retain exactly one original helper');
    const response = await fetch(statusUrl, { signal: AbortSignal.timeout(2000) });
    assert.equal(response.status, 200);
    assert.deepEqual(await response.json(), result.startup);
  } finally {
    if (child.exitCode === null) {
      child.kill();
      await Promise.race([exited.catch(() => {}), delay(5000)]);
    }
  }
}

async function prepareClient(role, directory) {
  const bundle = process.env[`HIVEMIND_NATIVE_${role.toUpperCase()}_BUNDLE`];
  if (bundle) {
    await cp(path.resolve(bundle), directory, { recursive: true });
  } else {
    for (const suffix of ['', '-ui']) {
      const name = `hivemind-${role}${suffix}.exe`;
      await copyFile(path.join(root, 'hivemind-rs/target/debug', name), path.join(directory, name));
    }
  }
  return bundle;
}

async function freePort() {
  const probe = createServer();
  await new Promise((resolve, reject) => {
    probe.once('error', reject);
    probe.listen(0, '127.0.0.1', resolve);
  });
  const port = probe.address().port;
  await new Promise((resolve) => probe.close(resolve));
  return port;
}

async function checkIndependentClient(role, env, port, label, verify) {
  const directory = await mkdtemp(path.join(os.tmpdir(), `hivemind-${role}-${label}-`));
  const bundle = await prepareClient(role, directory);
  env = {
    ...env,
    [role === 'master' ? 'MASTER_HTTP_ADDR' : 'WORKER_CONTROL_HTTP_ADDR']: `127.0.0.1:${port}`,
    MASTER_UI_DIR: bundle && role === 'master' ? path.join(directory, 'master-ui') : path.join(root, 'frontend/master-ui/dist'),
    WORKER_UI_DIR: bundle && role === 'worker' ? path.join(directory, 'worker-ui') : path.join(root, 'frontend/worker-ui/dist'),
    HIVEMIND_VPN_STATE_ROOT: directory,
    TORRENT_API_DIR: path.join(directory, 'api'),
    TORRENT_BT_DIR: path.join(directory, 'bt'),
  };
  const parent = spawn(path.join(directory, `hivemind-${role}.exe`), [], { cwd: directory, env, stdio: ['ignore', 'pipe', 'pipe'] });
  let output = '';
  parent.stdout.on('data', (chunk) => { output += chunk; });
  parent.stderr.on('data', (chunk) => { output += chunk; });
  const exited = new Promise((resolve, reject) => {
    parent.once('exit', (code) => resolve(code));
    parent.once('error', reject);
  });
  const statusUrl = `http://127.0.0.1:${port}/api/startup-status`;
  const result = { role, label, directory, backendPid: parent.pid, checks: [] };
  let helper;
  try {
    await until(async () => {
      assert.equal(parent.exitCode, null, `${label} exited before startup: ${output}`);
      try {
        const response = await fetch(statusUrl, { signal: AbortSignal.timeout(1000) });
        result.startup = await response.json();
        return response.ok && result.startup.state !== 'initializing';
      } catch { return false; }
    }, `${role} ${label} startup`, 60_000);
    if (bundle) assert.equal(result.startup.state, 'ready');
    await until(async () => { helper = await findHelper(parent, role); return !!helper; }, `${role} ${label} helper`);
    if (result.startup.state === 'ready') await inspectUntil(helper, role === 'worker' ? 'Sign in and connect' : 'Sign in', `${role} ${label} render`);
    result.originalWindow = await windowAction(helper, 'state');
    result.helperPid = helper.pid;
    await verify({ parent, helper, directory, env, result, statusUrl });
    await windowAction(helper, 'close');
    await inspectUntil(helper, 'Quit', `${role} ${label} Quit`);
    await windowAction(helper, 'press', ['-Button', 'Quit']);
    assert.equal(await Promise.race([exited, delay(15_000).then(() => { throw new Error(`${label} did not quit`); })]), 0);
    await until(async () => !(await findHelper(parent, role)), `${role} ${label} helper exit`);
    return result;
  } finally {
    if (parent.exitCode === null) {
      parent.kill();
      await Promise.race([exited.catch(() => {}), delay(5000)]);
    }
    result.output = output;
  }
}

async function runRole(role) {
  // A unique executable directory prevents loading an operator's .env or state.
  const directory = await mkdtemp(path.join(os.tmpdir(), `hivemind-${role}-lifecycle-`));
  const bundle = await prepareClient(role, directory);
  let upstreamConnections = 0;
  const upstream = createServer((socket) => { upstreamConnections += 1; socket.destroy(); });
  await new Promise((resolve, reject) => {
    upstream.once('error', reject);
    upstream.listen(0, '127.0.0.1', resolve);
  });
  const safeKeys = new Set(['systemroot', 'windir', 'comspec', 'path', 'temp', 'tmp', 'userprofile', 'localappdata', 'appdata', 'programdata', 'programfiles', 'programfiles(x86)', 'number_of_processors', 'processor_architecture']);
  const env = Object.fromEntries(Object.entries(process.env).filter(([key]) => safeKeys.has(key.toLowerCase())));
  const port = role === 'master' ? 8082 : 18080;
  Object.assign(env, {
    NODEPOOL_GRPC_ENDPOINT: `http://127.0.0.1:${upstream.address().port}`,
    NODEPOOL_GRPC_ADDR: `127.0.0.1:${upstream.address().port}`,
    MASTER_HTTP_ADDR: '127.0.0.1:8082',
    WORKER_CONTROL_HTTP_ADDR: '127.0.0.1:18080',
    MASTER_WEBSITE_API_BASE: `http://127.0.0.1:${upstream.address().port}`,
    WORKER_WEBSITE_API_BASE: `http://127.0.0.1:${upstream.address().port}`,
    WEBSITE_API_BASE: `http://127.0.0.1:${upstream.address().port}`,
    WORKER_GRPC_ADDR: '127.0.0.1:0',
    MASTER_UI_DIR: bundle ? path.join(directory, 'master-ui') : path.join(root, 'frontend/master-ui/dist'),
    WORKER_UI_DIR: bundle ? path.join(directory, 'worker-ui') : path.join(root, 'frontend/worker-ui/dist'),
    WORKER_EXECUTION_PUBLIC_KEY_PEM: generateKeyPairSync('ed25519').publicKey.export({ type: 'spki', format: 'pem' }),
    WORKER_ID: 'native-lifecycle-fixture',
    HIVEMIND_VPN_STATE_ROOT: directory,
    TORRENT_API_DIR: path.join(directory, 'api'),
    TORRENT_BT_DIR: path.join(directory, 'bt'),
    UPDATE_ENABLED: 'false',
    RUST_LOG: 'info',
  });
  const parent = spawn(path.join(directory, `hivemind-${role}.exe`), [], { cwd: directory, env, stdio: ['ignore', 'pipe', 'pipe'] });
  let stdout = '';
  let stderr = '';
  parent.stdout.on('data', (chunk) => { stdout += chunk; });
  parent.stderr.on('data', (chunk) => { stderr += chunk; });
  const exited = new Promise((resolve, reject) => {
    parent.once('exit', (code, signal) => resolve({ code, signal }));
    parent.once('error', reject);
  });
  const result = { role, directory, bundle: bundle ? path.resolve(bundle) : null, checks: [] };
  let helper;
  const statusUrl = `http://127.0.0.1:${port}/api/startup-status`;
  try {
    await until(async () => {
      assert.equal(parent.exitCode, null, `Client exited before startup: ${stderr}`);
      try {
        const response = await fetch(statusUrl, { signal: AbortSignal.timeout(1000) });
        result.startup = await response.json();
        return response.ok && result.startup.state !== 'initializing';
      } catch { return false; }
    }, `${role} real backend startup`, 60_000);
    if (bundle) assert.equal(result.startup.state, 'ready', `${role} packaged client must complete startup`);
    await until(async () => { helper = await findHelper(parent, role); return !!helper; }, `${role} parent-spawned WebView2 helper`);
    if (result.startup.state === 'ready') await inspectUntil(helper, role === 'worker' ? 'Sign in and connect' : 'Sign in', `${role} real client rendered sign-in`);
    result.originalWindow = await windowAction(helper, 'state');
    result.backendPid = parent.pid;
    result.helperPid = helper.pid;
    result.checks.push(`real dedicated client launches its own restricted helper; startup state is ${result.startup.state}`);
    await windowAction(helper, 'close');
    await inspectUntil(helper, 'Keep in background', `${role} real client close dialog`);
    await windowAction(helper, 'press', ['-Button', 'Keep in background']);
    await until(async () => !(await windowAction(helper, 'state')).visible, `${role} real client background hide`);
    assert.equal(parent.exitCode, null);
    const backgroundResponse = await fetch(statusUrl, { signal: AbortSignal.timeout(2000) });
    assert.equal(backgroundResponse.status, 200);
    assert.deepEqual(await backgroundResponse.json(), result.startup);
    result.checks.push('background preserves the actual backend process and responsive local HTTP service');
    await windowAction(helper, 'restore');
    await until(async () => (await windowAction(helper, 'state')).visible, `${role} real client tray restoration`);
    await windowAction(helper, 'close');
    await inspectUntil(helper, 'Keep in background', `${role} background before duplicate launch`);
    await windowAction(helper, 'press', ['-Button', 'Keep in background']);
    await until(async () => !(await windowAction(helper, 'state')).visible, `${role} hidden before duplicate launch`);
    await activateExisting(role, directory, env, result, parent, helper, statusUrl);
    const copiedDirectory = await mkdtemp(path.join(os.tmpdir(), `hivemind-${role}-duplicate-`));
    for (const suffix of ['', '-ui']) {
      const name = `hivemind-${role}${suffix}.exe`;
      await copyFile(path.join(directory, name), path.join(copiedDirectory, name));
    }
    for (let count = 0; count < 3; count += 1) {
      await windowAction(helper, 'close');
      await inspectUntil(helper, 'Keep in background', `${role} repeated background confirmation`);
      await windowAction(helper, 'press', ['-Button', 'Keep in background']);
      await until(async () => !(await windowAction(helper, 'state')).visible, `${role} repeated hide`);
      await activateExisting(role, copiedDirectory, env, result, parent, helper, statusUrl);
    }
    result.checks.push('relaunching the EXE, including from another directory, restores the original window without a second backend or helper');
    await windowAction(helper, 'minimize');
    await until(async () => (await windowAction(helper, 'state')).minimized, `${role} minimize before relaunch`);
    await activateExisting(role, directory, env, result, parent, helper, statusUrl);
    result.checks.push('relaunching a minimized client restores the same native HWND');
    await windowAction(helper, 'screenshot', ['-Screenshot', path.join(evidence, `${role}-reactivated.png`)]);
    result.independentPort = await checkIndependentClient(role, env, await freePort(), 'different-port', async (other) => {
      assert.notEqual(other.parent.pid, parent.pid);
      assert.notEqual(other.helper.pid, helper.pid);
      await activateExisting(role, other.directory, other.env, other.result, other.parent, other.helper, other.statusUrl);
      await activateExisting(role, directory, env, result, parent, helper, statusUrl);
      assert.equal(other.parent.exitCode, null);
    });
    result.checks.push('a deliberately different UI port starts and reactivates its own independent backend and helper');
    if (role === 'master') {
      await windowAction(helper, 'close');
      await inspectUntil(helper, 'Keep in background', 'Master background before Worker launch');
      await windowAction(helper, 'press', ['-Button', 'Keep in background']);
      await until(async () => !(await windowAction(helper, 'state')).visible, 'Master hidden before Worker launch');
      result.independentRole = await checkIndependentClient('worker', env, 18080, 'separate-role', async (other) => {
        assert.notEqual(other.parent.pid, parent.pid);
        assert.notEqual(other.helper.pid, helper.pid);
        assert.equal((await windowAction(helper, 'state')).visible, false, 'Worker launch must not activate Master');
        await activateExisting('master', directory, env, result, parent, helper, statusUrl);
        assert.equal(other.parent.exitCode, null);
        assert.equal((await fetch(other.statusUrl)).status, 200);
      });
      result.checks.push('Master and Worker coexist independently; opening Worker does not reactivate the background Master');
    }
    await windowAction(helper, 'close');
    await inspectUntil(helper, 'Quit', `${role} real client quit confirmation`);
    await windowAction(helper, 'press', ['-Button', 'Quit']);
    const exit = await Promise.race([exited, delay(15_000).then(() => { throw new Error('Explicit Quit did not stop the actual backend'); })]);
    assert.equal(exit.code, 0);
    assert.match(stdout + stderr, /Local UI quit confirmed; shutting down client services/);
    await until(async () => {
      try { await fetch(statusUrl, { signal: AbortSignal.timeout(1000) }); return false; }
      catch { return true; }
    }, `${role} local HTTP service closed after Quit`);
    await until(async () => !(await findHelper(parent, role)), `${role} original helper stops after Quit`);
    result.checks.push('explicit Quit reaches the parent shutdown path, exits the actual backend cleanly and closes its HTTP listener');
    result.restart = await checkIndependentClient(role, env, port, 'restart-after-quit', async (replacement) => {
      assert.notEqual(replacement.parent.pid, parent.pid);
      assert.notEqual(replacement.helper.pid, helper.pid);
      await activateExisting(role, replacement.directory, replacement.env, replacement.result, replacement.parent, replacement.helper, replacement.statusUrl);
    });
    result.checks.push('Quit releases ownership so a subsequent launch starts a fresh backend and can be reactivated normally');
    assert.equal(upstreamConnections, 0, 'Lifecycle test must not attempt login, registration or other upstream operations');
    result.checks.push('no account credentials, live Nodepool access, registration or task execution used');
    return result;
  } catch (error) {
    result.failure = error.message;
    throw error;
  } finally {
    if (parent.exitCode === null) {
      parent.kill();
      await Promise.race([exited.catch(() => {}), delay(5000)]);
    }
    result.stdout = stdout;
    result.stderr = stderr;
    result.upstreamConnections = upstreamConnections;
    await writeFile(path.join(evidence, `${role}-runtime-result.json`), JSON.stringify(result, null, 2));
    await new Promise((resolve) => upstream.close(resolve));
  }
}

if (process.platform !== 'win32') throw new Error('Native runtime smoke testing requires Windows.');
await mkdir(evidence, { recursive: true });
for (const port of [8082, 18080]) {
  const probe = createServer();
  await new Promise((resolve, reject) => {
    probe.once('error', reject);
    probe.listen(port, '127.0.0.1', resolve);
  });
  await new Promise((resolve) => probe.close(resolve));
}
for (const role of ['master', 'worker']) {
  const result = await runRole(role);
  console.log(`${role}: ${result.checks.length} actual runtime lifecycle checks passed`);
}
