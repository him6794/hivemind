# PROJECT KNOWLEDGE BASE

## OVERVIEW
HiveMind is a Rust distributed task runtime with nodepool coordination, master API, worker execution, React/Vite UIs, an official public website + website-api, and an optional Cloudflare artifact service.

## STRUCTURE
- `hivemind-rs/` — authoritative Rust workspace and role binaries
- `proto/` — protobuf contracts
- `executor-rs/` — managed-function runtime dependency
- `frontend/` — official public website (account/download marketing entry)
- `frontend/master-ui/` — UI bundled/served with user-deployed master HTTP
- `frontend/worker-ui/` — UI for worker control HTTP
- `scripts/` — packaging and operational tooling
- `docker-compose.yml` — canonical Rust deployment topology
- `infra/` — legacy deployment definitions; do not assume current
- `cloudflare/` — separate update/artifact service

## WHERE TO LOOK
| Task | Location |
|---|---|
| Role startup | `hivemind-rs/crates/hivemind-bin/src/lib.rs` |
| Protocol | `proto/*.proto`, `hivemind-rs/crates/proto` |
| Master HTTP API | `hivemind-rs/crates/master-api` |
| Official website API | `hivemind-rs/crates/website-api` |
| Identity/CPT/VPN gRPC | `hivemind-rs/crates/node-manager`, `vpn-service` |
| Worker execution | `hivemind-rs/crates/worker-executor` |
| Deployment | `docker-compose.yml`, `.env.example`, `hivemind-rs/Dockerfile` |
| Public website UI | `frontend/` (`3080`) |
| Master UI | `frontend/master-ui` (served by master on `8082`) |
| Worker UI | `frontend/worker-ui` → worker control `18080` |
| Release | `Makefile`, `docker-compose.yml`, feature-gated `cargo build` |

## HTTP SURFACE BOUNDARIES
- **Master HTTP (`MASTER_HTTP_ADDR`, default `0.0.0.0:8082`)**
  - Exists only in the **user-deployed master** role; nodepool, worker, and website roles do not start it.
  - The master process serves both the API and bundled master-ui assets on the same listener; no separate UI service is required at runtime.
  - Used only by master-ui and downloaded requestor/master clients.
  - Not the official public product website backend.
- **Website API (`WEBSITE_HTTP_ADDR`, default `0.0.0.0:8090`)**
  - Official product backend for public register/login/CPT transfer/VPN config issuance.
  - Consumed by the public website (`frontend/` on `3080`).
  - Proxies identity/CPT/VPN through nodepool gRPC; never through a user master.
- **Worker control HTTP (`WORKER_CONTROL_HTTP_ADDR`, default `127.0.0.1:18080`)**
  - Exists only in the **worker** role and is started by the worker process.
  - The worker process serves both the control API and bundled worker-ui assets on this listener; worker-ui is not a separate runtime backend.
  - Local/operator surface for that worker node; bind it to a routable interface only when remote operator access is intended.
- Do **not** point the public website at master `:8082`.
- Do **not** treat master HTTP as a shared multi-tenant public account service.

## CONVENTIONS
- Rust is the backend source of truth; do not revive the missing `services/` legacy tree.
- Keep feature-gated role builds independently compilable: `master`, `nodepool`, `worker`, `website`.
- Bind and advertised endpoints are distinct; remote deployments require routable endpoint configuration.
- Backend authorization, not UI checks or CORS, defines privilege.
- Preserve unrelated dirty-worktree changes; never reset or checkout wholesale.

## ANTI-PATTERNS
- No unauthenticated public worker control endpoint.
- No wildcard CORS or public database/Redis exposure in production.
- No path joins/deletes from unvalidated task IDs.
- No secrets, tokens, or weak credentials in release artifacts.
- No claim of release readiness from compilation alone.
- Do not use `infra/` or other legacy packaging trees as Rust sources without reconciliation.
- No wiring official website frontend to master HTTP (`8082`).

## COMMANDS
```bash
cargo fmt --all -- --check
cargo check --workspace
cargo test --workspace
cargo build --release --no-default-features --features worker --bin hivemind-worker
cargo build --release --no-default-features --features master --bin hivemind-master
cargo build --release --no-default-features --features nodepool --bin hivemind-nodepool
cargo build --release --no-default-features --features website --bin hivemind-website-api
npm ci; npm run build; npm test
```

