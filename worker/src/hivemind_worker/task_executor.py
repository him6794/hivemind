"""Task execution and packaging utilities for WorkerNode.

This module encapsulates task execution via pydantic-monty sandboxed interpreter,
result packaging, and script discovery, so that WorkerNode stays lean.

Execution mode is locked to pydantic-monty only. Docker is explicitly disabled.

Contract:
- execute_task(node, task_id: str, task_zip_bytes: bytes, required_resources: dict | None) -> None
  Uses fields and methods on `node`:
    node._log(level=..), node._send_task_logs(...), node.task_stop_events,
    node.resources_lock, node.running_tasks, node.available_resources,
    node.worker_stub, node.token, node.node_id

Edge cases handled:
- Missing stop event
- Periodic log shipping and final result upload
"""

from __future__ import annotations

import os
import time
from io import BytesIO
from zipfile import ZipFile, ZIP_DEFLATED
from tempfile import mkdtemp
from shutil import rmtree
from os.path import join, exists, relpath, dirname
from os import makedirs, walk

try:
    from . import nodepool_pb2
except Exception:  # pragma: no cover - fallback when running as script
    import nodepool_pb2  # type: ignore


def _safe_extract_zip(zip_ref: ZipFile, dest_dir: str) -> None:
    base = os.path.abspath(dest_dir)
    for member in zip_ref.infolist():
        name = member.filename
        normalized = os.path.normpath(name)
        if os.path.isabs(normalized) or normalized.startswith('..'):
            raise ValueError(f"Invalid ZIP entry path: {name}")
        target = os.path.abspath(os.path.join(dest_dir, normalized))
        if not (target == base or target.startswith(base + os.sep)):
            raise ValueError(f"Invalid ZIP entry path: {name}")
    zip_ref.extractall(dest_dir)


def _find_executable_script(node, task_dir: str):
    """Find an executable script inside task_dir and return (script_path, cmd_list).
    If a Python file is selected, the caller can prepend the desired interpreter.
    """
    script_candidates = [
        ('run.py', ['python'], 'Python runner'),
        ('main.py', ['python'], 'Python main'),
        ('app.py', ['python'], 'Python app'),
        ('index.py', ['python'], 'Python index'),
        ('start.py', ['python'], 'Python start'),
    ]

    for script_name, cmd_prefix, description in script_candidates:
        script_path = os.path.join(task_dir, script_name)
        if os.path.exists(script_path):
            if cmd_prefix:
                try:
                    if cmd_prefix[0] == 'python':
                        node._log(f"找到可執行腳本： {script_name} ({description})")
                        return script_path, [script_name]
                    import subprocess
                    result = subprocess.run([cmd_prefix[0], '--version'], capture_output=True, timeout=5)
                    if result.returncode != 0:
                        node._log(f"命令 {cmd_prefix[0]} 可用，跳過 {script_name}")
                        continue
                except (Exception,):
                    node._log(f"命令 {cmd_prefix[0]} 檢查失敗，跳過 {script_name}")
                    continue
            node._log(f"找到可執行腳本： {script_name} ({description})")
            return script_path, (cmd_prefix + [script_path] if cmd_prefix else [script_path])

    try:
        python_files = [f for f in os.listdir(task_dir) if f.endswith('.py')]
        if python_files:
            priority = ['main.py', 'app.py', 'run.py', 'start.py']
            selected = next((pf for pf in priority if pf in python_files), None) or python_files[0]
            script_path = os.path.join(task_dir, selected)
            node._log(f"使用找到的 Python腳本: {selected}")
            return script_path, [selected]
    except OSError as e:
        node._log(f"找到可執行腳本失敗: {e}")

    return None, None


