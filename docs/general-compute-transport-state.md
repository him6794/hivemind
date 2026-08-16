# General-compute transport state

- Goal: complete the `general-compute-v1alpha1` authenticated CAS/chunk transport without restoring Monty.
- Status: running; the runtime local CAS boundary, protobuf chunk envelopes, authenticated Worker and Nodepool source-upload services, the Nodepool Prepare-to-Upload client path, Nodepool-owned immutable artifact lifecycle state, per-Worker durable transfer state, explicit reference/production execution modes, task-specific operator-owned OCI bundle materialization, and a versioned production result envelope are wired. Cross-worker resume remains an authenticated Nodepool re-upload, not shared Worker storage.
- Cross-worker coordination slice: Nodepool persists an active/revoked transfer lease with monotonically increasing generation per task/attempt/Worker assignment. The generation is carried by the signed Worker execution token and the Prepare/Upload/Resume envelopes. Production Workers now query Nodepool's authoritative lease RPC before each general-compute admission or chunk operation, so a Worker that missed a revoke cannot replay an old token after reassignment.
- Current step: prepare and run the next multi-process/container OCI execution checkpoint with an operator-owned runner/rootfs; the Postgres-backed settlement *unit* checkpoint is complete and its temporary test container was removed. The real multi-process fixture is still external; do not claim production readiness from the process-level fake-runner fixture.
- Compose deployment boundary checkpoint (2026-08-14): the Worker now receives a
  fixed production registry path (`/etc/hivemind/general-compute/backends.json`)
  from a named read-only `worker-general-compute-config` volume and a separate
  mutable state/CAS root at `/var/lib/hivemind/general-compute`. The registry
  remains optional at startup, so an absent or malformed operator file fails
  production routing closed; no host path is inferred. The release contract
  parses the resolved Compose JSON and checks both mount modes. This validates
  packaging/wiring only; real rootless OCI/container execution and operator
  rootfs provisioning remain external gates.
- Operator OCI E2E preflight checkpoint (2026-08-14): commit `1e6d513` adds
  `scripts/general-compute-oci-e2e.ps1` plus its contract test. The harness
  validates the operator registry, absolute bundle/rootfs/artifact/runner paths,
  pinned runner digest, rootless namespace/cgroup/no-new-privileges/read-only
  root/network-deny policy, default-deny `SCMP_ACT_ERRNO` seccomp digest, and an
  isolated Compose project. `-CheckOnly` is safe by default; `-Run` requires an
  explicit opt-in and a reviewed Postgres-backed task fixture, and currently
  refuses to start containers until that fixture is wired. This is a fail-closed
  deployment preflight, not real OCI isolation or multi-process completion
  evidence.
- OCI runner state-root checkpoint (2026-08-14): commit `4dfe4b0` adds the
  required operator `runner_state_root` to production backend registration,
  passes it from Worker dispatch to the pinned launcher, and emits it as a
  direct `--root` argument. Relative, missing, parent-traversal, non-directory,
  and symlink roots fail closed. The materialized production path now requires
  this binding; legacy fake-runner bundle tests remain explicitly process-level.
  This is an invocation/state-boundary fix, not evidence of host rootless
  namespaces, seccomp enforcement, or multi-process Postgres completion.
