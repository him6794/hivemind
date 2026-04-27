"""Local task runner for debugging venv execution.

This script is intentionally standalone:
- No nodepool/gRPC required
- Forces venv execution path (Docker disabled)
- Reads a task zip from disk and executes via `hivemind_worker.task_executor.execute_task`

Usage (PowerShell):
    python .\tools\run_task_zip_local.py --zip "D:\\discord-community-bot\\assets.zip"

Optional:
    --runtime-python "D:\\path\\to\\runtime\\python.exe"

Notes:
- The task zip must contain an executable entrypoint (e.g. main.py/run.py/run.cmd/run.bat).
- The task workspace is extracted to a temp dir and will be cleaned up at the end.
"""

from __future__ import annotations

import argparse
import os
import sys
from pathlib import Path
from threading import Event, Lock


def _ensure_worker_src_on_path() -> Path:
    here = Path(__file__).resolve()
    worker_root = here.parents[1]  # .../worker
    worker_src = worker_root / "src"
    if str(worker_src) not in sys.path:
        sys.path.insert(0, str(worker_src))
    return worker_root


class _LocalNode:
    """Minimal node shim for task_executor.execute_task."""

    def __init__(self):
        self.logs: list[str] = []
        self.resources_lock = Lock()

        # Force venv path
        self.docker_available = False
        self.docker_client = None
        self.docker_status = "unavailable"

        self.running_tasks: dict[str, dict] = {}
        self.task_stop_events: dict[str, Event] = {}
        self.available_resources = {"cpu": 1, "memory_gb": 1, "gpu": 0, "gpu_memory_gb": 0}

        # upload stubs
        self.worker_stub = None
        self.token = ""
        self.node_id = "local"

    def _log(self, msg, level=None):
        line = str(msg)
        self.logs.append(line)
        print(line)

    def _send_task_logs(self, task_id: str, logs: str):
        # stream to console only
        for line in str(logs).splitlines():
            print(f"[TaskLog {task_id}] {line}")

    def _release_resources(self, task_id: str):
        # no-op for local runner
        return


def main() -> int:
    worker_root = _ensure_worker_src_on_path()

    default_runtime_candidates = [
        worker_root / "runtime" / "python.exe",
        worker_root / "runtime" / "python312.exe",
        worker_root / "runtime" / "python" / "python.exe",
        worker_root / "runtime" / "python" / "python312.exe",
    ]
    default_runtime_python = next((p for p in default_runtime_candidates if p.exists()), None)

    parser = argparse.ArgumentParser(description="Run a task zip locally via venv execution (no Docker).")
    parser.add_argument("--zip", dest="zip_path", default=r"D:\discord-community-bot\assets.zip")
    parser.add_argument(
        "--runtime-python",
        dest="runtime_python",
        default=str(default_runtime_python) if default_runtime_python else None,
        help=(
            "Explicit runtime python.exe path used to create the task venv. "
            "Default: auto-detect under worker/runtime if present."
        ),
    )
    parser.add_argument(
        "--allow-system-python",
        action="store_true",
        help=(
            "If set, allow falling back to system python on PATH when runtime python is not found. "
            "(Default behavior is to fail fast to avoid accidental use of local Python.)"
        ),
    )
    parser.add_argument("--task-id", dest="task_id", default="local-task")
    args = parser.parse_args()

    zip_path = Path(args.zip_path)
    if not zip_path.exists():
        print(f"[ERROR] Task zip not found: {zip_path}")
        return 2

    task_zip_bytes = zip_path.read_bytes()

    from hivemind_worker import task_executor as te

    node = _LocalNode()
    node.running_tasks[args.task_id] = {"status": "Queued", "resources": {}}
    node.task_stop_events[args.task_id] = Event()

    # Prefer runtime python and fail-fast by default (to avoid accidentally using developer machine Python).
    if args.runtime_python:
        runtime_python = os.path.normpath(args.runtime_python)
        if not os.path.exists(runtime_python):
            print(f"[ERROR] --runtime-python path not found: {runtime_python}")
            return 2

        te._resolve_task_python = lambda _node: runtime_python  # type: ignore[attr-defined]
        print(f"[Runner] Using runtime python: {runtime_python}")
        resolved = runtime_python
    else:
        if not args.allow_system_python:
            print(
                "[ERROR] Runtime python not found under worker/runtime, and --allow-system-python not set.\n"
                "Provide --runtime-python to force a bundled interpreter (recommended)."
            )
            return 2
        resolved = te._resolve_task_python(node)

    if not resolved:
        print("[ERROR] No usable python resolved for task execution.")
        return 2
    print(f"[Runner] Resolved base python: {resolved}")

    te.execute_task(node, args.task_id, task_zip_bytes)
    print("[Runner] Done.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