def _create_result_zip(node, task_id: str, workspace: str, success: bool, stopped: bool = False, task_logs: list[str] | None = None):
    """Create a ZIP buffer of the workspace with execution logs included."""
    try:
        log_file = join(workspace, "execution_log.txt")
        with open(log_file, 'w', encoding='utf-8') as f:
            f.write(f"Task ID: {task_id}\n")
            if stopped:
                f.write("Status: Stopped by user\n")
                f.write("Execution Result: Terminated\n")
            else:
                f.write(f"Status: {'Success' if success else 'Failed'}\n")
                f.write(f"Execution Result: {'Completed' if success else 'Error'}\n")
            f.write(f"Time: {time.strftime('%Y-%m-%d %H:%M:%S')}\n")
            f.write(f"Node: {node.node_id}\n")
            if stopped:
                f.write("\nNote: This task was stopped by user request.\n")
                f.write("Any partial results or intermediate files are included in this package.\n")

        if task_logs:
            task_log_file = join(workspace, "task_logs.txt")
            with open(task_log_file, 'w', encoding='utf-8') as f:
                f.write(f"=== Task {task_id} Complete Logs ===\n")
                f.write(f"Generated at: {time.strftime('%Y-%m-%d %H:%M:%S')}\n")
                f.write(f"Status: {'Stopped by user' if stopped else ('Success' if success else 'Failed')}\n")
                f.write(f"Node: {node.node_id}\n\n")
                for entry in task_logs:
                    f.write(f"{entry}\n")
                f.write("\n=== End of Logs ===\n")

        if stopped:
            stop_file = join(workspace, "task_stopped.txt")
            with open(stop_file, 'w', encoding='utf-8') as f:
                f.write(f"Task {task_id} was stopped by user request at {time.strftime('%Y-%m-%d %H:%M:%S')}\n")
                f.write("This file indicates that the task did not complete normally.\n")
                f.write("Check execution_log.txt and task_logs.txt for more details.\n")

        zip_buffer = BytesIO()
        with ZipFile(zip_buffer, 'w', ZIP_DEFLATED) as zip_file:
            for root, dirs, files in walk(workspace):
                for file in files:
                    file_path = join(root, file)
                    arcname = relpath(file_path, workspace)
                    zip_file.write(file_path, arcname)

        result_size = len(zip_buffer.getvalue())
        node._log(f"Created result zip for task {task_id}: {result_size} bytes ({'stopped' if stopped else 'completed'}), logs included")
        return zip_buffer.getvalue()

    except Exception as e:
        node._log(f"Failed to create result zip: {e}")
        try:
            zip_buffer = BytesIO()
            with ZipFile(zip_buffer, 'w', ZIP_DEFLATED) as zip_file:
                error_content = (
                    f"Task {task_id} packaging failed: {str(e)}\n"
                    f"Status: {'Stopped' if stopped else 'Failed'}\n"
                    f"Time: {time.strftime('%Y-%m-%d %H:%M:%S')}\n"
                )
                if task_logs:
                    error_content += "\n=== Partial Logs ===\n" + "\n".join(task_logs[-50:]) + "\n"
                zip_file.writestr("error_log.txt", error_content)
            return zip_buffer.getvalue()
        except Exception:
            return None


