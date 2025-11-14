// Cloudflare Worker with simple admin panel to manage update manifests for
// 'worker' and 'master'. Admin login is protected by an environment variable
// ADMIN_PASSWORD. Data is stored in KV (binding name: VERSIONS). If KV is not
// bound, it will fallback to in-memory cache (not persistent).

export default {
	async fetch(request, env, ctx) {
		const url = new URL(request.url);
		const method = request.method.toUpperCase();

		// Route map
		if (url.pathname === '/' || url.pathname === '/admin') {
			// Admin UI (requires login)
			const session = await verifySession(request, env);
			if (!session) {
				return htmlResponse(loginPage());
			}
			return htmlResponse(adminPage());
		}

		if (url.pathname === '/login' && method === 'POST') {
			return handleLogin(request, env);
		}
		if (url.pathname === '/logout') {
			return logoutResponse();
		}

		// Public manifest endpoints (no auth)
		if (url.pathname === '/worker/manifest' || url.pathname === '/manifest/worker' || url.pathname === '/api/manifest/worker') {
			const data = await getManifest(env, 'worker');
			return jsonResponse(data || defaultManifest('worker'));
		}
		if (url.pathname === '/master/manifest' || url.pathname === '/manifest/master' || url.pathname === '/api/manifest/master') {
			const data = await getManifest(env, 'master');
			return jsonResponse(data || defaultManifest('master'));
		}

		// Auth-protected APIs
		if (url.pathname === '/api/me') {
			const session = await verifySession(request, env);
			if (!session) return unauthorized();
			return jsonResponse({ ok: true, user: 'admin', iat: session.iat, exp: session.exp });
		}

		// Config/status endpoints (admin)
		if (url.pathname === '/api/config' && method === 'GET') {
			const session = await verifySession(request, env);
			if (!session) return unauthorized();
			const base = await getBaseUrl(env);
			const source = await getBaseUrlSource(env);
			const { kv, name: kvBinding } = resolveKV(env);
			const { bucket, name: r2Binding } = resolveR2(env);
			const kvBound = Boolean(kv && kv.get && kv.put);
			const r2Bound = Boolean(bucket && bucket.put);
			return jsonResponse({ ok: true, base_url: base, source, kvBound, r2Bound, kvBinding, r2Binding });
		}
		if (url.pathname === '/api/config' && method === 'POST') {
			const session = await verifySession(request, env);
			if (!session) return unauthorized();
			const body = await parseJSON(request);
			if (!body || typeof body.base_url !== 'string') {
				return jsonResponse({ ok: false, error: 'base_url is required' }, 400);
			}
			let raw = String(body.base_url || '').trim();
			if (raw && !/^https?:\/\//i.test(raw)) raw = 'https://' + raw;
			await putConfig(env, { base_url: raw });
			return jsonResponse({ ok: true });
		}
		if (url.pathname === '/api/status' && method === 'GET') {
			const { kv } = resolveKV(env);
			const { bucket } = resolveR2(env);
			const kvBound = Boolean(kv && kv.get && kv.put);
			const r2Bound = Boolean(bucket && bucket.put);
			const worker = await getManifest(env, 'worker');
			const master = await getManifest(env, 'master');
			return jsonResponse({ ok: true, kvBound, r2Bound, latest: { worker: worker?.latest || null, master: master?.latest || null } });
		}

		if (url.pathname.startsWith('/api/version/') && method === 'GET') {
			// GET /api/version/{channel}?v=1.2.3  (optionally filter specific version)
			const channel = url.pathname.split('/').pop();
			const manifest = await getManifest(env, channel);
			if (!manifest) return jsonResponse(defaultManifest(channel));
			const v = url.searchParams.get('v');
			if (v) {
				const entry = manifest.versions?.[v];
				if (!entry) return jsonResponse({ error: 'Version not found', channel, version: v }, 404);
				return jsonResponse({ channel, version: v, artifacts: entry.artifacts || [], updated_at: entry.updated_at });
			}
			return jsonResponse(manifest);
		}
		if (url.pathname.startsWith('/api/version/') && method === 'POST') {
			// POST /api/version/{channel} (auth required) - set simple top-level version metadata (legacy)
			const session = await verifySession(request, env);
			if (!session) return unauthorized();
			const channel = url.pathname.split('/').pop();
			const body = await parseJSON(request);
			if (!body || !body.version) {
				return jsonResponse({ ok: false, error: 'version is required' }, 400);
			}
			const version = String(body.version);
			const manifest = (await getManifest(env, channel)) || defaultManifest(channel);
			manifest.latest = version;
			manifest.versions = manifest.versions || {};
			manifest.versions[version] = manifest.versions[version] || { artifacts: [], updated_at: new Date().toISOString() };
			manifest.versions[version].updated_at = new Date().toISOString();
			await putManifest(env, channel, manifest);
			return jsonResponse({ ok: true, manifest });
		}
		if (url.pathname.startsWith('/api/upload/') && method === 'POST') {
			// POST /api/upload/{channel}  (multipart/form-data) fields: version, os, arch, file
			const session = await verifySession(request, env);
			if (!session) return unauthorized();
			const channel = url.pathname.split('/').pop();
			const contentType = request.headers.get('Content-Type') || '';
			if (!contentType.includes('multipart/form-data')) {
				return jsonResponse({ ok: false, error: 'multipart/form-data required' }, 400);
			}
			const form = await request.formData();
			const version = String(form.get('version') || '').trim();
			const osName = String(form.get('os') || '').trim().toLowerCase();
			const arch = String(form.get('arch') || '').trim().toLowerCase();
			const file = form.get('file');
			if (!version || !osName || !arch || !file) {
				return jsonResponse({ ok: false, error: 'version, os, arch, file required' }, 400);
			}
			if (typeof file.name !== 'string' || !file.stream) {
				return jsonResponse({ ok: false, error: 'invalid file upload' }, 400);
			}
			// Read file for digest
			const arrayBuff = await file.arrayBuffer();
			const shaArr = new Uint8Array(await crypto.subtle.digest('SHA-256', arrayBuff));
			const sha256 = [...shaArr].map(b => b.toString(16).padStart(2, '0')).join('');
			const size = arrayBuff.byteLength;
			// R2 put
			const key = `${channel}/${version}/${osName}-${arch}/${file.name}`;
			try {
				const { bucket } = resolveR2(env);
				if (bucket && bucket.put) {
					await bucket.put(key, file.stream(), { httpMetadata: { contentType: 'application/octet-stream' } });
				} else {
					return jsonResponse({ ok: false, error: 'R2 bucket not bound (set R2_BINDING env)' }, 500);
				}
			} catch (e) {
				return jsonResponse({ ok: false, error: 'R2 upload failed', detail: String(e) }, 500);
			}
			const manifest = (await getManifest(env, channel)) || defaultManifest(channel);
			manifest.versions = manifest.versions || {};
			manifest.versions[version] = manifest.versions[version] || { artifacts: [], updated_at: new Date().toISOString() };
			const baseUrl = await getBaseUrl(env);
			const download_url = baseUrl ? `${baseUrl}/${key}` : '';
			const artifact = { os: osName, arch, filename: file.name, r2_key: key, size, sha256, download_url };
			// Remove existing same os/arch artifact if present
			manifest.versions[version].artifacts = (manifest.versions[version].artifacts || []).filter(a => !(a.os === osName && a.arch === arch));
			manifest.versions[version].artifacts.push(artifact);
			manifest.versions[version].updated_at = new Date().toISOString();
			manifest.latest = version; // optional auto-bump latest
			await putManifest(env, channel, manifest);
			return jsonResponse({ ok: true, artifact, manifest_version: version });
		}

		// Fallback 404
		return jsonResponse({ error: 'Not Found' }, 404);
	},
};

// ---------- Storage helpers (extended manifest) ----------
async function getManifest(env, channel) {
	if (!channel) return null;
	const { kv } = resolveKV(env);
	if (kv && kv.get) {
		const s = await kv.get(keyFor(channel));
		return s ? JSON.parse(s) : null;
	}
	globalThis.__mem = globalThis.__mem || new Map();
	return globalThis.__mem.get(keyFor(channel)) || null;
}
async function putManifest(env, channel, manifest) {
	const { kv } = resolveKV(env);
	if (kv && kv.put) {
		await kv.put(keyFor(channel), JSON.stringify(manifest));
		return;
	}
	globalThis.__mem = globalThis.__mem || new Map();
	globalThis.__mem.set(keyFor(channel), manifest);
}
function keyFor(channel) { return `manifest:${channel}`; }
function defaultManifest(channel) {
	return { channel, latest: null, versions: {} };
}

// ---------- Config helpers ----------
async function getConfig(env) {
	const { kv } = resolveKV(env);
	if (kv && kv.get) {
		const s = await kv.get('config:global');
		return s ? JSON.parse(s) : {};
	}
	globalThis.__mem = globalThis.__mem || new Map();
	return globalThis.__mem.get('config:global') || {};
}
async function putConfig(env, cfg) {
	const current = await getConfig(env);
	const next = { ...current, ...cfg };
	const { kv } = resolveKV(env);
	if (kv && kv.put) {
		await kv.put('config:global', JSON.stringify(next));
		return;
	}
	globalThis.__mem = globalThis.__mem || new Map();
	globalThis.__mem.set('config:global', next);
}
async function getBaseUrl(env) {
	const cfg = await getConfig(env);
	const kvUrl = (cfg && cfg.base_url) ? String(cfg.base_url) : '';
	const envUrl = env.PUBLIC_R2_BASE_URL ? String(env.PUBLIC_R2_BASE_URL) : '';
	const base = (kvUrl || envUrl || '').trim();
	return base ? base.replace(/\/$/, '') : '';
}
async function getBaseUrlSource(env) {
	const cfg = await getConfig(env);
	if (cfg && cfg.base_url) return 'kv';
	if (env.PUBLIC_R2_BASE_URL) return 'env';
	return 'none';
}

// ---------- Binding resolution (env-controlled) ----------
function resolveKV(env) {
	const name = (env && env.KV_BINDING) ? String(env.KV_BINDING) : 'VERSIONS';
	const kv = env && env[name];
	return { kv, name };
}
function resolveR2(env) {
	const name = (env && env.R2_BINDING) ? String(env.R2_BINDING) : 'BINARIES';
	const bucket = env && env[name];
	return { bucket, name };
}

// ---------- Auth & session ----------
async function handleLogin(request, env) {
	const body = await parseFormOrJSON(request);
	const password = (body && body.password) ? String(body.password) : '';
	const adminPass = (env && env.ADMIN_PASSWORD) ? String(env.ADMIN_PASSWORD) : '';
	if (!adminPass) {
		return jsonResponse({ ok: false, error: 'SERVER_NOT_CONFIGURED' }, 500);
	}
	if (!password || password !== adminPass) {
		return jsonResponse({ ok: false, error: 'INVALID_PASSWORD' }, 401);
	}
	// issue session cookie (signed)
	const token = await signSession(env, { sub: 'admin' }, 24 * 3600); // 24h
	const headers = new Headers({ 'Content-Type': 'application/json' });
	setSessionCookie(headers, token, 24 * 3600);
	return new Response(JSON.stringify({ ok: true }), { status: 200, headers });
}

function logoutResponse() {
	const headers = new Headers();
	headers.append('Set-Cookie', `session=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0`);
	return new Response('Logged out', { status: 200, headers });
}

async function verifySession(request, env) {
	const cookie = request.headers.get('Cookie') || '';
	const token = parseCookie(cookie).get('session');
	if (!token) return null;
	try {
		const session = await verifyToken(env, token);
		const now = Math.floor(Date.now() / 1000);
		if (!session || (session.exp && session.exp < now)) return null;
		return session;
	} catch (_) {
		return null;
	}
}

function setSessionCookie(headers, token, maxAgeSec) {
	const secure = true; // CF is HTTPS
	headers.append('Set-Cookie', `session=${token}; Path=/; HttpOnly; SameSite=Lax; Max-Age=${maxAgeSec}; ${secure ? 'Secure;' : ''}`);
}

function parseCookie(cstr) {
	const map = new Map();
	cstr.split(';').forEach((p) => {
		const [k, v] = p.split('=').map((s) => (s || '').trim());
		if (k) map.set(k, decodeURIComponent(v || ''));
	});
	return map;
}

async function signSession(env, payload, ttlSec) {
	const now = Math.floor(Date.now() / 1000);
	const body = {
		...payload,
		iat: now,
		exp: now + ttlSec,
	};
	const json = JSON.stringify(body);
	const b64 = base64url(json);
	const sig = await hmacSign(env, b64);
	return `${b64}.${sig}`;
}

async function verifyToken(env, token) {
	const [b64, sig] = String(token).split('.');
	if (!b64 || !sig) return null;
	const good = await hmacVerify(env, b64, sig);
	if (!good) return null;
	try { return JSON.parse(atoburl(b64)); } catch { return null; }
}

async function hmacSign(env, message) {
	const key = await importKey(env);
	const sig = await crypto.subtle.sign('HMAC', key, new TextEncoder().encode(message));
	return bufToHex(sig);
}

async function hmacVerify(env, message, hexSig) {
	const key = await importKey(env);
	const sig = hexToBuf(hexSig);
	const ok = await crypto.subtle.verify('HMAC', key, sig, new TextEncoder().encode(message));
	return ok;
}

async function importKey(env) {
	const secret = (env && env.ADMIN_PASSWORD) ? String(env.ADMIN_PASSWORD) : 'default-secret';
	return crypto.subtle.importKey('raw', new TextEncoder().encode(secret), { name: 'HMAC', hash: 'SHA-256' }, false, ['sign', 'verify']);
}

function base64url(s) {
	return btoa(s).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/g, '');
}
function atoburl(s) {
	s = s.replace(/-/g, '+').replace(/_/g, '/');
	while (s.length % 4) s += '=';
	return atob(s);
}
function bufToHex(buf) {
	const b = new Uint8Array(buf);
	return [...b].map((x) => x.toString(16).padStart(2, '0')).join('');
}
function hexToBuf(hex) {
	const len = hex.length / 2;
	const out = new Uint8Array(len);
	for (let i = 0; i < len; i++) out[i] = parseInt(hex.substr(i * 2, 2), 16);
	return out;
}