- Settlement slice (2026-08-14): Nodepool now treats the alpha Worker usage envelope as an `unverified` claim and persists immutable settlement provenance (`execution_id`, `attempt_id`, idempotency key, request digest, billing/cost versions, usage claim, and evidence level). The billed amount remains the Nodepool-owned fixed reservation (`max_cpt`); a Worker claim cannot choose variable pricing. Completion fails closed if the request/result identity, approved `billing-v1`/`cost-v1` versions, status, evidence level, artifact root, or usage policy does not match.
- Worker routing fixture (2026-08-14): a process-level Worker test now materializes a task-specific bundle, canonicalizes the artifact bind root (including Windows extended-path spelling), invokes an operator-owned pinned fake runner, and consumes a valid `general-compute-result-v1` envelope. The fixture is deliberately labelled non-container E2E; real rootless OCI/container isolation remains an external deployment gate.
- Completed implementation slice (2026-08-14): `BackendRegistration.execution_mode` is persisted in the trusted capability snapshot and enforced by `ReferenceBackendExecutor`; a production registration cannot enter the direct reference adapter. Worker production dispatch requires a matching mode, pinned production registration, operator-owned task roots, verified source/input materialization, and a canonical source-plus-input digest before bundle launch.
- Production bundle/result slice is implemented: Worker startup snapshots the production/capability registries, task-specific bundle creation copies only a real operator rootfs template and rejects task-root/rootfs/config symlinks, declared artifact mounts are checked against materialized regular files, and the runner stdout decoder requires `general-compute-result-v1` with validated output manifest/root, status, digest, and usage claims. Existing legacy sandbox fixtures still use the fixed `/hivemind/artifacts` contract; materialized production launches use the canonical task artifact root. Multi-process/container E2E and operator-owned runner deployment validation remain open.
- Completed this round: scheduler capability JSON fixtures now carry `execution_mode`; Worker RED/GREEN coverage proves production capability cannot fall back to the reference executor and fails closed for missing production configuration, rootfs, or runner; production policy rejects unrequested artifact mounts; task bundle/rootfs/config symlink writes are rejected; and `input_sha256` now binds to the canonical, length-framed source-plus-input bytes actually materialized by the Worker.
- Verification this round: `cargo test -p hivemind-worker-executor --lib production_worker_routes_materialized_bundle_to_operator_runner --target x86_64-pc-windows-gnu --locked` passed; `cargo test -p hivemind-task-scheduler --lib --target x86_64-pc-windows-gnu --locked` passed (117 passed, 1 intentional ignored); `cargo test -p hivemind-database --lib task_migrations_create_general_compute_settlement_table --target x86_64-pc-windows-gnu --locked` passed against the temporary Postgres container; `cargo test -p hivemind-task-scheduler --lib general_compute_completion_persists_nodepool_settlement_provenance --target x86_64-pc-windows-gnu --locked` passed against the same temporary Postgres container; `cargo check -p hivemind-task-scheduler --tests --target x86_64-pc-windows-gnu --locked` passed; and `git diff --check` passed. The temporary container `hivemind-codex-settlement-pg-20260814` was removed after verification. The prior general-compute runtime/Worker/production suites remain green. Workspace-wide rustfmt remains intentionally unrun because unrelated dirty files have pre-existing formatting differences.
- Additional verification: the complete locked `general-compute-runtime` suite passed (including the crate-internal direct-process lifecycle tests, production sandbox suite, and supervisor compile-fail boundary); the focused Worker materialized-bundle routing test passed again. The direct-process command types remain `pub(crate)`, and the external compile-fail doctest prevents a production caller from constructing the reference supervisor.
- S1 tensor checkpoint (2026-08-14): commit `012c935` adds materialized tensor byte validation and canonical little-endian conversion for fixed-width/complex values, plus canonical signed-magnitude BigInt scalar validation. Tensor tests (9), serial locked runtime tests, runtime check, and GNU Worker/Task Scheduler/Bin checks passed. Parallel runtime timing fixtures remain a separate known reliability item.
- Coordination status: running. The real Postgres-backed settlement checkpoint passed and its temporary container was cleaned up. The next checkpoint is multi-process/container OCI execution plus operator-owned runner/rootfs deployment validation; no production-readiness claim is made until those external-state gates pass.
- Completed checkpoints:
  - `69098e3` binds local CAS uploads to execution, attempt, idempotency, request digest, artifact manifest coordinates, and SHA-256 bytes.
  - `4b8d955` adds bounded `GeneralComputeChunkUpload` and `GeneralComputeChunkResumeRequest` protobuf envelopes and wire validators.
  - Monty remains removed; do not restore its executable, nested repository metadata, or runtime path.
- Coordination evidence: assignment creates generation 1, reset revokes it, reassignment rotates the attempt and creates generation 2; same-Worker replay remains idempotent, while the Nodepool authority rejects a revoked generation before CAS ingest even when the old Worker has not received a push notification.
- Next checkpoint: prove a lease cannot be reused by another Worker or a rotated attempt, and that reassignment creates a new lease before any chunk upload; do not put 16 MiB chunks into the existing 4 MiB `ExecuteTask` unary RPC.
- Completed this round:
  - `VerifiedWorkerExecution::from_token` re-verifies the Ed25519 Nodepool token, requires the `worker-execution` role, and binds JWT task/worker claims to the assigned Worker.
  - `ingest_general_compute_chunk` rejects a protobuf token that differs from the verified token, validates the bounded wire envelope, converts it to `ChunkUploadEnvelope`, and delegates all request/manifest/bytes/CAS checks to the runtime.
  - RED/GREEN coverage is in `hivemind-rs/crates/worker-executor/tests/chunk_transport.rs` for token mismatch, stale attempt, wrong request digest, manifest mismatch, payload tampering, and identical replay.
- Completed this round:
  - Added the separate `GeneralComputeChunkService` with `UploadChunk` and `ResumeChunks`, mounted alongside `WorkerNodeService` with an independent 16 MiB-plus-overhead message cap.
  - `ExecuteTask` general-compute admission now stores the validated request identity in the Worker assignment report; chunk RPCs cannot use a manually or partially seeded assignment as a substitute.
  - Service-level tests now cover successful admission-bound upload/replay, wrong assignment token, stale attempt/request digest, missing general-compute admission, and unavailable operator CAS.
- Completed this round:
  - Added `PrepareGeneralCompute` to the dedicated authenticated chunk service. It validates the signed worker token, runtime admission, request manifest, and immutable execution identity before recording the assignment.
  - `ExecuteTask` now refuses to replace a prepared general-compute attempt with a different request identity.
  - Nodepool scheduler now uses `PrepareGeneralCompute` followed by authenticated chunk uploads when a manifest contains inline bytes plus chunk coordinates. CAS-only artifacts fail closed because Nodepool has no trusted raw-byte source yet; pure inline artifacts continue through the existing execution path.
  - Scheduler chunk planning binds task, execution, attempt, idempotency, request digest, artifact id, offset, size, SHA-256, and exact inline bytes. Oversized protocol chunks remain rejected by the Worker boundary.
- Completed this round:
  - Worker-execution Ed25519 tokens for `general-compute-v1alpha1` now carry execution, attempt, idempotency, and request-digest claims. Prepare, ExecuteTask, UploadChunk, and ResumeChunks reject a token whose typed identity differs from the admitted manifest.
  - Added regression coverage for token-bound attempt replacement and token round-trip identity binding (including the Nodepool signer); legacy managed-function tokens remain task/worker-bound.
  - CAS-only prepare failures now become terminal task failures without incrementing Worker failure reputation, instead of resetting indefinitely for redispatch. Transport/CAS availability failures still redispatch without Worker penalty.
  - Nodepool now calls authenticated `ResumeChunks` per artifact after Prepare and uploads only descriptors the Worker reports missing; every requested descriptor must match the persisted inline manifest before bytes are sent.
