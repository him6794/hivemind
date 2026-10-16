# Managed Function v1 Templates

These templates are small `managed-function-v1` tasks for the active managed
runtime. They use the same closed source-function and JSON-input language as the
historical v0 fixtures, but v1 meters each replica's execution and settles from
validated usage evidence.

Submit a task with the source and input contents:

```json
{
  "task_id": "example-managed-task",
  "runtime": "managed-function-v1",
  "task_source": "<contents of .hmf file>",
  "torrent": "<contents of matching .input.json file>",
  "max_cpt": 1000
}
```

`max_cpt` is the positive maximum total charge for the whole task, including
the platform fee and all three default replicas. It is not a per-replica usage
allowance. Nodepool deterministically assigns the same integer execution budget
to each replica from the fee-exclusive task budget; any indivisible remainder
stays with the owner. For example, a 100 CPT cap gives each of three replicas 30
CPT of execution budget, so the worst-case hold is 99 CPT (90 CPT of usage plus
9 CPT fee) and the remaining 1 CPT stays with the owner. Settlement aggregates
valid replica usage, adds the fee, and never exceeds the task cap. Unused held
CPT is refunded after settlement.

Automatic paid retries are disabled until a funded platform treasury is
available. Retry work is not charged to the task owner.

The CLI's `--input` option takes inline JSON. For a file-based workflow, read
the matching `.input.json` file and pass its contents in that option, or submit
the JSON request directly to `POST /api/tasks`.

Templates:

- `01_policy_gate`: approve or reject a request from user risk and budget.
- `02_weighted_score`: convert metrics into a weighted score and band.
- `03_batch_sum`: summarize a list of payment records.
- `04_price_quote`: estimate task price and check budget.
- `05_route_task`: choose a worker pool and priority for a task.