// ---------- utils & pages ----------
async function parseJSON(request) {
	try { return await request.json(); } catch { return null; }
}
async function parseFormOrJSON(request) {
	const ct = request.headers.get('Content-Type') || '';
	if (ct.includes('application/json')) return parseJSON(request);
	if (ct.includes('application/x-www-form-urlencoded')) {
		const form = await request.formData();
		const obj = {};
		for (const [k, v] of form.entries()) obj[k] = v;
		return obj;
	}
	// try text
	try { return JSON.parse(await request.text()); } catch { return null; }
}

function jsonResponse(obj, status = 200) {
	return new Response(JSON.stringify(obj), { status, headers: { 'Content-Type': 'application/json', 'Cache-Control': 'no-store' } });
}
function htmlResponse(html, status = 200) {
	return new Response(html, { status, headers: { 'Content-Type': 'text/html; charset=utf-8' } });
}
function unauthorized() {
	return new Response('Unauthorized', { status: 401 });
}

function loginPage() {
	return `<!doctype html>
<html>
<head>
	<meta charset="utf-8" />
	<meta name="viewport" content="width=device-width, initial-scale=1" />
	<title>Hivemind Admin Login</title>
	<style>
		body{font-family:system-ui,-apple-system,Segoe UI,Roboto,Ubuntu;display:flex;align-items:center;justify-content:center;height:100vh;background:#0b1220;color:#e5e7eb}
		.card{background:#111827;border:1px solid #1f2937;border-radius:12px;padding:24px;max-width:360px;width:100%;box-shadow:0 10px 30px rgba(0,0,0,.4)}
		h1{font-size:20px;margin:0 0 16px}
		input{width:100%;padding:10px 12px;border-radius:8px;border:1px solid #374151;background:#0b1220;color:#e5e7eb}
		button{width:100%;padding:10px 12px;border-radius:8px;border:0;background:#2563eb;color:white;font-weight:600;margin-top:12px}
		.msg{margin-top:12px;color:#fca5a5;min-height:1.2em}
	</style>
	</head>
	<body>
		<div class="card">
			<h1>Hivemind 管理面板登入</h1>
			<input id="password" type="password" placeholder="管理密碼" />
			<button onclick="login()">登入</button>
			<div class="msg" id="msg"></div>
		</div>
		<script>
			async function login(){
				const password = document.getElementById('password').value;
				const res = await fetch('/login', {method:'POST', headers:{'Content-Type':'application/json'}, body:JSON.stringify({password})});
				if(res.ok){ location.href = '/admin'; }
				else{ const j = await res.json().catch(()=>({})); document.getElementById('msg').textContent = j.error||'登入失敗'; }
			}
		</script>
	</body>
</html>`;
}