- Verification this round:
  - `cargo test -p hivemind-auth --lib --target x86_64-pc-windows-gnu --locked` passed (6 tests, including execution-token attempt identity binding).
  - `cargo test -p hivemind-worker-executor --lib --target x86_64-pc-windows-gnu --locked` passed (97 tests).
  - `cargo test -p hivemind-worker-executor --test chunk_transport --target x86_64-pc-windows-gnu --locked` passed (8 adapter tests).
  - `cargo test -p hivemind-task-scheduler --lib --target x86_64-pc-windows-gnu --locked` passed (100 passed, 1 intentional ignored).
  - After Resume integration, the focused Prepare/Resume/upload scheduler test and the full scheduler lib suite pass (101 passed, 1 intentional ignored).
  - Final `cargo check -p hivemind-auth -p hivemind-worker-executor -p hivemind-task-scheduler -p hivemind-bin --target x86_64-pc-windows-gnu --locked` passed.
  - `cargo check -p hivemind-worker-executor -p hivemind-task-scheduler -p hivemind-bin --target x86_64-pc-windows-gnu --locked` passed.
  - `cargo test -p hivemind-proto --target x86_64-pc-windows-gnu --locked` passed (10 tests).
  - `cargo test -p general-compute-runtime --locked` passed (all runtime, sandbox, supervisor, and compile-fail doc tests).
  - `cargo check -p hivemind-worker-executor --tests --locked` passed.
  - `cargo check -p hivemind-worker-executor --locked` passed.
  - `cargo test -p hivemind-worker-executor --test chunk_transport --target x86_64-pc-windows-gnu --locked` passed (8 adapter tests).
  - `cargo test -p hivemind-worker-executor --lib grpc_server::tests::chunk_service --target x86_64-pc-windows-gnu --locked` passed (5 service tests).
  - `cargo test -p general-compute-runtime --locked` passed (all runtime transport, CAS, sandbox, supervisor, and doc tests).
  - `cargo test -p hivemind-proto --locked` passed (8 tests).
- Verification this round:
  - `cargo test -p hivemind-worker-executor --lib grpc_server::tests::prepare_general_compute_records_admission_before_chunk_transfer --target x86_64-pc-windows-gnu --locked` passed.
  - `cargo test -p hivemind-worker-executor --lib grpc_server::tests::execute_task_cannot_replace_a_prepared_general_compute_request --target x86_64-pc-windows-gnu --locked` passed.
  - `cargo test -p hivemind-task-scheduler --lib inline_general_compute_chunk_plan --target x86_64-pc-windows-gnu --locked` passed (2 tests).
  - `cargo test -p hivemind-task-scheduler --lib test_execute_on_worker_ignores_stale_general_compute_response_without_settlement --target x86_64-pc-windows-gnu --locked` passed.
  - `cargo check -p hivemind-bin --locked` passed after the new generated RPC surface was mounted.
- The MSVC static-link mismatch is avoided without symbol shims: MSVC consumers dynamically load the validated `libtailscale.dll`, while GNU consumers retain the existing static archive. A live VPN integration run still requires the configured overlay and credentials.
- Blockers: durable Worker resume state is implemented per Worker but cross-worker/container E2E and trusted usage/billing settlement remain. A metadata-only/CAS-only manifest is still terminally rejected when no exact Nodepool-owned source row exists; it does not enter a redispatch loop. Production runner isolation primitives remain operator-owned and require deployment validation.
- Scope guard: preserve unrelated dirty frontend/API/Cargo/proto changes; do not reset, checkout, or bulk-stage them.

## Active checkpoint (2026-08-14)

- Decision: persist validated inline artifact bytes in a dedicated Nodepool table keyed by task and stable artifact id, rather than treating the mutable attempt manifest as the raw-byte source.
- The scheduler will verify the persisted artifact metadata and digest against the current manifest before uploading chunks. Retry attempt rotation changes only the request attempt identity; artifact bytes remain reusable by task/artifact id.
- Scheduler dispatch now treats the mutable attempt manifest as metadata only: even originally inline artifacts must be loaded from the task-bound Nodepool source row before any Worker upload.
- This checkpoint does not add URL fetching or accept caller-provided filesystem paths. A metadata-only artifact with no Nodepool-owned bytes remains an operator-side terminal failure.
- Status: completed for the full-byte persistence/read-path and immutable lifecycle slice; the remaining work is durable chunk resume state and lifecycle settlement.
- Implementation evidence: `general_compute_artifact_sources`, `general_compute_artifacts`, and `general_compute_artifact_manifest_chunks` are created by the database migration; `TaskRepository::create` stores validated inline bytes and immutable task/artifact/SHA-256/size/chunk coordinates; scheduler dispatch loads only exact task-bound rows and rechecks identity/expiry before chunk planning. Existing inline and metadata-only/CAS-only tests remain green.
- Verification: `cargo test -p hivemind-database --lib task_migrations_create_general_compute_artifact_source_table --target x86_64-pc-windows-gnu --locked`; `cargo test -p hivemind-task-scheduler --lib --target x86_64-pc-windows-gnu --locked` (104 passed, 1 intentional ignored); scoped `cargo check` for task-scheduler/node-manager/bin passed; `git diff --check` passed.

