# Master API

User-deployed requestor client HTTP surface for HiveMind.

Master is **not** the official public account service. It:

- serves the local master UI and requestor HTTP API
- logs the operator into nodepool and forwards the resulting user JWT
- does **not** require or hold the platform `JWT_SECRET`
- optionally auto-joins the platform VPN via website-api on login

## Runtime model

```text
master-ui ── HTTP /api/* ──▶ master (:8082)
                              ├─ optional website-api VPN bootstrap
                              └─ authenticated gRPC ──▶ nodepool
```

- Nodepool remains the authority for token validation and authorization.
- Master only extracts structural JWT claims locally (subject / expiry) so it can
  rate-limit and route; signature verification stays with nodepool.
- Account registration belongs on the official website / website-api.
  `POST /api/register` on master is disabled (`410 Gone`).

## Important endpoints

Public:

- `GET /health`
- `POST /api/login` — login through nodepool; may auto-issue VPN config first
- `POST /api/register` — disabled; register on the official website

Authenticated (Bearer user JWT from login):

- `GET /api/tasks`, `POST /api/tasks`, `POST /api/tasks/quote`
- `GET /api/tasks/:task_id/log|result`, `POST /api/tasks/:task_id/stop`
- `GET /api/tasks/:task_id/artifact/download`
- `GET /api/balance`, `GET /api/workers`
- provider / admin routes are still proxied; nodepool enforces ownership/admin scope

## Configuration

| Variable | Purpose |
|---|---|
| `MASTER_HTTP_ADDR` | Local HTTP bind (default `0.0.0.0:8082`) |
| `NODEPOOL_GRPC_ENDPOINT` | Reachable nodepool gRPC host:port (usually over VPN) |
| `MASTER_UI_DIR` | Bundled master-ui asset directory |
| `MASTER_WEBSITE_API_BASE` / `WEBSITE_API_BASE` | HTTPS origin of the deployed Rust Website API; must expose `/api/login` and protected `/api/vpn/config` for automatic enrollment |
| `MASTER_VPN_AUTHKEY` | Optional role-scoped Headscale preauth key; keyed startup joins before Nodepool-dependent work proceeds |
| `MASTER_VPN_LOGIN_SERVER` / `HEADSCALE_LOGIN_SERVER` | Headscale login server for VPN join |
| `MASTER_VPN_HOSTNAME` | Optional Tailscale hostname |
| `VPN_STARTUP_TIMEOUT_SECS` | Bounded keyed VPN/Nodepool startup deadline (1-300 seconds; default 120) |
| `MASTER_VPN_STATE_DIR` | Optional userspace Tailscale state dir |
| `MASTER_VPN_TAILSCALE_BIN` | Optional path to `tailscale` binary |
| `MASTER_CORS_ALLOWED_ORIGINS` | Explicit CORS allow-list (no wildcard) |

Master does **not** need `JWT_SECRET`.

## VPN bootstrap

Master and Worker are user-deployed processes on a local suitable host. The
Orange Pi platform host is reserved for Nodepool, Website API, Headscale,
PostgreSQL, and Redis; it must not also host the downloaded Master or Worker.

For an operator-provisioned remote Master:

1. Set `NODEPOOL_GRPC_ENDPOINT` to the Nodepool gRPC address reachable through
   Headscale.
2. Set `MASTER_VPN_AUTHKEY` to a role-scoped Headscale preauth key and, when
   needed, set `MASTER_VPN_LOGIN_SERVER` or `HEADSCALE_LOGIN_SERVER`.
3. Start Master. It joins Headscale, waits for the Nodepool gRPC transport
   handshake, and only then exposes the startup path that depends on Nodepool.
4. Master uses the validated effective endpoint; on Windows this can be the
   localhost bridge exposed by the embedded `libtailscale.dll` client.
5. Log in with website credentials when the Nodepool application token is not
   pre-provisioned. User-driven enrollment remains available in this mode.

For interactive enrollment without `MASTER_VPN_AUTHKEY`, configure
`MASTER_WEBSITE_API_BASE` or `WEBSITE_API_BASE` with the HTTPS origin of the
deployed Rust Website API. That service must expose `POST /api/login` and the
protected `POST /api/vpn/config`; the official Next BFF is not a substitute
unless it explicitly serves that route. After login, the local `/api/vpn/bootstrap`
route forwards the bearer JWT to the Website API, consumes the one-time
Headscale key in process memory, joins the overlay, and waits for Nodepool
protocol readiness before updating the Master gRPC endpoint. The same flow is
used for a restored session; persisted libtailscale state is attempted before
issuing a new key. An expired JWT requires login again.

If `MASTER_VPN_AUTHKEY` is absent, Master keeps the local UI available while
remote operations remain gated until authenticated enrollment is ready. The
local status route exposes only nonsecret state. `HEADSCALE_API_KEY` is a
platform-side secret and is never placed in a Master package, browser storage,
or status response. Automatic update/download behavior is not implemented by
this startup slice.

Local compose can omit `MASTER_WEBSITE_API_BASE` when Master and Nodepool already
share a development network, but that is not evidence of the external Headscale
Master/Worker topology.

## Build / test

```bash
cargo check -p hivemind-bin --no-default-features --features master --bin hivemind-master
cargo test -p hivemind-master-api --lib
cargo test -p hivemind-bin --no-default-features --features master --lib
```
