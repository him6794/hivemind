"""Tests for the pydantic-monty execution path in task_executor."""
from __future__ import annotations

import sys
from io import BytesIO
from pathlib import Path
from threading import Event
from unittest.mock import MagicMock, patch
from zipfile import ZIP_DEFLATED, ZipFile

import pytest


# ---------------------------------------------------------------------------
# helpers
# ---------------------------------------------------------------------------

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


def _make_zip(files: dict[str, str]) -> bytes:
    buf = BytesIO()
    with ZipFile(buf, "w", ZIP_DEFLATED) as z:
        for name, content in files.items():
            z.writestr(name, content)
    return buf.getvalue()


# ---------------------------------------------------------------------------
# _try_run_with_monty tests
# ---------------------------------------------------------------------------

class TestTryRunWithMonty:
    """Unit tests for _try_run_with_monty()."""

    def test_returns_false_when_pydantic_monty_not_installed(self, tmp_path):
        te = _import_task_executor()
        node = DummyNode()
        (tmp_path / "main.py").write_text("x = 1\n")

        with patch.dict("sys.modules", {"pydantic_monty": None}):
            status, lines = te._try_run_with_monty(node, "t1", str(tmp_path), Event())

        assert status is False
        assert any("pydantic-monty" in m or "未安裝" in m for m in node.logs)

    def test_returns_false_when_requirements_txt_present(self, tmp_path):
        te = _import_task_executor()
        node = DummyNode()
        (tmp_path / "main.py").write_text("print('hello')\n")
        (tmp_path / "requirements.txt").write_text("numpy\n")

        mock_pm = MagicMock()
        with patch.dict("sys.modules", {"pydantic_monty": mock_pm}):
            status, lines = te._try_run_with_monty(node, "t1", str(tmp_path), Event())

        assert status is False
        assert any("requirements.txt" in m for m in node.logs)
        assert any("requirements.txt" in l for l in lines)

    def test_success_captures_stdout(self, tmp_path):
        te = _import_task_executor()
        node = DummyNode()
        (tmp_path / "main.py").write_text("print('hello world')\n")

        mock_pm = MagicMock()
        mock_monty_instance = MagicMock()

        def fake_run(*, print_callback=None, limits=None, inputs=None, **kw):
            if print_callback:
                print_callback("stdout", "hello world")
                print_callback("stdout", "\n")
            return None

        mock_monty_instance.run.side_effect = fake_run
        mock_pm.Monty.return_value = mock_monty_instance

        with patch.dict("sys.modules", {"pydantic_monty": mock_pm}):
            status, lines = te._try_run_with_monty(node, "t1", str(tmp_path), Event())

        assert status is True
        assert "hello world" in lines

    def test_success_with_return_value(self, tmp_path):
        te = _import_task_executor()
        node = DummyNode()
        (tmp_path / "main.py").write_text("1 + 2\n")

        mock_pm = MagicMock()
        mock_instance = MagicMock()
        mock_instance.run.return_value = 3
        mock_pm.Monty.return_value = mock_instance

        with patch.dict("sys.modules", {"pydantic_monty": mock_pm}):
            status, lines = te._try_run_with_monty(node, "t1", str(tmp_path), Event())

        assert status is True
        assert any("[return]" in l for l in lines)

    def test_runtime_error_reported_as_failure(self, tmp_path):
        te = _import_task_executor()
        node = DummyNode()
        (tmp_path / "main.py").write_text("1 / 0\n")

        mock_pm = MagicMock()
        mock_instance = MagicMock()
        mock_instance.run.side_effect = RuntimeError("ZeroDivisionError: division by zero")
        mock_pm.Monty.return_value = mock_instance

        with patch.dict("sys.modules", {"pydantic_monty": mock_pm}):
            status, lines = te._try_run_with_monty(node, "t1", str(tmp_path), Event())

        assert status is False
        assert any("ZeroDivisionError" in l or "執行錯誤" in l for l in lines)

    def test_import_error_returns_false(self, tmp_path):
        """Even ImportError inside task code is a failure (no venv fallback)."""
        te = _import_task_executor()
        node = DummyNode()
        (tmp_path / "main.py").write_text("import numpy\n")

        mock_pm = MagicMock()
        mock_instance = MagicMock()
        mock_instance.run.side_effect = RuntimeError("ModuleNotFoundError: No module named 'numpy'")
        mock_pm.Monty.return_value = mock_instance

        with patch.dict("sys.modules", {"pydantic_monty": mock_pm}):
            status, lines = te._try_run_with_monty(node, "t1", str(tmp_path), Event())

        # No venv anymore — must be False, not None
        assert status is False

    def test_syntax_error_returns_false(self, tmp_path):
        te = _import_task_executor()
        node = DummyNode()
        (tmp_path / "main.py").write_text("class Foo: pass\n")

        mock_pm = MagicMock()

        class FakeMontySyntaxError(Exception):
            pass

        mock_pm.MontySyntaxError = FakeMontySyntaxError
        mock_pm.Monty.side_effect = FakeMontySyntaxError("unsupported syntax: class")

        with patch.dict("sys.modules", {"pydantic_monty": mock_pm}):
            status, lines = te._try_run_with_monty(node, "t1", str(tmp_path), Event())

        # Must be False (no venv fallback)
        assert status is False
        assert any("解析失敗" in l or "腳本解析失敗" in m for l in lines for m in node.logs)


# ---------------------------------------------------------------------------
# execute_task integration: monty as sole non-Docker path
# ---------------------------------------------------------------------------

class TestExecuteTaskMontyPath:
    """Ensure execute_task uses monty (only) when Docker is unavailable."""

    def test_execute_task_uses_monty(self, monkeypatch, tmp_path):
        te = _import_task_executor()
        node = DummyNode()

        temp_dir = tmp_path / "task_t1"
        temp_dir.mkdir(parents=True, exist_ok=True)
        monkeypatch.setattr(te, "mkdtemp", lambda prefix=None: str(temp_dir))

        monty_called = []

        def fake_try_run_with_monty(n, tid, workspace, stop_ev, required_resources=None):
            monty_called.append(True)
            return True, ["monty output line"]

        monkeypatch.setattr(te, "_try_run_with_monty", fake_try_run_with_monty)
        monkeypatch.setattr(te, "rmtree", lambda p: None)

        task_zip = _make_zip({"main.py": "print('hi')\n"})
        te.execute_task(node, "t1", task_zip)

        assert monty_called, "monty should have been called"
        assert any("monty output line" in log for log in node.logs)

    def test_execute_task_monty_failure_marks_failed(self, monkeypatch, tmp_path):
        te = _import_task_executor()
        node = DummyNode()

        temp_dir = tmp_path / "task_t1"
        temp_dir.mkdir(parents=True, exist_ok=True)
        monkeypatch.setattr(te, "mkdtemp", lambda prefix=None: str(temp_dir))
        monkeypatch.setattr(te, "_try_run_with_monty", lambda *a, **kw: (False, ["任務失敗"]))
        monkeypatch.setattr(te, "rmtree", lambda p: None)

        task_zip = _make_zip({"main.py": "raise Exception()\n"})
        te.execute_task(node, "t1", task_zip)

        assert any("失敗" in log for log in node.logs)
        assert "t1" in node._released