## Handoff checkpoint

- Keep Monty removed. The managed-function runtime and general-compute runtime are separate paths; do not reintroduce a Monty executable or nested source metadata.
- Next implementation owner: make resume state durable across retry and Worker changes using the task-bound source rows and lifecycle state. Preserve the token identity fields and fail-closed metadata-only/CAS-only disposition.
- Do not claim production readiness until OCI routing, durable availability state, cross-worker resume, and trusted usage/billing settlement are implemented and tested.

## Authenticated source-upload checkpoint (2026-08-14)

- Added a separate `GeneralComputeArtifactService.UploadChunk` protobuf/API for user-token uploads into Nodepool-owned persistence. It is deliberately distinct from the Worker execution-token `GeneralComputeChunkService` and does not enlarge `ExecuteTask`.
- Nodepool validates the common wire envelope, authenticates the Nodepool JWT, requires task ownership (or admin), accepts only non-terminal `general-compute-v1alpha1` tasks, and delegates exact manifest coordinate/SHA-256/size/idempotency checks to `TaskRepository`.
- Master exposes only an HTTP-to-gRPC proxy at `POST /api/tasks/:task_id/general-compute/artifacts/chunk`; it does not hold signing secrets or write local files. JSON uses an explicit byte array and the route has a bounded body limit above one protocol chunk.
- RED/GREEN evidence: Nodepool owner/replay/conflict/terminal tests, proto wire validation tests, HTTP body validation tests, and the in-process HTTP -> Nodepool -> Postgres integration test all pass under `x86_64-pc-windows-gnu`.
- Remaining gaps are unchanged: cross-Worker transfer coordination, production OCI routing, and trusted usage/billing settlement. CAS-only manifests still fail closed until an upload client supplies the exact persisted chunk coordinates.

## Artifact lifecycle checkpoint (2026-08-14)

- Added Nodepool-owned immutable `general_compute_artifacts` identity rows and `general_compute_artifact_manifest_chunks` coordinate rows. They are keyed by `(task_id, artifact_id)` and survive attempt rotation; mutable attempt JSON is metadata only.
- Lifecycle state is persisted as `pending` / `available` / `expired` with `complete`, expected chunk count, digest, size, and optional task-deadline expiry. Inline creation and authenticated chunk upload update the state; reads, scheduler source loading, and uploads materialize expiry and fail closed.
- Scheduler now checks current attempt coordinates against the immutable identity before loading bytes. Existing direct-manifest migration paths can backfill the identity once without replacing an existing row.
- RED/GREEN evidence: lifecycle/expiry, coordinate-drift, chunk idempotency, Nodepool owner/replay, database migration, and full scheduler lib tests pass under `x86_64-pc-windows-gnu` (111 passed, 1 intentional ignored).
- Remaining gaps: cross-Worker transfer coordination, production OCI routing, and trusted usage/billing settlement. No URL/filesystem source inference was added, and Monty remains removed.

## Worker durable transfer checkpoint (2026-08-14)

- `CasChunkStore` now creates an operator-owned `.transfers` journal below the configured CAS root. The journal persists an immutable `(execution_id, artifact_id, size, sha256, chunk coordinates)` manifest and atomic per-chunk completion markers.
- Authenticated Worker uploads write the verified CAS object and completion marker; `ResumeChunks` reopens the journal, rehashes CAS objects, reconciles crash windows, and returns only chunks still absent. A retry may rotate `attempt_id` while reusing the stable execution/artifact identity.
- Journal paths are keyed by a SHA-256 of the typed identity, marker contents are checked, symlinked/non-directory state roots fail closed, and manifest redefinition or corrupt markers are rejected. No URL, arbitrary filesystem path, or Worker-provided completion claim is trusted.
- This is durable per-Worker operator state, not a cross-Worker shared cache. When a task moves to a different Worker, Nodepool must use its authenticated source rows and upload exact missing chunks to that Worker. Cross-Worker leases/generation evidence, production OCI routing, and trusted usage/billing settlement remain outstanding.
- RED/GREEN evidence: runtime artifact transfer restart, attempt rotation, adapter upload recovery, manifest redefinition, corrupt-marker fail-closed tests; full `general-compute-runtime` locked suite; Worker GNU chunk transport suite; Worker test compile/check; `git diff --check` passed.

## Authoritative cross-Worker revoke checkpoint (2026-08-14)

- Added `ValidateGeneralComputeTransferLease` to the NodeManager gRPC surface. Nodepool validates the Ed25519 execution token, checks the active non-expired database lease, and compares the full task/execution/attempt/worker/generation identity.
- The authority comparison also includes the idempotency key and request digest; the Worker never delegates a partially bound identity to Nodepool.
- Production Worker chunk services call that authority before Prepare, ExecuteTask admission, UploadChunk, and ResumeChunks. Authority unavailability fails closed as `Unavailable`; a revoked or reassigned lease is `PermissionDenied`.
- RED/GREEN evidence: `cargo test -p hivemind-worker-executor --lib reassignment_revokes_old_worker_before_chunk_replay_and_allows_new_worker --target x86_64-pc-windows-gnu --locked` passes; it proves Worker A's stale token fails after the shared authority rotates the lease to Worker B, while Worker B generation 2 succeeds.
- This closes stale replay at the control-plane boundary but is not production-ready: expiry cleanup, multi-process/container E2E, OCI routing, and trusted usage/billing settlement remain outstanding.

