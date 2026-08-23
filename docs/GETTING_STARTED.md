# HiveMind Getting Started

## Prerequisites

- Docker Desktop (or Docker Engine) with the `docker compose` command
- Windows PowerShell 5.1 or PowerShell 7 for the release smoke harness
- OpenSSL with Ed25519 support for generating worker execution keys
- Node.js 20.9+ for Next.js frontend builds and Playwright browser QA
- Rust stable when building or testing the backend outside Docker

The release uses these browser-facing endpoints. The ports must be available on
the host:

| Surface or API | Default URL | Operator purpose |
| --- | --- | --- |
| Official Site | `http://localhost:8080` | Public product site and account center |
| Master UI | `http://localhost:3000` | Submit managed-function tasks and inspect their status, logs, results, and artifacts |
| Worker UI | `http://localhost:3001` | Inspect the local worker and register its capacity |
| Master API | `http://localhost:8082` | Authenticated HTTP gateway used by the browser surfaces |
| Worker control | `http://localhost:18080` | Local worker profile/control API used by Worker UI |

## Configure

Raw `docker compose` requires the following release values:

- `POSTGRES_PASSWORD`: a strong database password.
- `JWT_SECRET`: a unique, non-default application signing secret.
- `WORKER_EXECUTION_PRIVATE_KEY_PEM`: an Ed25519 private key used by nodepool
  to sign worker execution tokens.
- `WORKER_EXECUTION_PUBLIC_KEY_PEM`: the public key matching that private key;
  the worker uses it to verify execution tokens.
- `WORKER_NODEPOOL_TOKEN`: optional. Leave it blank when the provider will log
  in and register through Worker UI.

Start from `.env.example` for a persistent operator configuration. Generate a
matching Ed25519 key pair with OpenSSL:

```powershell
openssl genpkey -algorithm Ed25519 -out worker-execution-private.pem
openssl pkey -in worker-execution-private.pem -pubout -out worker-execution-public.pem
```

Do not commit `.env` or private keys. In an environment file, PEM values must be
encoded in the form supported by the deployment environment (the repository
template uses one line with literal `\n` separators).

For local release verification, `scripts/release-stack-smoke.ps1` is
self-contained: when values are absent it generates an ephemeral
`POSTGRES_PASSWORD`, a non-default `JWT_SECRET`, and a matching Ed25519 private
and public key pair. It preserves user-supplied values exactly and restores the
calling process environment afterward. It also assigns Redis and PostgreSQL
collision-free ephemeral host ports when `REDIS_HOST_PORT` and
`POSTGRES_HOST_PORT` are unset. The five product/API ports listed above remain
fixed and must be free.

## External operator topology

For the deployed platform topology, keep the Orange Pi limited to the control
plane and persistent services:

```text
Orange Pi ARM64
  Nodepool + Website API + Headscale + PostgreSQL + Redis

Local suitable host
  Master + Worker
```

Master and Worker connect to the Orange Pi Nodepool through the Headscale
overlay. Do not deploy either downloaded client, a managed prover, or the
platform `HEADSCALE_API_KEY` on the Orange Pi as a substitute for that topology.
The API key remains server-side and is never distributed in client packages.

To make a local Windows Master or Worker enroll automatically, set
`WEBSITE_API_BASE` (or the role-specific `MASTER_WEBSITE_API_BASE` /
`WORKER_WEBSITE_API_BASE`) to the HTTPS origin of the deployed Rust Website API.
That origin must expose both `POST /api/login` and the protected
`POST /api/vpn/config`; the official Next BFF is not a VPN-config endpoint unless
that route is explicitly added there. The downloaded Worker package exposes
`WEBSITE_API_BASE` in `.env.worker.example`; the runtime also has a baked-in
public default for deployments that intentionally use it.

After the local Master or Worker starts, its local UI/control surface is available
without a VPN key. On the first authenticated login, the local process forwards
the bearer JWT to the Website API, consumes the returned one-time Headscale key
in memory, joins Headscale, and waits for the Nodepool gRPC protocol probe. Only
then do Master remote operations or Worker registration proceed. The browser and
client package never receive `HEADSCALE_API_KEY`, and no password or reusable
Headscale key is persisted.