def _try_run_with_monty(node, task_id: str, workspace: str, stop_event, required_resources: dict | None = None) -> tuple[bool, list[str]]:
    """Execute the task using pydantic-monty sandboxed interpreter.

    Returns:
        (True,  log_lines) ??task executed successfully.
        (False, log_lines) ??task failed (monty unavailable, unsupported code, or runtime error).

    pydantic-monty is a minimal, secure Python interpreter written in Rust.
    It supports a reasonable subset of Python but cannot install third-party packages.
    Tasks that require external packages must be run via Docker.
    """
    if not required_resources:
        required_resources = {}
    # ── availability check ─────────────────────────────
    try:
        import pydantic_monty  # noqa: PLC0415
    except ImportError:
        msg = "[Monty] pydantic-monty 未安裝，無法在非 Docker 模式執行任務"
        node._log(msg)
        return False, [msg]

    # Monty cannot install third-party packages
    req_file = os.path.join(workspace, 'requirements.txt')
    if os.path.exists(req_file):
        msg = "[Monty] 任務包含 requirements.txt，pydantic-monty 不支援安裝第三方套件；請改用 Docker 模式"
        node._log(msg)
        return False, [msg]

    # Only handle Python scripts
    script_path, _ = _find_executable_script(node, workspace)
    if not script_path or not script_path.endswith('.py'):
        msg = "[Monty] 未找到 Python 腳本，無法執行"
        node._log(msg)
        return False, [msg]

    try:
        with open(script_path, 'r', encoding='utf-8') as f:
            code = f.read()
    except Exception as e:
        msg = f"[Monty] 無法讀取腳本: {e}"
        node._log(msg)
        return False, [msg]

    # ── stdout capture via print_callback ────────────────────
    output_lines: list[str] = []
    _buf: list[str] = []  # partial-line buffer

    def _print_cb(output_type: str, text: str) -> None:
        _buf.append(text)
        combined = ''.join(_buf)
        while '\n' in combined:
            idx = combined.index('\n')
            line = combined[:idx]
            combined = combined[idx + 1:]
            _buf.clear()
            _buf.append(combined)
            output_lines.append(line)

    # ── host-side network functions ───────────────────────
    import urllib.request as _urllib_req  # noqa: PLC0415
    import json as _json_mod              # noqa: PLC0415

    _net_timeout = 15.0  # HTTP 請求逾時（秒）

    def _host_http_get(url: str) -> str:
        with _urllib_req.urlopen(url, timeout=_net_timeout) as r:
            return r.read().decode('utf-8', errors='replace')

    def _host_http_get_json(url: str):
        return _json_mod.loads(_host_http_get(url))

    def _host_http_post(url: str, body: str = '', content_type: str = 'application/json') -> str:
        req = _urllib_req.Request(url, data=body.encode('utf-8'), method='POST')
        req.add_header('Content-Type', content_type)
        with _urllib_req.urlopen(req, timeout=_net_timeout) as r:
            return r.read().decode('utf-8', errors='replace')

    _network_funcs = {
        'http_get':      _host_http_get,       # http_get(url) -> str
        'http_get_json': _host_http_get_json,  # http_get_json(url) -> dict/list
        'http_post':     _host_http_post,      # http_post(url, body, content_type) -> str
    }

    # ── parse ────────────────────────────────────────
    node._log(f"[Monty] 使用 pydantic-monty 執行任務 {task_id}: {os.path.basename(script_path)}")
    try:
        m = pydantic_monty.Monty(
            code,
            script_name=os.path.basename(script_path),
            external_functions=list(_network_funcs.keys()),
        )
    except Exception as e:
        msg = f"[Monty] 腳本解析失敗: {type(e).__name__}: {e}"
        node._log(msg, level=30)
        return False, [msg]

    # ── run ─────────────────────────────────────────────
    try:
        # 將 cpu_score 換算為 cpu_fraction（0.0～1.0）
        try:
            rc = float(required_resources.get('cpu', 1))
        except Exception:
            rc = 1.0
        try:
            total_cores = float(os.cpu_count() or 1)
        except Exception:
            total_cores = 1.0
        advertised_score = getattr(node, 'advertised_cpu_score', None)
        if advertised_score and rc > 1 and rc > total_cores:
            # score 模型：cpu_fraction = requested_score / advertised_score
            cpu_fraction = min(float(rc) / float(advertised_score), 1.0) if advertised_score > 0 else 1.0
        elif rc <= 0:
            cpu_fraction = 0.01
        elif rc <= 1:
            cpu_fraction = rc                        # 已達上限，鉄制在 rc
        elif 1 < rc <= 100:
            cpu_fraction = rc / 100.0 * total_cores / total_cores  # % ??fraction
        else:
            cpu_fraction = 1.0
        cpu_fraction = max(0.01, min(cpu_fraction, 1.0))

        # ── 組建 ResourceLimits ──────────────────────────────
        mem_gb = max(float(required_resources.get('memory_gb', 1)), 0.5)  # 最低 0.5 GB
        # max_allocations 根據 cpu_fraction 等比例縮放
        max_alloc = max(int(10_000_000 * cpu_fraction), 1_000_000)
        limits = pydantic_monty.ResourceLimits(
            max_duration_secs=float(required_resources.get('max_duration_secs', 300.0)),
            max_memory=int(mem_gb * 1024 * 1024 * 1024),
            max_allocations=max_alloc,
            max_recursion_depth=int(required_resources.get('max_recursion_depth', 500)),
        )
        node._log(
            f"[Monty] 資源限制：time={limits['max_duration_secs']}s, "
            f"memory={mem_gb}GB, cpu_fraction={cpu_fraction:.2%}, "
            f"allocations={limits['max_allocations']}, recursion={limits['max_recursion_depth']}"
        )
        result = m.run(print_callback=_print_cb, limits=limits, external_functions=_network_funcs)

        # flush partial-line buffer
        remaining = ''.join(_buf)
        if remaining:
            output_lines.append(remaining)

        if result is not None:
            output_lines.append(f"[return]: {result!r}")

        node._log(f"[Monty] 任務 {task_id} pydantic-monty 執行完成")
        return True, output_lines

    except Exception as e:
        err_str = str(e)
        err_type = type(e).__name__

        # flush partial-line buffer before reporting
        remaining = ''.join(_buf)
        if remaining:
            output_lines.append(remaining)

        error_line = f"[Monty 執行錯誤] {err_type}: {err_str}"
        output_lines.append(error_line)
        node._log(f"[Monty] 任務 {task_id} 執行失敗: {err_type}: {err_str}")
        return False, output_lines


