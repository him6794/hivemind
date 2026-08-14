# Reviewed general-compute OCI fixture

`scripts/general-compute-oci-e2e.ps1 -Run` is deliberately an operator-gated
release check. Set `HIVEMIND_GENERAL_COMPUTE_OCI_E2E_TASK_FIXTURE` to the
reviewed fixture script (the repository implementation is
`scripts/general-compute-oci-task-fixture.ps1`) and set
`HIVEMIND_GENERAL_COMPUTE_OCI_E2E_CASES` to an operator-owned case plan.

The harness invokes the fixture twice:

1. `-Phase provision` receives the host registry and project-prefixed volume
   names. It must copy only the operator-approved bundle rootfs, pinned runner,
   canonical seccomp profile, and rewritten `backends.json` into the named
   volumes. The Worker must see fixed in-container paths (`/etc/hivemind/...`
   and `/var/lib/hivemind/...`); task input must never supply a host path.
2. `-Phase execute` runs after `postgres`, `redis`, `nodepool`, `master`, and
   `worker` are up. It must authenticate through Master, wait for the Worker
   registration, submit and poll a real `general-compute-v1alpha1` task, and
   query Nodepool/Postgres for the persisted typed result and settlement.

The case-plan JSON is intentionally explicit because request digests are
canonical Rust/serde bytes and cannot be safely guessed by a shell script:

```json
{
  "max_cpt": 1000,
  "primary_manifest": { "...": "a complete validated GeneralComputeRequest" },
  "timeout_cancel": {
    "manifest": { "...": "a long-running validated request" },
    "cancel_after_seconds": 1,
    "expected_task_status": "CANCELLED",
    "expected_result_status": "cancelled"
  },
  "network_denied": {
    "manifest": { "...": "a request whose guest attempts network egress" },
    "expected_task_status": "FAILED",
    "expected_result_status": "backend_unavailable"
  },
  "filesystem_denied": {
    "manifest": { "...": "a request whose guest attempts a forbidden write" },
    "expected_task_status": "FAILED",
    "expected_result_status": "backend_unavailable"
  }
}
```

Each manifest must already carry a correct `request_digest`; the API remains
the authority that validates it. The fixture does not synthesize or weaken that
identity. The execute phase writes
`general-compute-oci-e2e-v1` evidence with service identity, all required case
checks, and the durable `general-compute-result-v1` fields. The harness then
validates that evidence and retains it under `test_logs/` (or the absolute path
from `HIVEMIND_GENERAL_COMPUTE_OCI_E2E_EVIDENCE`).

The repository fixture is an orchestration implementation, not a claim that a
host supports nested rootless OCI. Missing registry material, unsupported
runner primitives, malformed plans, failed settlement, or any hostile-workload
case mismatch fails closed and leaves the overall release status `running`.