## Authority integration and expiry checkpoint (2026-08-14)

- Fixed the test-only `TransferLeaseAuthority` mocks after the trait gained the idempotency-key and request-digest parameters. The full Worker executor lib suite now passes (99 tests).
- Added a real Nodepool gRPC/client integration test over an isolated Postgres schema: an assigned general-compute lease validates through the generated RPC; after expiry it is materialized as `expired` and the same token is rejected.
- Nodepool authority and scheduler lease reads now materialize only the queried task's expired active lease, preventing stale `active` rows from remaining in persistent state while avoiding unrelated-task updates.
- Verification: Nodepool lib 77 passed; Task Scheduler lib 114 passed with one intentional ignored test; Worker chunk transport 9 passed; scoped `cargo check` for Nodepool/Task Scheduler/Worker/bin passed; `git diff --check` passed. Workspace-wide rustfmt was not run because unrelated pre-existing dirty files have formatting differences.
- Remaining blockers are unchanged: multi-process/container E2E, production OCI routing, and trusted usage/billing settlement. Monty remains removed and `ExecuteTask` remains below the chunk transport size boundary.
- Worker state construction is now fail-closed at the API boundary: the no-authority constructor is test-only, while production callers must use `new_with_transfer_lease_authority`. Worker executor lib (99) and Worker/bin scoped checks remain green after this hardening.
- The public API boundary is covered by a compile-fail doctest on `WorkerGrpcState`; the Worker doc-test suite executes and passes that case instead of relying on a cfg-hidden example.

## Current coordination round (2026-08-14)

- Status: `running`.
- Completed this round: process-tree cancellation/descendant timing fixtures were stabilized in `a0ac3ff`; sparse CSR/CSC/COO metadata validation landed in `b8c264e`; materialized sparse bytes now enforce checksums, CSR/CSC indptr bounds and monotonicity, index bounds, sorted/duplicate policy, COO pair ordering, byte order, and signed-index rules in `85f2bcd`; CPython timeout startup contention is stabilized in `a22c70c`; deterministic f64 broadcast/add/multiply and axis reduction kernels landed in `35d4264`; bounded 2-D f64 matmul with zero-inner-dimension semantics landed in `14c6c4d`; typed dense f32/f64/complex64/complex128 kernels landed in `aceff3a`; bounded typed vector dot landed in `e1daccb`; bounded typed batched matmul landed in `88ae864`; bounded f64 linear solve with partial pivoting landed in `272b6fe`; bounded complex128 FFT reference landed in `819868f`; deterministic splitmix64 RNG with seed/stream/subsequence binding landed in `a788f70`; bounded fixed-step scalar RK4 ODE reference landed in `d032c52`; bounded deterministic unit-circle Monte Carlo reference landed in `ac0580b`; bounded CSR/CSC/COO f64 sparse matvec reference landed in `2235caf`; sparse segment scans were tightened to linear bounded iteration in `048d2aa`; f64 solve residual/error validation landed in `048509f`; FFT round-trip accuracy and golden-vector gates landed in `7c44660`; bounded f64 LU factorization landed in `2123da0`; sparse f64 residual/tolerance validation landed in `75c930c`.
- Verification: numeric integration tests (18), RNG integration tests (2), ODE integration tests (2), Monte Carlo integration tests (3), FFT accuracy/golden integration tests (3), sparse ABI integration tests (8), sparse algebra and residual integration tests (8), residual-gate integration tests (2), LU integration tests (3), the locked `general-compute-runtime` suite (serial and four-thread parallel), runtime check, GNU Worker/Task Scheduler/Bin checks, scoped rustfmt, and scoped `git diff --check` passed. Strict clippy remains blocked by pre-existing crate-wide debt; no LU or sparse-residual-specific warning remains.
- Recovery note: one concurrent full-suite invocation transiently hit the known supervisor output-drain fixture; the focused test and a standalone four-thread runtime suite passed on rerun, with no supervisor production path change. The sparse linear-scan correction was rechecked with the focused sparse suite and runtime check.
- Next action: evaluate QR/SVD or pinned optimized backends; sparse solve/reduce and broader golden vectors remain open, as do multi-process/container OCI E2E, operator runner deployment, GPU capability, and trusted settlement.
- Checkpoint: no production-readiness claim; multi-process/container OCI E2E and operator-owned runner deployment validation remain open.
- LU and sparse residual checkpoints are complete at `2123da0` and `75c930c`; continue with QR/SVD or a pinned optimized backend. Overall coordination remains `running` and no production-readiness claim is made.

## Typed dense numeric checkpoint (2026-08-14)

- Runtime commit `aceff3a` generalizes the bounded CPU dense reference kernels
  to typed `DenseTensor<T>` values. `F32Tensor`, `F64Tensor`, `Complex64Tensor`,
  and `Complex128Tensor` share the same checked elementwise, broadcast, reduce,
  and 2-D matmul paths; complex operations are component-wise and do not alter
  the transport or Worker RPC boundary.
- RED→GREEN evidence: 8 numeric integration tests pass, including f32
  broadcast/multiply and complex64/complex128 arithmetic. The complete locked
  runtime suite passes serially and with four test threads, runtime check passes,
  and GNU Worker/Task Scheduler/Bin checks pass.
