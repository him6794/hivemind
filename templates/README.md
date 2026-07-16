# Hivemind Task Templates

Ready-to-use starting points for requestors. Each template is a self-contained directory:

## Available Templates

| Template | Use Case | Requirements |
|----------|----------|-------------|
| `python-script` | Run a Monty-compatible Python script and report its result | CPU 100, 2GB RAM, 1GB storage |
| `data-processing` | Process example data and report a summary | CPU 200, 4GB RAM, 10GB storage |
| `batch-render` | Render a deterministic frame range and report completion | CPU 500, 8GB RAM, GPU, 50GB storage |

## How to Use

1. Copy the template directory
2. Replace the top-level `main.py` entrypoint with your own Monty-compatible code
3. Adjust `task.json` resource requirements
4. Package: `zip -r task.zip .` inside the directory
5. Submit: `hivemind submit task.zip --username user --password pass`

## Structure

Each template contains:
- `task.json` — metadata and resource requirements
- `main.py` — the top-level Python entrypoint required by the Rust worker ZIP contract
- Results are reported to stdout by the Rust worker

## Customization

Modify `task.json` to adjust:
- `cpu_score` — minimum CPU benchmark score
- `memory_gb` — RAM requirement
- `gpu_score` — minimum GPU benchmark score (add if needed)
- `gpu_memory_gb` — VRAM requirement (add if needed)
- `storage_gb` — disk space requirement

Run `hivemind submit <zip> --cpu-score 500 --memory-gb 8` to override resource requirements at submission time.

The Rust worker runs each ZIP's `main.py` with the managed Monty file contract. Keep imports one module per statement, use only the supported standard library, and do not rely on `requirements.txt` packages or host filesystem access.

`docker-job` remains a legacy reference only. It is not an executable task package for the authoritative Rust worker runtime.
