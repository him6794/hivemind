# Historical Managed Function v0 Templates

These fixtures preserve the frozen `managed-function-v0` contract for
compatibility tests and historical task records. They are not accepted for new
submission. Use [`../managed-function-v1/`](../managed-function-v1/) for active
work.

The archived request shape was:

```json
{
  "task_id": "example-managed-task",
  "runtime": "managed-function-v0",
  "task_source": "<contents of .hmf file>",
  "torrent": "<contents of matching .input.json file>",
  "max_cpt": 25
}
```

The `torrent` field was used as JSON input for managed functions.
`managed-function-v0` remains a frozen historical runtime contract. Operators
may still need the cross-platform `production_sandboxed_dsl` Worker backend to
read compatible historical work; that route does not require Windows
Containers/HCS.

Templates:

- `01_policy_gate`: approve or reject a request from user risk and budget.
- `02_weighted_score`: convert metrics into a weighted score and band.
- `03_batch_sum`: summarize a list of payment records.
- `04_price_quote`: estimate task price and check budget.
- `05_route_task`: choose a worker pool and priority for a task.