- This is a local reference-kernel slice only. It does not claim BLAS/LAPACK,
  FFT, ODE, RNG, Monte Carlo, sparse algebra, GPU, cross-worker/container E2E,
  or production runner deployment readiness. Keep those as separate gates.

## Bounded typed dot checkpoint (2026-08-14)

- Runtime commit `e1daccb` adds a checked one-dimensional `DenseTensor<T>::dot`
  primitive. It is typed and bounded, rejects rank/length mismatches, and does
  not change the transport, Worker RPC, CAS, or admission boundary.
- RED→GREEN evidence: the initial dot test failed on the missing method; the
  focused dot suite then passed 3 tests, and the full numeric suite passes 11.
  Runtime serial/parallel suites and GNU cross-crate checks remain green.
- This does not imply a production BLAS/LAPACK backend, cross-worker/container
  E2E, or release readiness; those gates remain explicit.

## Bounded typed batched matmul checkpoint (2026-08-14)

- Runtime commit `88ae864` adds three-dimensional typed batched matmul with
  single-batch broadcasting and explicit rank/batch/inner-dimension validation.
  It remains entirely inside the reference numeric module and does not alter
  the transport or Worker admission surface.
- RED→GREEN evidence: the focused batched suite passes 3 tests for broadcast,
  zero-inner/complex arithmetic, and mismatch rejection; numeric now passes 14
  tests, with runtime serial/parallel and GNU cross-crate checks green.
- This is not a production BLAS/LAPACK backend or an OCI/GPU/settlement gate.

## Bounded f64 solve checkpoint (2026-08-14)

- Runtime commit `272b6fe` adds a square f64 vector-RHS solve with deterministic
  partial pivoting and fail-closed singular/non-finite handling. It remains in
  the local numeric module and does not alter transport or Worker admission.
- RED→GREEN evidence: the solve-focused suite passes 2 tests, numeric passes
  16 tests, and runtime serial/parallel plus GNU cross-crate checks pass.
- This is not a production LAPACK claim; residual/error gates, complex or
  multi-RHS solve, OCI/GPU E2E, and settlement remain separate work.

## Bounded complex FFT reference checkpoint (2026-08-14)

- Runtime commit `819868f` adds a bounded complex128 one-dimensional forward /
  inverse reference DFT with unnormalized forward and `1/n` inverse semantics.
  It is local numeric code only and leaves transport, Worker admission, and CAS
  unchanged.
- RED→GREEN evidence: FFT-focused tests pass 2 cases; numeric passes 18, and
  runtime serial/parallel plus GNU cross-crate checks remain green.
- This is not a production FFT backend; optimized implementation, tolerances,
  real transforms, OCI/GPU E2E, and settlement are still open gates.

## Deterministic RNG checkpoint (2026-08-14)

- Runtime commit `a788f70` adds a bounded `splitmix64-v1` RNG with explicit
  seed/stream/subsequence identity and a one-million-sample cap. It is local
  reference code and leaves transport, Worker admission, and CAS unchanged.
- RED→GREEN evidence: RNG tests pass 2 pinned replay/stream-separation and
  bounded unit-interval/cap cases; runtime serial/parallel and GNU cross-crate
  checks remain green.
- This does not claim cryptographic quality, statistical coverage, Monte Carlo
  confidence, or production determinism-policy wiring.

## Bounded RK4 ODE reference checkpoint (2026-08-14)

- Runtime commit `d032c52` adds a bounded fixed-step scalar RK4 reference
  integrator with explicit target direction, step-count cap, and finite-state /
  finite-derivative validation. It is local numeric code only and does not
  alter the transport, Worker admission, CAS, or chunk-resume boundary.
- RED→GREEN evidence: the focused ODE suite passes 2 cases for an exponential
  known solution and invalid configuration, direction, step-budget, and
  non-finite failure semantics. The locked runtime suite (serial and four
  threads) plus GNU Worker/Task Scheduler/Bin checks remain green.
- This is not a production ODE backend. Adaptive/vector/stiff solvers,
  numerical error gates, OCI/container E2E, operator runner deployment, and
  trusted usage/billing settlement remain open.

## Bounded deterministic Monte Carlo checkpoint (2026-08-14)

- Runtime commit `ac0580b` adds a fixed unit-circle π reference estimator on
  the pinned `splitmix64-v1` stream identity. It reports hit count, estimate,
  Bernoulli variance, standard error, and a pinned 90/95/99% normal confidence
  interval under a 500,000-trial cap; it does not alter transport, Worker
  admission, CAS, or chunk-resume behavior.
- RED→GREEN evidence: the focused suite passes 3 tests for the pinned 10,000
  trial replay (`7,813` hits), empty/over-budget fail-closed behavior, and
  wider confidence levels. The locked runtime suite (serial and four threads)
  plus GNU Worker/Task Scheduler/Bin checks remain green.
- This is a local deterministic reference fixture, not a general sampler,
  cryptographic RNG, production statistical guarantee, GPU path, OCI/container
  E2E, operator deployment proof, or trusted settlement implementation.

## Bounded sparse f64 matvec checkpoint (2026-08-14)

- Runtime commit `2235caf` adds a bounded sparse `f64` matrix-vector reference
  kernel over already validated, materialized CSR/CSC/COO bytes. It keeps the
  transport, Worker admission, CAS, and chunk-resume boundaries unchanged;
  indices support the manifest's signed/unsigned dtype, byte order, and index
  base, while allowed duplicates accumulate in manifest order.
