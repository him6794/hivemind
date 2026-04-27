"""Regression tests: non-Docker task execution uses pydantic-monty and cleans up temp dir."""
from __future__ import annotations

import os
import sys
from io import BytesIO
from pathlib import Path
from threading import Event
from unittest.mock import patch
from zipfile import ZIP_DEFLATED, ZipFile

import pytest


def _import_task_executor():
    this_file = Path(__file__).resolve()
    worker_src = this_file.parents[2]
    if str(worker_src) not in sys.path:
        sys.path.insert(0, str(worker_src))
    from hivemind_worker import task_executor
    return task_executor


class DummyNode:
    def __init__(self):
        self.logs: list[str] = []
        self.docker_available = False
        self.docker_client = None
        self.docker_status = "unavailable"
        self.running_tasks = {"t1": {"status": "Queued", "resources": {}}}
        self.task_stop_events = {"t1": Event()}
        self.available_resources = {"cpu": 1, "memory_gb": 1, "gpu": 0, "gpu_memory_gb": 0}
        from threading import Lock
        self.resources_lock = Lock()
        self.worker_stub = None
        self.token = ""
        self.node_id = "dummy"
        self._released: list[str] = []
        self._sent_logs: list[tuple[str, str]] = []

    def _log(self, msg, level=None):
        self.logs.append(str(msg))

    def _release_resources(self, task_id: str):
        self._released.append(task_id)

    def _send_task_logs(self, task_id: str, logs: str):
        self._sent_logs.append((task_id, logs))


def _make_task_zip_bytes(files: dict[str, str]) -> bytes:
    buf = BytesIO()
    with ZipFile(buf, "w", ZIP_DEFLATED) as z:
        for name, content in files.items():
            z.writestr(name, content)
    return buf.getvalue()


def test_execute_task_uses_monty_and_cleans_temp_dir(monkeypatch: pytest.MonkeyPatch, tmp_path: Path):
    """Non-Docker execution must go through pydantic-monty and clean up the temp dir."""
    te = _import_task_executor()
    node = DummyNode()

    # Redirect mkdtemp to a known directory so we can assert cleanup
    temp_dir = tmp_path / "task_t1"
    temp_dir.mkdir(parents=True, exist_ok=True)
    monkeypatch.setattr(te, "mkdtemp", lambda prefix=None: str(temp_dir))

    # Stub _try_run_with_monty to succeed
    monty_called: list[str] = []

    def fake_monty(n, tid, workspace, stop_ev, required_resources=None):
        monty_called.append(tid)
        return True, ["monty ran ok"]

    monkeypatch.setattr(te, "_try_run_with_monty", fake_monty)

    # Capture rmtree calls to verify cleanup
    removed: list[str] = []
    monkeypatch.setattr(te, "rmtree", lambda p: removed.append(os.path.normpath(str(p))))

    task_zip = _make_task_zip_bytes({"main.py": "print('ok')\n"})
    te.execute_task(node, "t1", task_zip)

    assert monty_called == ["t1"], "_try_run_with_monty must be called for non-Docker tasks"
    assert os.path.normpath(str(temp_dir)) in removed, "temp dir must be cleaned up"
    assert "t1" in node._released, "resources must be released"
    assert any("monty ran ok" in log for log in node.logs)


def test_execute_task_monty_failure_reported(monkeypatch: pytest.MonkeyPatch, tmp_path: Path):
    """When monty returns False the task is marked as failed and result is uploaded."""
    te = _import_task_executor()
    node = DummyNode()

    temp_dir = tmp_path / "task_t1"
    temp_dir.mkdir(parents=True, exist_ok=True)
    monkeypatch.setattr(te, "mkdtemp", lambda prefix=None: str(temp_dir))

    monkeypatch.setattr(
        te, "_try_run_with_monty",
        lambda n, tid, ws, ev, req=None: (False, ["[Monty 執行錯誤] ZeroDivisionError: division by zero"])
    )
    monkeypatch.setattr(te, "rmtree", lambda p: None)

    task_zip = _make_task_zip_bytes({"main.py": "1/0\n"})
    te.execute_task(node, "t1", task_zip)

    assert "t1" in node._released
    assert any("ZeroDivisionError" in log for log in node.logs)
    # The final completion log should say 失敗
    assert any("失敗" in log for log in node.logs)


def test_execute_task_no_venv_functions_exist():
    """venv helper functions must not exist after removal."""
    te = _import_task_executor()
    for removed_fn in ('_candidate_bundled_python_paths', '_resolve_task_python',
                       '_create_task_venv', '_run_pip_install', '_is_real_python'):
        assert not hasattr(te, removed_fn), f"{removed_fn} should have been removed"