On restart, the process first attempts to rehydrate its persisted libtailscale
state. A restored valid UI session can request a fresh one-time key if that state
was revoked or expired. An expired JWT returns to explicit login rather than
falling back to stored credentials. For unattended operator startup, an optional
role-scoped `MASTER_VPN_AUTHKEY` or `WORKER_VPN_AUTHKEY` remains supported; set
`HEADSCALE_LOGIN_SERVER` and `NODEPOOL_GRPC_ENDPOINT` as needed. Keyed startup waits for the Nodepool gRPC transport handshake and fails closed if the overlay
path is unavailable. Set `VPN_STARTUP_TIMEOUT_SECS` between 1 and 300 seconds
when the default 120-second readiness deadline needs adjustment. A login-server-only setting does not auto-enroll.

Automatic update/download behavior is not implemented yet. The current startup
slice only adds authenticated Headscale enrollment, persisted-state rehydration,
and readiness ordering. The root all-in-one Compose stack and
`infra/docker-compose.vpn.yml` are local/legacy development configurations, not
formal external Headscale evidence.

```bash
cd hivemind-rs
cargo build --release
```

```bash
cd frontend
npm install
npm run build

cd master-ui
npm install
npm run build

cd ../worker-ui
npm install
npm run build
```

If you prefer repository-level commands, the root `Makefile` also provides:

```bash
make build-frontend
```

This builds all three release surfaces:

- `frontend/` - official site
- `frontend/master-ui/` - customer dashboard
- `frontend/worker-ui/` - worker console

## Run

### Release smoke and browser proof

From the repository root, validate packaging, build the complete stack, wait
for all five HTTP endpoints, and keep the containers running for browser QA:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/release-stack-smoke.ps1 -KeepRunning
```

The harness prints every generated setting without printing secret values. Omit
`-KeepRunning` when only an automated build/start/health/cleanup smoke is needed.
Use `-CheckOnly` to validate Compose packaging and environment generation
without starting containers.

In a second PowerShell session, run the serial Playwright journey. It creates a
fresh account, checks Official Site validation and balance, registers the local
worker, submits/cancels/completes tasks in Master UI, and captures downloads:

```powershell
cd frontend
npm ci
npm run test:e2e
```

To place screenshots, traces, the action transcript, downloads, and JSON results
in a specific evidence directory:

```powershell
$env:HIVEMIND_E2E_EVIDENCE_DIR = "D:\release-evidence\hivemind"
cd frontend; npm run test:e2e
```

The default evidence directory is
`.omo/evidence/task-8-release-grade-frontends-app-and-site/`. Override the
browser URLs with `HIVEMIND_SITE_URL`, `HIVEMIND_MASTER_UI_URL`, and
`HIVEMIND_WORKER_UI_URL` when verifying a remote candidate.

After verification, return to the repository root:

```powershell
docker compose down
```

Supply a persistent `.env` before using `-KeepRunning` when possible. If the
smoke harness generated all required values and then restored the environment,
raw Compose can require placeholder values for its configuration parser during
`docker compose down`; the values do not alter the already-created project's
identity.

### Managed proof live E2E (protected environment only)

`scripts/managed-proof-live-e2e.ps1` exercises the full external chain —
Website login, enrollment credential redemption with the server-assigned
Worker identity, managed task submission, remote proof, independent Nodepool
verification, billing/settlement, and result/log retrieval — in enforce mode.
It runs only against a real external deployment that can reach the Website API,
Nodepool transport, and Provider. Local Compose, Docker, WSL, SSH, socat, or
direct-host reachability are not substitutes for that evidence:

```powershell
scripts/managed-proof-live-e2e.ps1 `
  -WebsiteApiBase https://<website-origin> `
  -MasterApiBase http://<master-api> `
  -Username <account> -Password <password> `
  -TaskSourcePath examples/managed-add.hdsl `
  -TaskInputJson '{"left":20,"right":22}'
