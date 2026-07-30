# Hivemind Task Templates

Ready-to-use starting points for requestors. Hivemind runs
`managed-function-v0` tasks: a source function plus a JSON input payload.

## Available Templates

See `managed-function-v0/` for runnable samples. Each sample is a pair:

| Sample | Use Case |
|--------|----------|
| `01_policy_gate` | Approve or reject a request from user risk and budget |
| `02_weighted_score` | Convert metrics into a weighted score and band |
| `03_batch_sum` | Summarize a list of payment records |
| `04_price_quote` | Estimate task price and check budget |
| `05_route_task` | Choose a worker pool and priority for a task |

## How to Use

1. Copy a `.hmf` source file and its matching `.input.json`.
2. Edit the function source and input for your workload.
3. Submit with the CLI:

   ```bash
   hivemind submit templates/managed-function-v0/03_batch_sum.hmf \
     --input templates/managed-function-v0/03_batch_sum.input.json \
     --username user --password pass --max-cpt 25
   ```

   Or submit over HTTP with `POST /api/tasks` (see `docs/MANAGED_FUNCTION_RUNTIME.md`).

## Resource and Budget Overrides

Submission flags adjust the requested resources and budget:

- `--cpu-score` - minimum CPU benchmark score
- `--memory-gb` - RAM requirement
- `--gpu-score` - minimum GPU benchmark score
- `--gpu-memory-gb` - VRAM requirement
- `--storage-gb` - disk space requirement
- `--max-cpt` - the managed execution budget; execution stops when it is spent
