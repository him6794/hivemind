import assert from 'node:assert/strict';
import { spawn, execFile } from 'node:child_process';
import { createServer } from 'node:http';
import { readFile, mkdir, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { promisify } from 'node:util';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const exec = promisify(execFile);
const evidence = path.join(root, process.env.HIVEMIND_NATIVE_EVIDENCE_DIR || 'test_logs/native-ui');
const mime = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.svg': 'image/svg+xml', '.png': 'image/png', '.ico': 'image/x-icon' };
const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

async function until(check, label, timeout = 25_000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    if (await check()) return;
    await delay(100);
  }
  throw new Error(`Timed out: ${label}`);
}

async function windowAction(child, action, extra = []) {
  const { stdout } = await exec('powershell.exe', [
    '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass',
    '-File', path.join(root, 'scripts/native-ui-window-control.ps1'),
    '-ClientProcessId', String(child.pid), '-Action', action, ...extra,
  ], { timeout: 30_000 });
  return JSON.parse(stdout.trim());
}

async function hasText(child, text) {
  const state = await windowAction(child, 'inspect');
  return state.controls.some((control) => !control.offscreen && control.name === text);
}

async function runRole(role) {
  const requests = [];
  let fixtureMode = 'ready';
  const bundle = process.env[`HIVEMIND_NATIVE_${role.toUpperCase()}_BUNDLE`];
  const directory = bundle ? path.join(path.resolve(bundle), `${role}-ui`) : path.join(root, `frontend/${role}-ui/dist`);
  const server = createServer(async (request, response) => {
    const pathname = new URL(request.url, 'http://127.0.0.1').pathname;
    requests.push(pathname);
    if (fixtureMode === 'missing-bundle' && pathname.endsWith('.js')) {
      await delay(2000);
      response.writeHead(404); response.end(); return;
    }
    if (pathname === '/api/startup-status') {
      response.writeHead(200, { 'content-type': 'application/json' });
      response.end(JSON.stringify({ state: 'ready', phase: 'ready' }));
      return;
    }
    if (pathname === '/api/worker-info') {
      response.writeHead(200, { 'content-type': 'application/json' });
      response.end(JSON.stringify({ success: true, profile: { worker_id: 'native-fixture', ip: '127.0.0.1:50053', cpu_cores: 8, memory_gb: 16, cpu_score: 1000, gpu_score: 0, gpu_memory_gb: 0, storage_total_gb: 128, storage_available_gb: 64 } }));
      return;
    }
    const file = path.resolve(directory, `.${pathname === '/' ? '/index.html' : pathname}`);
    if (!file.startsWith(`${directory}${path.sep}`)) {
      response.writeHead(403); response.end(); return;
    }
    try {
      const contents = await readFile(file);
      response.writeHead(200, { 'content-type': mime[path.extname(file)] || 'application/octet-stream' });
      response.end(contents);
    } catch {
      response.writeHead(404); response.end();
    }
  });
  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(role === 'worker' ? 18080 : 8082, '127.0.0.1', resolve);
  });
  const url = `http://127.0.0.1:${server.address().port}/`;
  const signInLabel = role === 'worker' ? 'Sign in and connect' : 'Sign in';
  const executable = bundle
    ? path.join(path.resolve(bundle), `hivemind-${role}-ui.exe`)
    : path.join(root, `hivemind-rs/target/debug/hivemind-${role}-ui.exe`);
  const children = [];
  const result = { role, checks: [], requests };
  let child;
  let stdout = '';
  let stderr = '';
  try {
    child = spawn(executable, [url], { stdio: ['pipe', 'pipe', 'pipe'] });
    children.push(child);
    child.stdout.on('data', (chunk) => { stdout += chunk; });
    child.stderr.on('data', (chunk) => { stderr += chunk; });
    const exited = new Promise((resolve) => child.once('exit', (code, signal) => resolve({ code, signal })));
    await until(() => stdout.includes('HIVEMIND_LOCAL_UI_READY\n'), `${role} native readiness`);
    await until(async () => {
      result.lastInspection = await windowAction(child, 'inspect');
      return result.lastInspection.controls.some((control) => !control.offscreen && control.name === signInLabel);
    }, `${role} actual rendered sign-in controls`);
    assert.equal((await windowAction(child, 'state')).visible, true);
    await windowAction(child, 'screenshot', ['-Screenshot', path.join(evidence, `${role}-window.png`)]);
    result.checks.push('production sign-in controls render in a visible restricted WebView2');

    await windowAction(child, 'close');
    await until(() => hasText(child, `Keep ${role === 'master' ? 'Master' : 'Worker'} running?`), `${role} native close dialog`);
    await windowAction(child, 'screenshot', ['-Screenshot', path.join(evidence, `${role}-close.png`)]);
    await windowAction(child, 'press', ['-Button', 'Cancel']);
    await until(async () => {
      result.lastInspection = await windowAction(child, 'inspect');
      return !result.lastInspection.controls.some((control) => !control.offscreen && control.name === 'Keep in background');
    }, `${role} cancel dismissal`);
    assert.equal((await windowAction(child, 'state')).visible, true);
    assert.equal(child.exitCode, null);
    assert.equal(stdout.includes('HIVEMIND_LOCAL_UI_QUIT'), false);
    result.checks.push('real WM_CLOSE prompts; Cancel preserves visible running client');

    await windowAction(child, 'close');
    await until(() => hasText(child, 'Keep in background'), `${role} second close dialog`);
    await windowAction(child, 'press', ['-Button', 'Keep in background']);
    await until(async () => !(await windowAction(child, 'state')).visible, `${role} background hide`);
    assert.equal(child.exitCode, null);
    assert.equal(stdout.includes('HIVEMIND_LOCAL_UI_QUIT'), false);
    result.checks.push('background hides without quit marker or process exit');

    await windowAction(child, 'restore');
    await until(async () => (await windowAction(child, 'state')).visible, `${role} actual tray restoration`);
    await until(() => hasText(child, signInLabel), `${role} restored application`);
    result.checks.push('actual Windows notification-area click restores the application');

    await windowAction(child, 'close');
    await until(() => hasText(child, 'Quit'), `${role} quit dialog`);
    await windowAction(child, 'press', ['-Button', 'Quit']);
    const exit = await Promise.race([exited, delay(10_000).then(() => { throw new Error('Quit did not exit native helper'); })]);
    assert.equal(exit.code, 0);
    assert.equal(stdout.includes('HIVEMIND_LOCAL_UI_QUIT\n'), true);
    assert.equal(requests.some((request) => request.startsWith('/__hivemind_desktop__/')), false);
    result.checks.push('explicit Quit emits private parent shutdown marker and exits cleanly; no control HTTP requests');

    const eofChild = spawn(executable, [url], { stdio: ['pipe', 'pipe', 'pipe'] });
    children.push(eofChild);
    let eofOutput = '';
    eofChild.stdout.on('data', (chunk) => { eofOutput += chunk; });
    const eofExited = new Promise((resolve) => eofChild.once('exit', (code) => resolve(code)));
    await until(() => eofOutput.includes('HIVEMIND_LOCAL_UI_READY\n'), `${role} parent-lifetime fixture readiness`);
    eofChild.stdin.end();
    const eofCode = await Promise.race([eofExited, delay(10_000).then(() => { throw new Error('Parent EOF did not stop helper'); })]);
    assert.equal(eofCode, 0);
    assert.equal(eofOutput.includes('HIVEMIND_LOCAL_UI_QUIT'), false);
    result.checks.push('closing the private parent lifetime pipe stops helper without requesting backend quit');

    fixtureMode = 'missing-bundle';
    child = spawn(executable, [url], { stdio: ['pipe', 'pipe', 'pipe'] });
    children.push(child);
    let fallbackOutput = '';
    child.stdout.on('data', (chunk) => { fallbackOutput += chunk; });
    child.stderr.on('data', (chunk) => { stderr += chunk; });
    const fallbackExited = new Promise((resolve) => child.once('exit', (code) => resolve(code)));
    await until(() => fallbackOutput.includes('HIVEMIND_LOCAL_UI_READY\n'), `${role} failing-bundle fixture readiness`);
    await windowAction(child, 'close');
    await until(() => hasText(child, 'Keep client running?'), `${role} early-close fallback without React`);
    await windowAction(child, 'screenshot', ['-Screenshot', path.join(evidence, `${role}-fallback.png`)]);
    await windowAction(child, 'press', ['-Button', 'Cancel']);
    await until(async () => !(await hasText(child, 'Keep in background')), `${role} fallback cancel dismissal`);
    assert.equal(child.exitCode, null);
    await windowAction(child, 'close');
    await until(() => hasText(child, 'Keep in background'), `${role} fallback background dialog`);
    await windowAction(child, 'press', ['-Button', 'Keep in background']);
    await until(async () => !(await windowAction(child, 'state')).visible, `${role} fallback background hide`);
    assert.equal(child.exitCode, null);
    assert.equal(fallbackOutput.includes('HIVEMIND_LOCAL_UI_QUIT'), false);
    await windowAction(child, 'restore');
    await until(async () => (await windowAction(child, 'state')).visible, `${role} fallback tray restoration`);
    await windowAction(child, 'close');
    await until(() => hasText(child, 'Quit'), `${role} fallback quit dialog`);
    await windowAction(child, 'press', ['-Button', 'Quit']);
    const fallbackCode = await Promise.race([fallbackExited, delay(10_000).then(() => { throw new Error('Fallback Quit did not exit helper'); })]);
    assert.equal(fallbackCode, 0);
    assert.equal(fallbackOutput.includes('HIVEMIND_LOCAL_UI_QUIT\n'), true);
    result.checks.push('early close and failed React bundle retain working Cancel, background, tray restore and Quit through the native fallback');
    return result;
  } catch (error) {
    result.failure = error.message;
    if (child?.exitCode === null) {
      await windowAction(child, 'screenshot', ['-Screenshot', path.join(evidence, `${role}-failure.png`)]).catch(() => {});
    }
    throw error;
  } finally {
    for (const helper of children) {
      if (helper.exitCode === null) {
        helper.stdin.end();
        await Promise.race([new Promise((resolve) => helper.once('exit', resolve)), delay(5000)]);
        if (helper.exitCode === null) helper.kill();
      }
    }
    result.stderr = stderr;
    result.stdout = stdout;
    await writeFile(path.join(evidence, `${role}-result.json`), JSON.stringify(result, null, 2));
    await new Promise((resolve) => server.close(resolve));
  }
}

export { until, windowAction };

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (process.platform !== 'win32') throw new Error('Native UI smoke testing requires Windows.');
  await mkdir(evidence, { recursive: true });
  for (const role of ['master', 'worker']) {
    const result = await runRole(role);
    console.log(`${role}: ${result.checks.length} native lifecycle checks passed`);
  }
}