function adminPage() {
	return `<!doctype html>
<html>
<head>
	<meta charset="utf-8" />
	<meta name="viewport" content="width=device-width, initial-scale=1" />
	<title>Hivemind Admin</title>
	<style>
		body{font-family:system-ui,-apple-system,Segoe UI,Roboto,Ubuntu;background:#0b1220;color:#e5e7eb;margin:0;padding:0}
		header{display:flex;justify-content:space-between;align-items:center;padding:16px 20px;background:#111827;border-bottom:1px solid #1f2937}
		main{max-width:960px;margin:24px auto;padding:0 16px}
		.card{background:#111827;border:1px solid #1f2937;border-radius:12px;padding:16px;margin-bottom:16px}
		input{width:100%;padding:8px 10px;border-radius:8px;border:1px solid #374151;background:#0b1220;color:#e5e7eb}
		label{display:block;margin:8px 0 6px;color:#9ca3af}
		.row{display:grid;grid-template-columns:1fr 1fr;gap:12px}
		button{padding:10px 14px;border-radius:8px;border:0;background:#10b981;color:#06281b;font-weight:700}
		.warning{color:#fbbf24}
		.grid{display:grid;grid-template-columns:1fr 1fr;gap:16px}
		.small{font-size:12px;color:#9ca3af}
		a{color:#93c5fd}
		.table{width:100%;border-collapse:collapse}
		.table th,.table td{border-bottom:1px solid #1f2937;padding:6px 8px;text-align:left;font-size:13px}
		.badge{display:inline-block;padding:2px 6px;border-radius:6px;background:#374151;color:#d1d5db;font-size:12px}
	</style>
	</head>
	<body>
		<header>
			<div>Hivemind 管理面板</div>
			<nav><a href="/logout">登出</a></nav>
		</header>
		<main>
			<section class="card">
				<h3>系統設定</h3>
				<div class="row">
					<div>
						<label>KV 綁定</label>
						<div class="small" id="kv_status">讀取中...</div>
					</div>
					<div>
						<label>R2 綁定</label>
						<div class="small" id="r2_status">讀取中...</div>
					</div>
				</div>
				<label>下載 Base URL (供 public download_url 生成)</label>
				<input id="base_url" placeholder="https://cdn.example.com" />
				<div style="margin-top:10px"><button onclick="saveConfig()">儲存設定</button></div>
				<div class="small" style="margin-top:8px">公開 manifests: 
					<a href="/worker/manifest" target="_blank">/worker/manifest</a> · 
					<a href="/master/manifest" target="_blank">/master/manifest</a>
				</div>
			</section>

			<section class="grid">
				<div class="card">
					<h3>Worker 版本</h3>
					<div class="small" id="worker_current">讀取中...</div>
					<div id="worker_versions"></div>
				</div>
				<div class="card">
					<h3>Master 版本</h3>
					<div class="small" id="master_current">讀取中...</div>
					<div id="master_versions"></div>
				</div>
			</section>

			<div class="card">
				<h3>Artifact 上傳</h3>
				<div class="small">上傳壓縮檔 (zip/exe/tar.gz)，自動計算 SHA256，並寫入 manifest（需綁定 R2）。</div>
				<label>Channel</label>
				<select id="up_channel">
					<option value="worker">worker</option>
					<option value="master">master</option>
				</select>
				<label>Version</label>
				<input id="up_version" placeholder="1.2.3" />
				<label>OS</label>
				<select id="up_os">
					<option>windows</option>
					<option>linux</option>
					<option>darwin</option>
				</select>
				<label>Arch</label>
				<select id="up_arch">
					<option value="x86_64">x86_64</option>
					<option value="arm64">arm64</option>
					<option value="armv7">armv7</option>
				</select>
				<label>File</label>
				<input id="up_file" type="file" />
				<div style="margin-top:10px"><button onclick="uploadArtifact()">上傳 Artifact</button></div>
				<pre id="up_result" style="margin-top:12px;font-size:12px;white-space:pre-wrap"></pre>
			</div>

			<p class="small">提示：Base URL 可用 Cloudflare R2 自訂域名或公開路徑，儲存後新上傳的 download_url 會依此生成。</p>
		</main>
		<script>
			window.onerror = function(msg, src, line, col, err){
				console.error('[頁面錯誤]', msg, src, line+':'+col, err);
			};
			async function loadConfig(){
				try {
					const r = await fetch('/api/config');
					if(!r.ok) return;
					const j = await r.json();
					document.getElementById('base_url').value = j.base_url || '';
					document.getElementById('kv_status').textContent = (j.kvBound ? '已綁定' : '未綁定') + (j.kvBinding?(' ('+j.kvBinding+')'):'');
					document.getElementById('r2_status').textContent = (j.r2Bound ? '已綁定' : '未綁定') + (j.r2Binding?(' ('+j.r2Binding+')'):'');
				} catch(e){ console.error('loadConfig error', e); }
			}
			async function saveConfig(){
				try {
					const base_url = document.getElementById('base_url').value.trim();
					const r = await fetch('/api/config', { method:'POST', headers:{'Content-Type':'application/json'}, body: JSON.stringify({ base_url })});
					if(r.ok){ alert('已儲存'); } else { const j = await r.json().catch(()=>({})); alert('儲存失敗: '+(j.error||r.status)); }
				} catch(e){ console.error('saveConfig error', e); }
			}
			function renderVersions(ch, manifest){
				try {
					const container = document.getElementById(ch+'_versions');
					const latest = manifest.latest || null;
					document.getElementById(ch+'_current').textContent = '最新版本: '+(latest||'(none)')+' | 共 '+Object.keys(manifest.versions||{}).length+' 版本';
					const keys = Object.keys(manifest.versions||{}).sort(function(a,b){return a.localeCompare(b,undefined,{numeric:true,sensitivity:'base'});}).reverse();
					var rows = [];
					for (var i=0;i<keys.length;i++) {
						var v = keys[i];
						var entry = manifest.versions[v];
						var arts = (entry.artifacts||[]).map(function(a){
							return '<div><span class="badge">' + a.os + '/' + a.arch + '</span> ' +
								'<a href="' + (a.download_url||'#') + '" target="_blank">' + a.filename + '</a> ' +
								'<span class="small">(' + (a.size||0) + ' bytes)</span></div>';
						}).join('') || '<span class="small">(無)</span>';
						rows.push('<tr><td>' + v + ' ' + (v===latest?'<span class="badge">latest</span>':'') + '</td><td>' + arts + '</td><td><span class="small">' + (entry.updated_at||'') + '</span></td><td><button data-ch="'+ch+'" data-v="'+v+'" class="mkLatestBtn">設為最新</button></td></tr>');
					}
					var html = '<table class="table"><thead><tr><th>版本</th><th>Artifacts</th><th>更新時間</th><th>操作</th></tr></thead><tbody>' + rows.join('') + '</tbody></table>';
					container.innerHTML = html;
					var btns = container.querySelectorAll('.mkLatestBtn');
					for (var j=0;j<btns.length;j++) {
						btns[j].addEventListener('click', function(ev){
							var b = ev.currentTarget;
							markLatest(b.getAttribute('data-ch'), b.getAttribute('data-v'));
						});
					}
				} catch(e){ console.error('renderVersions error', e); }
			}
			async function fetchManifests(){
				try {
					for(var idx=0; idx<['worker','master'].length; idx++){
						var ch = ['worker','master'][idx];
						var r = await fetch('/api/version/'+ch);
						var j = await r.json();
						renderVersions(ch, j);
					}
				} catch(e){ console.error('fetchManifests error', e); }
			}
			async function markLatest(ch, version){
				try {
					const r = await fetch('/api/version/'+ch, {method:'POST', headers:{'Content-Type':'application/json'}, body:JSON.stringify({version})});
					if(r.ok){ fetchManifests(); } else { const j = await r.json().catch(()=>({})); alert('更新失敗: '+(j.error||r.status)); }
				} catch(e){ console.error('markLatest error', e); }
			}
			async function uploadArtifact(){
				try {
					var fd = new FormData();
					fd.append('version', document.getElementById('up_version').value.trim());
					fd.append('os', document.getElementById('up_os').value.trim());
					fd.append('arch', document.getElementById('up_arch').value.trim());
					var fileInput = document.getElementById('up_file');
					if(!fileInput.files.length){ alert('請選擇檔案'); return; }
					fd.append('file', fileInput.files[0]);
					var channel = document.getElementById('up_channel').value;
					var res = await fetch('/api/upload/'+channel, { method:'POST', body: fd });
					var j = await res.json().catch(()=>({}));
					document.getElementById('up_result').textContent = JSON.stringify(j, null, 2);
					if(res.ok){ fetchManifests(); }
				} catch(e){ console.error('uploadArtifact error', e); }
			}
			loadConfig();
			fetchManifests();
		</script>
	</body>
</html>`;
}