- RED→GREEN evidence: the focused sparse-algebra suite passes 6 cases for CSR,
  CSC/COO equivalence, duplicate accumulation, one-based big-endian decoding,
  non-finite/vector failures, and unsupported/capped inputs. The locked runtime
  suite (serial and four threads) plus GNU Worker/Task Scheduler/Bin checks are
  green.
- This is not a production sparse backend. Residual/error golden vectors,
  optimized/GPU implementations, OCI/container E2E, operator deployment, and
  trusted usage/billing settlement remain open.

## Bounded f64 solve residual checkpoint (2026-08-14)

- Runtime commit `048509f` adds the sequential infinity-norm residual check and
  tolerance-gated f64 solve. It stays inside the local numeric module and does
  not alter transport, Worker admission, CAS, or chunk-resume behavior.
- RED→GREEN evidence: the focused residual suite passes 2 cases for the pinned
  3×3 residual fixture, accepted/rejected tolerances, and invalid tolerance
  inputs; the locked runtime suite (serial and four threads) plus GNU Worker/
  Task Scheduler/Bin checks remain green.
- This is a reference numerical error gate, not a production LAPACK or
  backward-error guarantee. FFT/sparse golden vectors, GPU/OCI E2E, operator
  deployment, and trusted usage/billing settlement remain open.

## Bounded f64 SVD factorization checkpoint (2026-08-14)

- Runtime commit `31c3a21` adds a bounded deterministic thin one-sided Jacobi
  SVD for tall, square, and wide `f64` matrices. It returns `U`, descending
  singular values, and `Vᵀ`, supports reconstruction and dual-factor
  orthogonality/error norms, and keeps deterministic sign normalization.
- RED→GREEN evidence: the focused SVD suite passes 4 tests; the locked
  `general-compute-runtime` suite passes with one and four test threads,
  `cargo check -p general-compute-runtime --locked` passes, and the GNU
  Worker/Task Scheduler/Bin cross-crate check passes. No transport, Worker
  admission, CAS, chunk-resume, or settlement ABI changed.
- This is a bounded CPU reference implementation, not production
  BLAS/LAPACK, a pinned optimized backend, GPU execution, OCI/container E2E,
  operator deployment, or trusted settlement. The next numerical gate is real
  FFT or broader FFT golden vectors; transport status remains `running` and
  carries no production-readiness claim.

## Bounded real f64 FFT checkpoint (2026-08-14)

- Runtime commit `3e5d53e` adds a bounded full-spectrum real forward DFT and
  `1/n` inverse transform with `rfft`/`irfft` aliases. The inverse validates
  finite values, DC/Nyquist reality, and conjugate symmetry before producing a
  real signal; round-trip error and finite/non-negative tolerance gates are
  exposed without changing the existing complex FFT ABI.
- RED?REEN evidence: the focused real FFT suite passes 3 tests; the locked
  `general-compute-runtime` suite passes with one and four test threads,
  `cargo check -p general-compute-runtime --locked` passes, and the GNU
  Worker/Task Scheduler/Bin cross-crate check passes. No transport, Worker
  admission, CAS, chunk-resume, or settlement ABI changed.
- This is an O(n²) bounded CPU reference transform, not an optimized FFT
  backend or production scientific image. Broader golden vectors, statistics/
  RNG coverage, backend pinning, GPU/OCI E2E, operator deployment, and trusted
  usage/billing settlement remain open; transport status remains `running` and
  carries no production-readiness claim.

## Bounded f64 QR factorization checkpoint (2026-08-14)

- Runtime commit `dc8e66b` adds a bounded deterministic thin Householder QR
  reference factorization for tall and square `f64` matrices. It returns thin
  orthogonal/upper factors, supports reconstruction and orthogonality/error
  norms, and rejects wide, rank-deficient, non-finite, over-cap (1024), shape
  mismatch, and invalid-tolerance inputs.
- RED→GREEN evidence: the focused QR suite passes 4 tests; the locked
  `general-compute-runtime` suite passes with one and four test threads,
  `cargo check -p general-compute-runtime --locked` passes, and the GNU
  Worker/Task Scheduler/Bin cross-crate check passes. No transport, Worker
  admission, CAS, chunk-resume, or settlement ABI changed.
- This is a bounded CPU reference implementation, not production
  BLAS/LAPACK, SVD, an optimized backend, GPU execution, OCI/container E2E,
  operator deployment, or trusted settlement. The next numerical gate is SVD
  or a pinned optimized backend; the transport status remains `running` and
  carries no production-readiness claim.

## Bounded f64 LU factorization checkpoint (2026-08-14)

- Runtime commit `2123da0` adds a bounded deterministic partial-pivot f64 LU
  reference factorization inside the local numeric module. It exposes `L`,
  `U`, and the pivot permutation for `P*A = L*U`, supports vector RHS solves
  and permuted reconstruction, and rejects non-square, singular, non-finite,
  over-cap (1024), and mismatched-RHS inputs.
- RED→GREEN evidence: the focused LU suite passes 3 tests; the locked runtime
  suite (serial and four-thread), runtime check, GNU Worker/Task Scheduler/Bin
  checks, scoped rustfmt, and `git diff --check` are green. This slice does
  not change transport, Worker admission, CAS, or chunk resume.
- This remains a bounded CPU reference implementation, not production
  BLAS/LAPACK, OCI/container E2E, operator deployment, GPU, or settlement
  proof. QR/SVD and sparse tolerance/error gates remain next.

