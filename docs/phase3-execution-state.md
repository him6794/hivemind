# Phase 3 execution state

## Goal and success criteria

Complete Claude's P3–P7 handoff in the attachment pasted-text-1.txt and
docs/NEXT_VERSION_PHASE3_2026-11-11.md. Preserve Nodepool as sole trusted
authority. Acceptance: resumable/budget-safe network pipelines; measured model
splits with bitwise reference equality; deterministic training; authenticated
idle-only Worker admission/UI; documentation; actual same-host three-Worker
E2E; local tested feature commits; whole-diff review and memory updates.
No push, PR, deployment or unrelated mutations.

## Status

complete

## Current step and owners (2026-11-12)

- root: implementation, review, tests, final evidence/plan/memory and local
  feature commits complete; owned resources cleaned and worktree checked.
- p6_idle: completed scheduler closed-port timing investigation;7/7 unchanged
  reruns pass, original failure is outer2s timeout before disposition assertion;
  exact historical-load cause unproved, no unrelated source changes.
- p4_model: completed P3 explicit-default replica policy, network regression,
  panic-free runner and all minimal CLI gates; idle.
- p5_training: completed independent read-only P6 follow-up review; no remaining
  blocking/security finding; idle.

## Local commits

- Baseline P1/P2: 3a3addc8 (concat), 8d89dc8c (pipeline check).
- a1a03db3: P5 federated training; Python4 and real CLI integration pass.
- e183fa65: initial P6 idle controls; config45/worker200/UI69+build pass.
- 5babdc1e: exact binary64 JSON roundtrips; CPU/GPU pins updated, v0 unchanged.
- 97b6e82a: P3 resumable pipeline runs; CLI105 plus integration pass,
  network17, minimal CLI all-target clippy/fmt pass.
- 84ae15fa: P4 model init/split; model10 tests, full-binary smoke, clippy pass.
- daa81e02: P6 authenticated/atomic admission fix; five targeted tests,
  worker-lib205 and30 binary/integration/doc tests, clippy/fmt pass.
- 7c88bf8c: model panic-free refactor; model10 tests pass.
- 28dcb5e5: P3 pinned default3 and panic-free runner;109 CLI tests (20 network,
  10 model), minimal CLI all-target clippy and fmt pass.
- d65b8f38: baseline models lint cleanup; nine tests, full workspace clippy and
  fmt pass.
- P7: documentation, isolated E2E harness and sanitized final-binary evidence
  are committed together with this state file. No push or deployment.

## Verification and decisions

- P3 retains all attempt charges; reserves unsettled tasks at full caps even
  when API billed_amount=0; reconciles uncertain HTTP500/408/400; preserves
  SENDING before POST; pins exact source/params/retained artifact content;
  lost artifact upload replies resume the same task. These regressions pass.
- P4 packed/sharded equality exposed serde_json default-parser double-rounding.
  Enabled float_roundtrip in both workspaces; CPU digest
  d4c641691b75ad5a673c1424d324be2a3412170e1fccd86ae7ecddb09d30e214;
  GPU digest 4ad70f9be7ae18660566c727759a6dd780a8d70cecc6b2a075584c2c8567293f.
  Full executor managed-function/general-compute suites and clippy/fmt pass.
- Final review: omitted pipeline replicas must pin effective3 in request and
  state, not inherit a configurable network default. Omitted/explicit3 resume
  is equivalent; old null-policy draft state fails closed. Batch is unchanged.
- Final review: Nodepool intentionally admits BUSY for anti-steering. Worker
  must locally reject NEW execution/prepare, preserve authenticated replay and
  in-flight work, and never fabricate usage. No Nodepool selection/billing
  change. Refusal can cause no-quorum failure (hcode auto-retry0) and other
  replicas' valid work may charge; guide/site/manual/plan now say so.
- Root replaced3 invariant unwraps in cli_model.rs; all10 model tests pass.
- P7 harness9/Python training4/release-doc contracts pass; Master UI98 pass.
  Website104 and28-page build pass including final idle caveat edit (session97552).
