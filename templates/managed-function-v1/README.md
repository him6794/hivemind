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

`max_cpt` is a positive per-replica execution allowance, not a whole-task
charge ceiling. In enforced consensus mode, Nodepool initially holds the
allowance for each replica plus the platform fee. Settlement uses valid replica
usage and refunds the unused part of that hold.

The CLI's `--input` option takes inline JSON. For a file-based workflow, read
the matching `.input.json` file and pass its contents in that option, or submit
the JSON request directly to `POST /api/tasks`.

Templates:

- `01_policy_gate`: approve or reject a request from user risk and budget.
- `02_weighted_score`: convert metrics into a weighted score and band.
- `03_batch_sum`: summarize a list of payment records.
- `04_price_quote`: estimate task price and check budget.
- `05_route_task`: choose a worker pool and priority for a task.
