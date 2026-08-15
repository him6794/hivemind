# Hivemind Architecture Notes For Agents

This file records the intended product and deployment boundaries. Future agents must preserve these boundaries unless the user explicitly changes the architecture.

## Workflow

Treat each feature as a checkpoint, not a batch. After completing a feature and confirming its tests pass, commit the finished work to the local git repository before starting the next feature. This keeps progress recoverable and lets each change be reviewed as a self-contained unit.

- Run the tests for the feature you just finished before committing. `cargo test` from `hivemind-rs` covers the Rust workspace; run the relevant crate's tests or the frontend contract suite when the change is narrower.
- Commit only the feature you just finished and verified. Do not accumulate multiple unfinished features in one commit, and do not fold unrelated edits into the same commit.
- Commit to the local repository only. Do not push to any remote unless the user explicitly asks.
- Each commit message should state the feature and that tests pass. Keep commits focused enough to revert individually.

## Deployment Model

Hivemind has three user-facing surfaces:

1. Official website, deployed by Hivemind.
2. Master node UI/API, deployed by users.
3. Worker node UI/API, deployed by users.

Officially deployed infrastructure:

- `nodepool`: the central coordination and account/billing service.
- `website backend`: the backend for the official website.
- `official website`: the public website and account-facing web app.

User-deployed infrastructure:

- `master node`: local task submission and task-control service.
- `worker node`: local compute-provider service.

## Trust Model

The deployment split in the previous section implies a hard trust boundary. Future agents must treat it as load-bearing:

- `nodepool` is the only trusted authority. It owns account state, balances, task state, worker registration, scheduling, and billing settlement.
- `master node` and `worker node` are user-deployed and therefore untrusted clients. Their processes, configuration, UIs, and local APIs are under the user's control and may be modified, replaced, or forged.
- The only thing nodepool may trust from a master or worker is an authenticated token issued by nodepool itself. Nodepool identifies master and worker callers solely by validating that token; it must not trust caller-supplied identity, address, capacity, pricing, result, or billing assertions without independently verifying or persisting them server-side.
- A worker's local HTTP API is for the local worker UI only. It is reachable only on the worker host and is never a nodepool trust anchor. Nodepool must not accept worker registration, capacity, result, or usage state over a worker's local HTTP API; those flow over the authenticated worker<->nodepool gRPC path.
- Results, resource usage, and billing figures reported by a worker are claims, not facts. Nodepool persists them only after the token proves the caller is the assigned worker, and it reserves the right to re-check, cap, or reject them. The same applies to any task submission metadata coming from a master.

If a change would let nodepool trust an unvalidated assertion from a master or worker, or would route a trust-sensitive operation (registration, billing, capacity, trust control) through another untrusted client instead of the authenticated gRPC path, that change violates this model.

## Request Flow

Official website path:

```text
Browser
  -> Official Website
  -> Website Backend
  -> Nodepool
```

Master node path:

```text
Browser
  -> Master Local UI
  -> Master HTTP API
  -> Nodepool
```

Worker node path:

```text
Browser
  -> Worker Local UI
  -> Worker HTTP API
  -> Nodepool / local worker runtime
```

## Official Website Responsibilities

The official website is a public product front door plus account center. It must not become a task or worker operations console.

Allowed official website capabilities:

- Public marketing and product explanation.
- Register and login.
- Show account balance.
- Account/billing actions such as transfer, payment, or CPT-related account operations when supported by backend APIs.
- Provide download, install, and deployment guidance for Master and Worker nodes.
- Link to documentation.

Not allowed on the official website:

- Submit tasks.
- List task progress.
- Stop or control tasks.
- Register worker nodes.
- Control worker state.
- Manage worker resources, pricing, or availability.
- Act as the user's Master UI or Worker UI.

## Website Backend Responsibilities

The website backend is the official website's BFF/API gateway. Browser requests should go to same-origin website backend endpoints, and the website backend calls `nodepool` server-side.

