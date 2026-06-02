# Hivemind Task Templates

Ready-to-use starting points for requestors. Each template is a self-contained directory:

## Available Templates

| Template | Use Case | Requirements |
|----------|----------|-------------|
| `python-script` | Run a Python script | CPU 100, 2GB RAM, 1GB storage |
| `data-processing` | Process input files → output | CPU 200, 4GB RAM, 10GB storage |
| `batch-render` | GPU-accelerated batch rendering | CPU 500, 8GB RAM, GPU, 50GB storage |
| `docker-job` | Run containerized workloads | CPU 100, 2GB RAM, 5GB storage |

## How to Use

1. Copy the template directory
2. Replace the entrypoint script with your own code
3. Adjust `task.json` resource requirements
4. Package: `zip -r task.zip .` inside the directory
5. Submit: `hivemind submit task.zip --username user --password pass`

## Structure

Each template contains:
- `task.json` — metadata and resource requirements
- `run.py` (or `run.sh`) — the entrypoint script
- Output goes to `./output/` directory

## Customization

Modify `task.json` to adjust:
- `cpu_score` — minimum CPU benchmark score
- `memory_gb` — RAM requirement
- `gpu_score` — minimum GPU benchmark score (add if needed)
- `gpu_memory_gb` — VRAM requirement (add if needed)
- `storage_gb` — disk space requirement

Run `hivemind submit <zip> --cpu-score 500 --memory-gb 8` to override at submission time.
