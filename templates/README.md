# Hivemind Task Templates

Ready-to-use starting points for requestors. Hivemind runs the active
`managed-function-v1` runtime: a source function plus a JSON input payload.

## Available Templates

See `managed-function-v1/` for runnable samples. Each sample is a pair:

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
   hivemind submit templates/managed-function-v1/03_batch_sum.hmf \
     --input '{"items":[{"status":"paid","amount":10},{"status":"pending","amount":7},{"status":"paid","amount":25}]}' \
     --username user --password pass --max-cpt 1000
   ```

   The CLI `--input` option takes inline JSON; the matching `.input.json` file
   is a reference payload and is not read automatically. Or submit over HTTP
   with `POST /api/tasks` (see `docs/MANAGED_FUNCTION_RUNTIME.md`).

## Resource and Budget Overrides

Submission flags adjust the requested resources and budget:

- `--cpu-score` - minimum CPU benchmark score
- `--memory-gb` - RAM requirement
- `--gpu-score` - minimum GPU benchmark score
- `--gpu-memory-gb` - VRAM requirement
- `--storage-gb` - disk space requirement
- `--max-cpt` - the positive per-replica managed execution allowance; the
  initial hold covers every selected replica plus the platform fee, and unused
  held CPT is refunded after settlement

The historical `managed-function-v0/` fixtures remain in the repository for
frozen compatibility tests and old task documentation. New submissions must
use `managed-function-v1`.
