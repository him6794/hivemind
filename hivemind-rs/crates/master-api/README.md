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

- `GET /api/tasks`, `POST /api/tasks`, `POST /api/tasks/upload`, `POST /api/tasks/quote`
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
| `MASTER_WEBSITE_API_BASE` | Official website-api base for automatic VPN issue on login |
| `MASTER_VPN_AUTHKEY` | Optional operator override; skips website-api issue when set |
| `MASTER_VPN_LOGIN_SERVER` / `HEADSCALE_LOGIN_SERVER` | Headscale login server for VPN join |
| `MASTER_VPN_HOSTNAME` | Optional Tailscale hostname |
| `MASTER_VPN_STATE_DIR` | Optional userspace Tailscale state dir |
| `MASTER_VPN_TAILSCALE_BIN` | Optional path to `tailscale` binary |
| `MASTER_CORS_ALLOWED_ORIGINS` | Explicit CORS allow-list (no wildcard) |

Master does **not** need `JWT_SECRET`.

## VPN bootstrap

For a downloaded remote master:

1. Set `MASTER_WEBSITE_API_BASE` to the public website-api.
2. Set `NODEPOOL_GRPC_ENDPOINT` to the nodepool address reachable over the VPN.
3. Start master; UI comes up immediately (nodepool connect is lazy).
4. Operator logs in with website credentials.
5. Master calls website-api `/api/vpn/config`, joins Headscale automatically, then
   logs into nodepool and returns the user JWT to the UI.

Local compose can omit `MASTER_WEBSITE_API_BASE` when master and nodepool already
share a network.

## Build / test

```bash
cargo check -p hivemind-bin --no-default-features --features master --bin hivemind-master
cargo test -p hivemind-master-api --lib
cargo test -p hivemind-bin --no-default-features --features master --lib
```