```

Evidence lands in `test_logs\managed-proof-live-e2e\` and is redacted by
construction: identifiers, states, timings, policy decisions, verification
outcomes, settlement amounts, and digests — never passwords, JWTs,
enrollment credentials, Headscale keys, proof tokens, source, input, or raw
proof envelopes.

### Development and manual runtime

The repository root `README.md` documents the other development and runtime
entry points.

```bash
make dev
```

For a manual run, set the database, Redis, and JWT environment variables, then
start `hivemind-bin` in the mode you need (`all`, `master`, `nodepool`, or
`worker`).

To run the full local release stack, including the official site and both app
surfaces without the smoke harness, first supply the required secrets and key
pair described above:

```bash
docker compose up --build
```

The default local ports are:

- `http://localhost:8080` - official site
- `http://localhost:3000` - master UI
- `http://localhost:3001` - worker UI
- `http://localhost:8082` - master API
- `http://localhost:18080` - worker control API

## Verify

```bash
curl http://localhost:8082/health
```

For frontend release verification:

```bash
$env:WEBSITE_NODEPOOL_GRPC_ADDR = "127.0.0.1:50051"
$env:VITE_API_BASE = "http://127.0.0.1:8082"
$env:VITE_WORKER_CONTROL_BASE = "http://127.0.0.1:18080"
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/release-frontend-smoke.ps1
```

## Trust Boundary

Nodepool is the only trusted authority for accounts, balances, task state,
worker registration, scheduling, and billing. The Official Site is only a
public product site and account center; task operations belong in Master UI and
worker operations belong in Worker UI. Browser code must not connect directly
to nodepool. The Official Site backend reaches nodepool server-side through
`WEBSITE_NODEPOOL_GRPC_ADDR`, while browser-facing traffic uses the Master API
or the local worker control endpoint.

The same rule governs billing for `managed-function-v0` tasks. A Worker's
reported usage is a claim, so nodepool settles only from a RISC Zero proof it
verifies itself, in its own bounded subprocess, against a pinned guest image ID.
An unproven or unverifiable managed execution is failed, never settled. Workers
that run managed tasks therefore need the prover sidecar described in the README
under "Managed-function proving"; `MANAGED_PROOF_ROLLOUT_MODE` defaults to the
fail-closed `enforce`.

## Troubleshooting

- **`OpenSSL is required`**: install OpenSSL and ensure `openssl` is on `PATH`,
  or supply a matching `WORKER_EXECUTION_PRIVATE_KEY_PEM` and
  `WORKER_EXECUTION_PUBLIC_KEY_PEM`.
- **Compose reports a missing variable**: raw Compose does not generate
  secrets. Fill `POSTGRES_PASSWORD`, `JWT_SECRET`, and both worker execution key
  variables, or use the release smoke harness.
- **A frontend or API port is already allocated**: free 8080, 3000, 3001, 8082,
  or 18080. Only Redis/PostgreSQL infrastructure host ports become
  collision-free ephemeral ports during smoke.
- **An endpoint times out**: run `docker compose ps` and
  `docker compose logs <service>`; the smoke harness treats any missing surface
  or unexpected health payload as a failure.
- **Worker registration fails**: confirm
  `http://localhost:18080/api/worker-info` responds, the public key matches the
  nodepool private key, and login credentials are valid. A blank
  `WORKER_NODEPOOL_TOKEN` is expected for UI-driven registration.
- **Every managed-function task fails**: the worker could not produce a
  verifiable proof. Check that `packaging/managed-prover/` held the sidecar when
  the worker image was built, that `MANAGED_PROVER_EXECUTABLE` points at it, and
  that `MANAGED_PROVER_TIMEOUT_SECS` exceeds the ~570-580 second proving time.
  `/api/admin/managed-proof/metrics` reports the rejection counters, and the
  admin audit log carries a `managed_proof_verification` entry per decision. A
  sidecar whose embedded guest does not match the nodepool trust pin is rejected
  on every task — regenerate the pin and receipt fixture after any guest change.
- **Playwright cannot launch a browser**: install Edge/Chrome on Windows or the
  Playwright browser dependencies on other platforms; optionally set
  `HIVEMIND_PLAYWRIGHT_CHANNEL`.
- **A browser test fails**: inspect the configured evidence directory for the
  Playwright trace, screenshot/video, JSON report, and
  `release-flow-actions.txt` before rerunning.

## Next Steps

- Read `docs/ARCHITECTURE.md` for the current workspace layout
- Use `make test` for the main Rust workspace test pass
- Use `make build-frontend` to build all three frontend surfaces
