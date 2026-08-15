# Managed Function v0 Templates

These templates are small `managed-function-v0` tasks that can be submitted with:

```json
{
  "task_id": "example-managed-task",
  "runtime": "managed-function-v0",
  "task_source": "<contents of .hmf file>",
  "torrent": "<contents of matching .input.json file>",
  "max_cpt": 25
}
```

The `torrent` field is used as JSON input for managed functions.
`managed-function-v0` remains the proof-facing runtime contract. Operators may
select the cross-platform `production_sandboxed_dsl` Worker backend for the
same closed interpreter; that route does not require Windows Containers/HCS.

Templates:

- `01_policy_gate`: approve or reject a request from user risk and budget.
- `02_weighted_score`: convert metrics into a weighted score and band.
- `03_batch_sum`: summarize a list of payment records.
- `04_price_quote`: estimate task price and check budget.
- `05_route_task`: choose a worker pool and priority for a task.