## Bounded sparse f64 residual checkpoint (2026-08-14)

- Runtime commit `75c930c` adds a sequential infinity-norm residual evaluator
  and tolerance gate to `SparseF64Matrix::matvec`. RHS length/finiteness and
  finite non-negative tolerance are validated before accepting the result;
  the sparse ABI, transport, Worker admission, CAS, and chunk resume remain
  unchanged.
- RED→GREEN evidence: the sparse numeric suite passes 8 tests for CSR/CSC/COO
  reference behavior plus residual reporting, tolerance acceptance/rejection,
  invalid tolerance, and RHS-shape failure semantics. The locked runtime suite,
  runtime check, GNU Worker/Task Scheduler/Bin checks, scoped rustfmt, and
  `git diff --check` remain green.
- This remains a bounded reference error gate, not sparse solve/reduce,
  backward-error proof, optimized/GPU backend, OCI E2E, operator deployment,
  or trusted settlement.

## Bounded FFT accuracy checkpoint (2026-08-14)

- Runtime commit `7c44660` adds a component-wise round-trip error evaluator and
  tolerance gate for the complex128 reference DFT, plus a four-point impulse
  golden vector. It remains local numeric code and leaves transport, Worker
  admission, CAS, and chunk-resume behavior unchanged.
- RED→GREEN evidence: the focused FFT accuracy suite passes 3 cases for the
  round-trip bound, invalid/too-tight tolerance failures, and the impulse
  spectrum vector. The locked runtime suite (serial and four threads) plus GNU
  Worker/Task Scheduler/Bin checks remain green.
- This is an O(n²) reference quality gate, not an optimized/real-transform
  backend. Sparse tolerance vectors, backend pinning, GPU/OCI E2E, operator
  deployment, and trusted usage/billing settlement remain open.

## Operator-owned OCI seccomp profile checkpoint (2026-08-14)

- The production registry now requires an operator-provided absolute
  `seccomp_profile_path`; materialization rejects missing, symlinked, malformed,
  non-canonical, digest-drifted, or schema-invalid profiles. The profile is
  copied into `linux.seccomp` with a non-empty `SCMP_ACT_ALLOW` syscall
  allowlist, while the policy keeps the `SCMP_ACT_ERRNO` default action and
  pinned SHA-256 annotation.
- RED→GREEN evidence: production 7/7, sandbox 22/22, locked runtime suite,
  Worker GNU test check, Task Scheduler/Bin checks, OCI preflight contract, and
  scoped `git diff --check` pass. The preflight now checks profile path,
  regular/non-symlink status, digest, default action, and syscall groups.
- Local commit: `43dd537 feat(runtime): bind operator seccomp profiles`.
- This remains a fail-closed contract and deployment preflight, not proof of
  host-level rootless seccomp enforcement, real OCI/container E2E,
  Worker→Nodepool→Postgres completion, or trusted usage/billing settlement;
  transport status remains `running`.

## Isolated OCI Compose project checkpoint (2026-08-14)

- The release Compose surface no longer fixes container names, network names,
  service IPv4 addresses, or a subnet. Internal torrent advertisement uses the
  `nodepool` service name, so an E2E project can receive its own Compose network.
- The preflight temporarily assigns project-prefixed names to every named
  volume, checks the resolved Compose JSON, and restores all caller environment
  variables in `finally` on both check-only and failure paths.
- RED→GREEN evidence: OCI harness contract, Compose release contract, resolved
  Compose config, and scoped `git diff --check` pass. Local commit:
  `c24d036 fix(deploy): isolate OCI compose projects`.
- This remains deployment resource isolation only; real rootless OCI execution,
  Postgres-backed completion, hostile workload checks, and trusted settlement
  are still open.

## Reviewed multi-process OCI fixture protocol checkpoint (2026-08-14)

- `scripts/general-compute-oci-e2e.ps1 -Run` now owns the execution lifecycle:
  it assigns project-prefixed named volumes and isolated host ports, invokes a
  reviewed PowerShell fixture in explicit `provision` then `execute` phases,
  starts `postgres`, `redis`, `nodepool`, `master`, and `worker` with Compose,
  and always performs `down --volumes --remove-orphans` cleanup.
- The fixture evidence contract is versioned as
  `general-compute-oci-e2e-v1`. The harness rejects success unless the evidence
  is bound to the random project/task, identifies all four services, marks
  worker registration, task completion, Postgres settlement, timeout/cancel,
  network deny, and filesystem deny true, and carries a validated
  `general-compute-result-v1` `ProductionResultEnvelope` result.
- Compose now propagates optional `WORKER_NODEPOOL_USERNAME`/
  `WORKER_NODEPOOL_PASSWORD`; default-user seeding is explicit opt-in through
  `HIVEMIND_SEED_DEFAULT_USER` and is only set by the isolated harness.
- Evidence is preserved under `test_logs/` (or a caller-provided absolute path)
  instead of being deleted during cleanup, so a failed or successful run leaves
  an auditable artifact.
- The repository now ships `scripts/general-compute-oci-task-fixture.ps1`, which
  performs the reviewed volume provisioning, Master authentication, Worker
  registration wait, task polling, Postgres result/settlement queries, and
  hostile-case assertions. Operators still supply the registry/rootfs/runner/
  profile and the explicit `HIVEMIND_GENERAL_COMPUTE_OCI_E2E_CASES` plan with
  canonical request digests; unsupported host primitives or missing plan data
  remain fail-closed release gates.