## NOTES
- Env overrides apply after `HIVEMIND_CONFIG` JSON load; invalid numeric env values fail closed.
- Workers need the managed runtime and reachable nodepool/torrent topology.
- UI assets must be version-compatible with the backend serving them.
- Compose: `website` depends on `website-api`; the standalone `master-ui` and `worker-ui` services are optional development previews because the master and worker processes already serve their bundled UIs.
- Website build arg is `VITE_WEBSITE_API_BASE` (injected as `VITE_API_BASE` for `frontend/`).
- Master-ui build arg remains master HTTP via `VITE_MASTER_API_BASE` / `VITE_API_BASE`.

## SERVICE CONNECTIONS AND REQUEST FLOW

The deployed topology has four role boundaries:

```text
public website (frontend :3080)
        │ HTTP /api/*
        ▼
website-api (:8090) ── authenticated gRPC ──▶ nodepool (:50051)
                                               │ database/redis, scheduler,
                                               │ worker registry, VPN, artifacts
master-ui / requestor client ── HTTP /api/* ─▶ master (:8082)
                                               └─ authenticated gRPC ──▶ nodepool
worker-ui ── HTTP /api/* ──▶ worker control (:18080)
                              ├─ local worker profile/login/register proxy
                              └─ authenticated gRPC ──▶ nodepool
worker (:50053) ◀─ authenticated execution RPC ── nodepool scheduler
```

- `master` starts its HTTP API and bundled `master-ui` together. Master forwards user-authenticated task and provider requests to nodepool; nodepool is the authority for task ownership and admin/provider authorization.
- `website-api` is the public account/CPT/VPN surface. It does not call master and must never use `MASTER_HTTP_ADDR`.
- `worker` starts worker control HTTP and bundled `worker-ui` together. Worker execution gRPC uses a separate worker-execution secret and task/worker-bound claims.
- `nodepool` is the only role that owns scheduler/database/worker-registry state. Its gRPC methods still validate the token in each request; network reachability is not authorization.
- Master task reads are user-scoped: list uses the JWT subject, while result/log/stop/artifact handlers verify the stored task owner (artifact download additionally permits configured admins). Worker/provider operations are scoped by the registered worker owner or admin.
- Worker control HTTP defaults to loopback. Expose it remotely only with an explicit bind override and an authenticated operator/network boundary; `/api/worker-info` is intentionally a local profile bootstrap endpoint and must not be exposed publicly.

## PRODUCT DEPLOYMENT MODEL

The public Hivemind platform has one authoritative `nodepool` control plane and
can have arbitrarily many independently downloaded and deployed clients:

```text
official website
  └─ account registration/login and VPN bootstrap configuration

one platform nodepool
  ├─ many worker clients (providers)
  └─ many master clients (requestors)
```

- The official website is the public account entrypoint. A user registers there
  before installing a worker or master client.
- A worker is a user-owned provider client. Its backend must automatically
  obtain the user's VPN bootstrap configuration, join the configured Headscale
  network, connect to the platform nodepool over the VPN, start its local
  worker-control backend, and serve the worker UI. The UI sends the user's
  credentials to the local worker backend; the backend authenticates through
  nodepool, registers that worker for the user, and receives work authorized for
  that account.
- A master is a user-owned requestor client. Its backend must perform the same
  automatic VPN bootstrap and nodepool connection, then serve the local master
  UI. The UI sends credentials to the local master backend; the backend logs in
  through nodepool, retains the resulting user JWT, and uses that JWT for task
  submission and task-management calls on behalf of that user.
- Master and worker deployments are not shared multi-tenant consoles. Each
  installed master belongs to the operator using it and must expose only that
  user's task and provider scope. Backend/nodepool authorization remains the
  authority; UI checks are not security boundaries.
- VPN provisioning is part of worker/master startup automation. Users must not
  be required to manually copy pre-auth keys or hand-configure a VPN after
  downloading a client. Headscale is the coordination service; Tailscale/
  WireGuard is the client data-plane connection.
- The nodepool is the shared platform control-plane endpoint for this model; it
  is not duplicated per master/worker client. It must not be exposed directly
  to the public WAN merely because clients are numerous. Clients reach it over
  the VPN overlay.
- A master that receives a user JWT from nodepool should treat that token as
  the user's delegated credential and forward it to nodepool for validation and
  authorization. Do not require every user-deployed master to possess a
  shared platform JWT signing secret; if local claim verification is needed,
  use a verification design that does not distribute the nodepool signing
  secret.