Rules:

- Do not expose `nodepool` directly to browsers.
- Do not require browser-side `NEXT_PUBLIC_API_BASE` for nodepool access.
- Use a server-side nodepool target such as `WEBSITE_NODEPOOL_GRPC_ADDR`.
- Keep website backend endpoints limited to official website capabilities.
- Do not add task submission, task status, or worker-management endpoints to the official website backend unless the architecture is explicitly changed.

## Nodepool Responsibilities

`nodepool` is officially deployed and is the central source of truth.

Responsibilities:

- Account registration and authentication.
- Token validation.
- Balances and account/billing state.
- Task coordination APIs used by Master nodes.
- Worker registration and worker status APIs used by Worker nodes.
- Scheduling, task state, billing settlement, and worker coordination.

## Master Node Responsibilities

The Master node is deployed by users. It is the task-entry and task-control surface.

Responsibilities:

- Start a local Master UI.
- Start a local Master HTTP API.
- Let users login with their Hivemind account.
- Submit tasks from Master UI to Master HTTP API.
- Forward task submission to `nodepool`.
- Show task progress.
- Stop or control tasks.
- Show task history and task results relevant to that user.

If a feature involves task submission or task progress, it belongs in Master UI/API, not the official website.

## Worker Node Responsibilities

The Worker node is deployed by users. It is the compute-provider surface.

Responsibilities:

- Start a local Worker UI.
- Start a local Worker HTTP API.
- Let users login with their Hivemind account.
- Register the local worker with `nodepool`.
- Configure worker resources, price, and availability.
- Start, stop, or control local worker execution.
- Report status and resource usage.

If a feature involves local worker control, worker pricing, worker capacity, or worker registration, it belongs in Worker UI/API, not the official website.

## Frontend Product Boundary

The official website should feel like a professional public website and account center.

Do:

- Use clear public-facing product language.
- Keep the homepage simple and non-technical.
- Direct users to deploy Master for task submission.
- Direct users to deploy Worker for compute supply.

Do not:

- Describe internal repository structure on public pages.
- Put low-level service names in homepage copy.
- Turn public pages into README-style architecture docs.
- Add Master or Worker operational controls to the official website.

## Native Worker Execution Boundary

Hivemind must support workers running natively on Windows without requiring the user to operate a Linux VM. Production general-compute execution is therefore platform-specific:

- Linux workers use `production_sandboxed_oci` with rootless OCI namespaces, cgroup v2, seccomp, read-only root, explicit artifact mounts, and deny-all networking.
- Windows workers use a distinct Windows-native production sandbox mode backed by Windows container/HCS process isolation. Windows must not reinterpret Linux OCI policy as a Windows sandbox.
- `reference_direct` is reference/test-only and is never a production fallback.
- Docker Desktop, WSL, Linux VMs, and remote Linux hosts may be used as development or validation infrastructure, but they do not constitute native Windows production isolation evidence.
- If the required platform isolation provider, pinned operator image/assets, resource limits, filesystem policy, or network policy cannot be established, the worker must fail closed as `backend_unavailable`.

A Windows-native production sandbox is not equivalent to a Job Object alone. Job Objects provide lifecycle and process-tree cleanup, but production isolation also requires a verified container boundary, restricted identity, read-only root, explicit artifact mounts, deny-by-default network, bounded CPU/memory/process resources, reparse-point-safe operator roots, and hostile-workload evidence. No implementation may substitute a direct host process, `cmd.exe`, PowerShell, Docker CLI, WSL, or an unconfined AppContainer for this boundary.

The release gate must distinguish policy/specification tests, mocked lifecycle tests, and real Windows HCS/container E2E evidence. A skipped or unavailable Windows isolation provider is not a passing production E2E result.

## Current Repo Note

The repository may still contain legacy or all-in-one services such as `master-api` and `hivemind-bin all`. Those can exist for development, compatibility, or local deployment, but official website development must follow the deployment model above.