- External Codex probe already done: no CLI executable or independent connector.
  Independent internal agents plus root audit are used; do not claim external
  Codex review. Workflow auto-trigger config absent; no extra skill installs.
- Final P7 read-only review corrected tutorial numeric wording: active hcode
  accepts JSON integers outside i64 as finite f64 with possible rounding; it
  does not reject every integer outside i64. Confirmed against source and CLI.
  No further documentation blocking findings.
- Scheduler broad run:215 pass/1 fail in467.56s. Original panic is outer2s
  timeout at dispatcher.rs:6677 (Elapsed), before disposition assertion.
  Exact test reruns pass7/7 without edits; test source predates Phase3.
  Closed-loopback OS refusal takes about2s on this host; test relies on a1s
  connect timer inside a2s outer timer. Scheduling delay is a plausible timing
  failure, not proved historical load; no unrelated timer/production change.

## Live E2E evidence already obtained

Final run test_logs/phase3-e2e-20261112-reviewed passed,
00:59:28–00:59:57 UTC on2026-11-12, final source commit d65b8f38.
Binary SHA256 16f80c6475829fc0a41c05fde97665a0f4d904a21e1c2de751da04bbccd4d685.
Ordinary pipeline: output13, 3tasks,228units,250CPT,3.500s.
Model: 2stages/4tasks,7638units,8400CPT,6.641s; exact remote/local equality
and independent scalar reference within1e-12. All21 replica reports successful,
positive and timestamped (7 per Worker);4 complete artifacts/3248bytes.
All five processes, two exact containers and temp workspace were removed;
resources.json has no cleanup errors; independent process/container/path queries
confirm removal. Sanitized evidence is docs/evidence/phase3-same-host-2026-11-12.json.
Same-host CPU hcode only; not cross-host, Linux headless, GPU, general-compute
isolation or performance evidence. Earlier pre-review E2E also passed but is
superseded by this final run.

## Completed jobs and owned-resource cleanup

- Full workspace real-DB tests: terminal session80979 finished nonzero. Command:
  cargo test --workspace --no-fail-fast -j 1 -- --test-threads=1.
  HIVEMIND_REQUIRE_TEST_DATABASE=1; isolated DB on127.0.0.1:19620.
  Total1156pass/1fail/2ignored. Complete captured output saved at
  test_logs/phase3-workspace-20261112.log.
  Broad executable predates final P3/P6 follow-ups; affected packages were
  separately retested on the final tree. Scheduler closed-port transport test
  hit its outer2s timeout; exact reruns pass7/7. Remaining VPN/Worker/doc targets
  passed. Live email and live Headscale tests were explicitly ignored;
  DB tests are mandatory, not skipped.
- Test PostgreSQL container hm-phase3-test-pg-20261112a, exact ID
  3e0c53444a00743069841e82b75f217861180781b2150f70ef90cc70aff03ed2.
  Removed with its disposable volumes after all DB tests; absence confirmed.
  All E2E-owned containers/processes/temp paths also removed, reports retained.
- Docker verified local desktop-linux named-pipe endpoint.
- No active task-owned terminal jobs or delegated work remain. Unrelated
  editor/rust-analyzer processes and user services were not stopped.
- Temporary index patch .git/codex-p3-index.patch was removed.
- Full workspace clippy found pre-existing items_after_test_module in models
  (API_TOKEN_PREFIX after test mod, also present at baseline8d89dc8c). Root moved
  only that constant before tests; full clippy/fmt/models tests/default build
  clippy passed (session69780). A trailing blank line then failed fmt and was
  corrected with rustfmt; final fmt and models9 tests pass. Default binary build
  completed successfully in session1020 (8m28s). No API/value or trust behavior
  changed. Final reviewed E2E session1221 completed successfully with this
  rebuilt binary; see evidence above.

## Next checkpoint

None required for P3–P7. The plan contains verification counts and the timing
exception; the guide contains commands and operational limits. External
deployment, cross-host/Linux headless/GPU validation and unrelated product
roadmap work remain outside this completed local task.

## Blockers

None. Broad test timing exception is documented; no claim of an entirely green
workspace run or of an independent external Codex review.
