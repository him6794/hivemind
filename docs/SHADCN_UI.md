# shadcn UI and native client verification

## Sources and packaging

The website, Master UI, and Worker UI use shadcn's `new-york` registry components, with Radix primitives providing accessible dialogs, tabs, and menus. `scripts/sync-shadcn-ui.mjs` fetches only the named components from `https://ui.shadcn.com/r/styles/new-york/`.

The canonical supplied light/dark OKLCH tokens live in `frontend/shadcn-theme.css`. `:root` is the valid CSS selector; the supplied token values are unchanged. Each surface contains its own `src/theme.css` so independent Docker/frontend build contexts remain self-contained.

`frontend/desktop/` holds the canonical Master/Worker lifecycle templates. The clients intentionally contain local copies, not imports outside their build contexts. Update the canonical files and both client copies together. The generator preserves existing files rather than overwriting local customizations; rerunning it does not propagate edits into existing copies. The parity tests detect drift.

Authentication and product boundaries are unchanged: the website stays an account center, Master owns task operations, and Worker owns local computer operations. Bearer sessions are not persisted in localStorage; only theme preferences are.

## Motion and window lifecycle

The website uses route entrance/exit transitions, and the clients animate window entrance, exit, and tab content. Reduced-motion preferences disable the motion and the exit delay. Both clients apply their saved theme before React starts, and their static startup/recovery screens use the same semantic color tokens so a failed bundle remains readable in light and dark modes.

On Windows, native close requests open an accessible confirmation with Cancel, Keep in background, and Quit. Background hides the window without stopping the runtime. The tray icon, Open menu, and relaunching the dedicated EXE restore the existing window; the tray Quit menu still requires confirmation.

Windows Master/Worker admission runs before any service bind. A private named-pipe owner is scoped to the Windows logon SID, role, and configured UI port, not the executable directory. A duplicate queues only SHOW and exits without initializing another backend/helper. The original process forwards the bounded fixed SHOW frame through its retained helper stdin; the helper unminimizes/shows/focuses its original HWND on the UI thread. Requests before helper readiness are coalesced. Distinct roles or UI ports remain independent, and port zero deliberately requests an independent ephemeral listener. Hostname/IPv6 bind forms remain supported.

Ownership and activation use separate non-inheritable, remote-rejecting named pipes with a protected DACL granting only the current logon SID. Activation creates a fresh Tokio pipe for each connection to avoid stale EOF state on reconnect, retaining the owner even if the listener fails. The reply acknowledges a queued request, not proof of foreground focus; native checks separately verify window visibility and identity. Once disabled UI, browser fallback, or helper loss is observed, further activation is rejected rather than binding duplicate services. During pending startup a request can be acknowledged before native availability is known, even if startup subsequently fails. In browser fallback, reopen the configured loopback UI URL manually. There is no HTTP activation route or generic native command surface.

The pipe DACL is a Windows logon-session boundary, not executable authentication. Processes under the same logon SID are trusted: a malicious same-logon process can pre-create the predictable owner/activation endpoints, return a false activation acknowledgment, and receive the targeted foreground permission. Resistance to such local pipe squatting is outside this boundary; the singleton protocol does not grant Nodepool authentication or cross-user privileges.

The helper's native WebView2 message channel accepts only fixed window actions, an exact loopback sender origin, and its per-instance nonce. Messages are JSON **text**, not JavaScript objects, because the existing Wry message receiver accepts only strings. No generic Tauri permissions or HTTP shutdown endpoint are granted.

Confirmed Quit emits a private stdout marker to the owning backend. EOF or malformed markers do not request a backend shutdown. The backend also observes this signal during initialization so a stalled startup cannot prevent Quit. A small native fallback dialog remains usable when React has not loaded or its bundle fails. Its explicit viewport positioning resists frontend CSS resets, and its entrance/exit animations respect reduced motion. Pending actions, including Escape, are coalesced; failed actions restore visibility and permit retry.

## Repeatable checks

Build current assets before rendered tests:

```powershell
npm --prefix frontend run build
npm --prefix frontend/master-ui run build
npm --prefix frontend/worker-ui run build

npm --prefix frontend test
npm --prefix frontend/master-ui test
npm --prefix frontend/worker-ui test
npm --prefix frontend run test:ui-contract
node --test scripts/native-ui-bridge.test.mjs
npm --prefix frontend run test:ui
```

The browser suite serves the built applications at ports 4173–4175. `HIVEMIND_UI_SURFACE=master`, `worker`, or `website` runs only that surface. Use a single aggregate run when producing final evidence. Screenshots wait for finite entrance animations and final paint frames; they also check the main heading's current foreground color. Tests cover light/dark themes, 320px layouts, keyboard/focus behavior, resource loading, startup failures, and principal UI flows.

For an interactive Windows desktop with WebView2 installed and ports 8082/18080 free:

```powershell
cargo build --manifest-path hivemind-rs/Cargo.toml -p hivemind-bin --no-default-features --features master-webview,worker-webview --bins
node scripts/native-ui-smoke.mjs
node scripts/native-ui-runtime-smoke.mjs
```

The native harness uses real WM_CLOSE, Windows UI Automation, and clicks the actual notification-area icon. Screenshots use the client's own HWND, not the foreground desktop. It also checks early close, a missing React bundle, and parent-pipe EOF. The runtime harness launches the dedicated Master and Worker binaries in unique temporary directories with a filtered environment, updates disabled, and an isolated loopback upstream. It verifies that background preserves the real HTTP service, repeated EXE launches (including copied directories) restore the same backend/helper/HWND, minimized windows are restored, distinct roles/UI ports coexist, and Quit closes the listener and permits a fresh restart.

To verify fresh packaged clients instead of debug executables, set `HIVEMIND_NATIVE_MASTER_BUNDLE` and `HIVEMIND_NATIVE_WORKER_BUNDLE` to their separate bundle directories before running `native-ui-runtime-smoke.mjs`. The harness copies the entire bundles into isolated temporary directories and uses their bundled UI assets. Use only fresh, unlaunched bundles without credentials or runtime state. `HIVEMIND_NATIVE_EVIDENCE_DIR` optionally selects a separate evidence directory.

Browser evidence is written under `test_logs/frontend-ui/`, including `results.json`; native evidence uses `test_logs/native-ui/`, including `*-result.json`. Override these locations with `HIVEMIND_E2E_EVIDENCE_DIR` and `HIVEMIND_NATIVE_EVIDENCE_DIR` respectively. These checks do not use real account credentials, register a live worker, submit work, or verify consensus/billing settlement. Database-backed Rust tests require an explicitly attached test database; a green run without one is not settlement evidence.
