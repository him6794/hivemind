# Platform validation guide

This guide describes what different validation evidence establishes. It is not a live run log or a claim that any external release gate has passed. Follow the repository [AGENTS.md](../AGENTS.md) for current commands and [AGENT.md](../AGENT.md) for the trust and deployment boundaries.

## Interpret evidence at its actual scope

- Unit and contract tests establish only the behavior covered by their assertions. Database-backed settlement tests require a real configured test database; an environment skip is not settlement evidence.
- A compile, package, static-contract, or preflight check establishes build or prerequisite properties. It does not establish that a workload executed on the intended provider.
- A mock or process-level fake-runner test can validate request routing, result handling, and lifecycle contracts. It does not prove container isolation, host policy enforcement, or provider execution.
- Local Compose and browser smoke tests establish the tested local service and UI flow. They do not establish the external Headscale path, Linux rootless OCI isolation, or native Windows HCS execution.
- For provider-level claims, preserve the result produced by the corresponding operational harness and tie it to the exact release artifact and environment. Do not infer a live pass from implementation, packaging, or another environment's evidence.

## Managed functions and Windows DSL execution

`managed-function-v1` runs in the closed, cross-platform interpreter. A Windows DSL test exercises that interpreter and its bounds; it does not require Windows Containers or HCS and is not evidence for Windows general-compute isolation. The frozen `managed-function-v0` contract remains readable for historical compatibility, not new submissions.

Validate interpreter semantics and limits separately from Nodepool-coordinated consensus and settlement. Consensus evidence must establish distinct replica execution, result/quorum validation, and Nodepool-authorized settlement; a single-Worker result or a Worker usage claim is not a substitute. See [Managed Function Runtime](MANAGED_FUNCTION_RUNTIME.md) and [managed consensus](managed-consensus-state.md).

## General-compute isolation

### Linux rootless OCI

Production Linux general compute requires the actual rootless OCI path and operator-owned, pinned assets. The reviewed policy includes user, pid, mount, and network namespaces; cgroup v2; `no_new_privileges`; a read-only root; explicit safe artifact/scratch mounts; deny-all networking; and a digest-pinned seccomp profile with a default-deny action. The runner, image, bundle/rootfs, state root, and profile must match the approved registration.

The OCI harness's `-CheckOnly` mode is a fail-closed preflight, not an execution test. Process-level fake runners, image probes, Compose packaging checks, and preflight success do not prove host isolation or a completed multi-process task. Use the [reviewed OCI fixture guide](general-compute-oci-fixture.md) and [`scripts/general-compute-oci-e2e.ps1`](../scripts/general-compute-oci-e2e.ps1) with the operator-provisioned fixture and case plan when validating the real path; retain its generated evidence.

### Native Windows HCS

Windows general compute is a distinct `production_sandboxed_windows` path backed by native Windows Containers/HCS. It is not the closed DSL interpreter and must not fall back to a direct host process, Docker Desktop, WSL, or a Linux VM. Validate it on a native Windows host with the Windows Containers feature and `vmcompute` available, the packaged operator HCS runtime bundle, and a real task executed through HCS. The [`scripts/windows-hcs-e2e.ps1`](../scripts/windows-hcs-e2e.ps1) gate checks prerequisites and records prerequisite evidence only; it does not itself execute a task. Do not report its success as HCS workload evidence.

## External Headscale flow

The external network check must exercise the deployed topology: Nodepool, Website API, Headscale, PostgreSQL, and Redis on the control-plane host; Master and Worker on a suitable separate host. Use the real authenticated enrollment path: sign in through the deployed Website API, obtain the protected one-time VPN enrollment material, join Headscale, complete the Nodepool gRPC readiness handshake, and register the Worker. Then exercise the intended task and verify its result, logs, quorum/usage, and Nodepool settlement as applicable.

Local Compose networking, a direct Nodepool endpoint, VPN-disabled startup, SSH or port forwarding, and an already-enrolled/directly reachable peer can help diagnose components, but none substitutes for testing authenticated enrollment and communication over the external Headscale path. See [Getting Started](GETTING_STARTED.md) and [Architecture](ARCHITECTURE.md) for the deployment and enrollment contracts.

## Signed client updates

The native Windows client has signed-update verification, package validation, activation, and rollback/recovery wiring. This implementation is distinct from proving that a release channel is operational: a release check must use the approved signed metadata and package endpoint, validate the exact packaged client on a clean host, and exercise activation and recovery. Missing or invalid signed metadata must leave the installed client unchanged; an unsigned fallback is not acceptable. See the signed-update boundary in [Architecture](ARCHITECTURE.md#目前狀態).

## Fail-closed outcomes and operational entry points

Missing credentials or readiness, unsupported providers, absent or mismatched operator assets, invalid signatures, unavailable isolation primitives, or failed quorum are blocked/failed outcomes—not passes and not permission to use a weaker execution path. A fail-closed result proves only that the refusal condition worked; it does not prove the provider or workload succeeded.

Use the existing operational checks for the claim being made:

- Local release stack and browser flow: [Getting Started](GETTING_STARTED.md), [`scripts/release-stack-smoke.ps1`](../scripts/release-stack-smoke.ps1), and the frontend E2E command documented there.
- OCI execution: [reviewed OCI fixture guide](general-compute-oci-fixture.md) and [`scripts/general-compute-oci-e2e.ps1`](../scripts/general-compute-oci-e2e.ps1).
- Windows HCS prerequisites: [`scripts/windows-hcs-e2e.ps1`](../scripts/windows-hcs-e2e.ps1); follow it with actual HCS task execution before making an execution claim.
- Documentation contract: [`scripts/release-docs.Tests.ps1`](../scripts/release-docs.Tests.ps1).