def execute_task(node, task_id: str, task_zip_bytes: bytes, required_resources: dict | None = None) -> None:
    """Execute a task via pydantic-monty sandboxed interpreter, stream logs, and upload result.

    Docker execution is intentionally disabled. All tasks run through pydantic-monty only.
    """
    if not required_resources:
        required_resources = {"cpu": 1, "memory_gb": 1, "gpu": 0, "gpu_memory_gb": 0}

    with node.resources_lock:
        if task_id in node.running_tasks:
            node.running_tasks[task_id]["status"] = "Executing"
            node.running_tasks[task_id]["exec_mode"] = "monty"

    stop_event = node.task_stop_events.get(task_id)
    if not stop_event:
        node._log(f"找不到任務 {task_id} 的停止事件")
        node._release_resources(task_id)
        return

    temp_dir = None
    success = False
    task_logs: list[str] = []
    stop_requested = False

    try:
        node._log(f"任務 {task_id} 執行模式：pydantic-monty")

        temp_dir = mkdtemp(prefix=f"task_{task_id}_")
        workspace = join(temp_dir, "workspace")
        makedirs(workspace)

        with ZipFile(BytesIO(task_zip_bytes), 'r') as zip_ref:
            _safe_extract_zip(zip_ref, workspace)
        node._log(f"Task {task_id} files extracted to {workspace}")

        #  pydantic-monty 沙盒執行 
        success, monty_lines = _try_run_with_monty(node, task_id, workspace, stop_event, required_resources)
        log_buffer_m: list[str] = []
        for line in monty_lines:
            node._log(f"[Task {task_id}]: {line}")
            task_logs.append(line)
            log_buffer_m.append(line)
        if log_buffer_m:
            node._send_task_logs(task_id, "\n".join(log_buffer_m))

        if stop_requested:
            completion_log = f"任務 {task_id} 被強制停止"
        else:
            completion_log = f"任務 {task_id} 執行{'成功' if success else '失敗'}"
        node._log(completion_log)
        task_logs.append(completion_log)
        node._send_task_logs(task_id, completion_log)

        result_zip = _create_result_zip(node, task_id, workspace, success, stop_requested, task_logs)
        if result_zip:
            try:
                if getattr(node, 'worker_stub', None):
                    node.worker_stub.TaskResultUpload(
                        nodepool_pb2.TaskResultUploadRequest(
                            task_id=task_id,
                            result_zip=result_zip,
                            token=node.token or "",
                        ),
                        metadata=[('authorization', f'Bearer {node.token}')],
                        timeout=30,
                    )
                    node._log(f"任務 {task_id} 結果已上傳 (TaskResultUpload)")
                else:
                    node._log("worker_stub 不可用，跳過結果上傳")
            except Exception as e:
                node._log(f"上傳結果失敗 (TaskResultUpload): {e}")

    except Exception as e:
        error_log = f"任務 {task_id} 執行失敗: {e}"
        node._log(error_log)
        task_logs.append(error_log)
        node._send_task_logs(task_id, error_log)
        success = False

    finally:
        if temp_dir and exists(temp_dir):
            try:
                rmtree(temp_dir)
            except Exception:
                pass
        try:
            note = f"Task {task_id} completed; success={success and not stop_requested}"
            node._log(note)
        except Exception:
            pass
        node._release_resources(task_id)
        node._log(f"Task {task_id} resources released, status: {'success' if success else 'failed'}")
