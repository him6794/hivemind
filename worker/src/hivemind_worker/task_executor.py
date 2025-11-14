"""Task execution and packaging utilities for WorkerNode.

This module encapsulates the heavy task execution logic (Docker/venv),
result packaging, and script discovery, so that WorkerNode stays lean.

Contract:
- execute_task(node, task_id: str, task_zip_bytes: bytes, required_resources: dict | None) -> None
  Uses fields and methods on `node`:
    node._log(level=..), node._send_task_logs(...), node.task_stop_events,
    node.resources_lock, node.running_tasks, node.available_resources,
    node.docker_available, node.docker_client, node.docker_status,
    node.worker_stub, node.token, node.node_id

Edge cases handled:
- Missing stop event
- Docker unavailable/failing -> fallback to venv
- Periodic log shipping and final result upload
"""

from __future__ import annotations

import os
import time
from io import BytesIO
from zipfile import ZipFile, ZIP_DEFLATED
from tempfile import mkdtemp
from secrets import token_hex
from shutil import copy2, rmtree
from os.path import join, exists, relpath, dirname
from os import makedirs, walk, chmod

try:
    from . import nodepool_pb2
except Exception:  # pragma: no cover - fallback when running as script
    import nodepool_pb2  # type: ignore


def _find_executable_script(node, task_dir: str):
    """Find an executable script inside task_dir and return (script_path, cmd_list).
    If a Python file is selected, the caller can prepend the desired interpreter.
    """
    script_candidates = [
        ('run.bat', [], 'Windows batch') if os.name == 'nt' else ('run.sh', ['bash'], 'Bash script'),
        ('run.cmd', [], 'Windows cmd'),
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
                        node._log(f"找到可執行腳本: {script_name} ({description})")
                        return script_path, [script_name]
                    import subprocess
                    result = subprocess.run([cmd_prefix[0], '--version'], capture_output=True, timeout=5)
                    if result.returncode != 0:
                        node._log(f"命令 {cmd_prefix[0]} 不可用，跳過 {script_name}")
                        continue
                except (Exception,):
                    node._log(f"命令 {cmd_prefix[0]} 檢查失敗，跳過 {script_name}")
                    continue
            node._log(f"找到可執行腳本: {script_name} ({description})")
            return script_path, (cmd_prefix + [script_path] if cmd_prefix else [script_path])

    try:
        python_files = [f for f in os.listdir(task_dir) if f.endswith('.py')]
        if python_files:
            priority = ['main.py', 'app.py', 'run.py', 'start.py']
            selected = next((pf for pf in priority if pf in python_files), None) or python_files[0]
            script_path = os.path.join(task_dir, selected)
            node._log(f"使用找到的Python檔案: {selected}")
            return script_path, [selected]
    except OSError as e:
        node._log(f"無法讀取任務目錄: {e}")

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


def execute_task(node, task_id: str, task_zip_bytes: bytes, required_resources: dict | None = None) -> None:
    """Execute a task in Docker if available, otherwise via venv, stream logs, and upload result.
    This mirrors the original WorkerNode._execute_task behavior.
    """
    if not required_resources:
        required_resources = {"cpu": 1, "memory_gb": 1, "gpu": 0, "gpu_memory_gb": 0}

    with node.resources_lock:
        if task_id in node.running_tasks:
            node.running_tasks[task_id]["status"] = "Executing"

    stop_event = node.task_stop_events.get(task_id)
    if not stop_event:
        node._log(f"找不到任務 {task_id} 的停止事件")
        node._release_resources(task_id)
        return

    temp_dir = None
    container = None
    success = False
    task_logs: list[str] = []
    stop_requested = False

    try:
        use_docker = node.docker_available and getattr(node, 'docker_client', None) is not None
        node._log(
            f"任務 {task_id} 執行模式判斷：docker_available={node.docker_available}, "
            f"docker_client={'存在' if getattr(node, 'docker_client', None) else '不存在'}, "
            f"最終使用={'Docker' if use_docker else 'venv'}"
        )
        if not use_docker:
            node._log(f"Docker 不可用，將使用 venv 執行任務 {task_id}，Docker 狀態: {getattr(node, 'docker_status', 'unknown')}")
        else:
            node._log(f"使用 Docker 執行任務 {task_id}")

        temp_dir = mkdtemp(prefix=f"task_{task_id}_")
        workspace = join(temp_dir, "workspace")
        makedirs(workspace)

        with ZipFile(BytesIO(task_zip_bytes), 'r') as zip_ref:
            zip_ref.extractall(workspace)
        node._log(f"Task {task_id} files extracted to {workspace}")

        if use_docker:
            container_name = f"task-{task_id}-{token_hex(4)}"
            mem_gb = max(required_resources.get('memory_gb', 1), 0.5)
            mem_limit = f"{int(mem_gb * 1024)}m"
            cpu_score = max(required_resources.get('cpu', 1), 1)
            cpu_limit = max(cpu_score / 100, 0.1)
            node._log(f"Docker 資源配置：memory={mem_limit}, cpu_limit={cpu_limit}")

            device_requests = []
            if required_resources.get('gpu', 0) > 0:
                device_requests.append({'Driver': 'nvidia', 'Count': -1, 'Capabilities': [['gpu']]})

            try:
                node._log(f"嘗試啟動 Docker 容器 {container_name}")
                if exists(workspace):
                    files_in_workspace = os.listdir(workspace)
                    print(f"[DEBUG] - 工作空間任務文件：{files_in_workspace}")

                container = node.docker_client.containers.run(
                    "justin308/hivemind-worker:latest",
                    command=["sh", "-c", """
                        echo "=== HiveMind Docker Task Execution ===" &&
                        echo "Task ID: ${TASK_ID:-unknown}" &&
                        echo "Working Directory: $(pwd)" &&
                        echo "Available files:" &&
                        ls -la &&
                        echo "=== Installing Requirements ===" &&
                        if [ -f requirements.txt ]; then
                            echo "Installing requirements..." &&
                            pip install --user -r requirements.txt || echo "Requirements installation failed"
                        else
                            echo "No requirements.txt found"
                        fi &&
                        echo "=== Finding and executing Python script ===" &&
                        SCRIPT_FILE="" &&
                        for script in main.py app.py run.py start.py; do
                            if [ -f "$script" ]; then
                                SCRIPT_FILE="$script"
                                echo "Found script: $SCRIPT_FILE"
                                break
                            fi
                        done &&
                        if [ -z "$SCRIPT_FILE" ]; then
                            SCRIPT_FILE=$(find . -name "*.py" -type f | head -n 1)
                            if [ -n "$SCRIPT_FILE" ]; then
                                echo "Using first Python file found: $SCRIPT_FILE"
                            else
                                echo "ERROR: No Python script found"
                                exit 1
                            fi
                        fi &&
                        echo "=== Executing $SCRIPT_FILE ===" &&
                        python "$SCRIPT_FILE" &&
                        echo "=== Task completed successfully ==="
                    """],
                    detach=True,
                    name=container_name,
                    volumes={workspace: {'bind': '/app/task', 'mode': 'rw'}},
                    working_dir="/app/task",
                    environment={"TASK_ID": task_id, "PYTHONUNBUFFERED": "1"},
                    mem_limit=mem_limit,
                    nano_cpus=int(cpu_limit * 1e9),
                    device_requests=device_requests if device_requests else None,
                    remove=False,
                )

                node._log(f"Task {task_id} Docker container started successfully with resources: CPU={cpu_limit}, Memory={mem_limit}")
                container.reload()
                try:
                    initial_logs = container.logs().decode('utf-8', errors='replace')
                    if initial_logs.strip():
                        print(f"[DEBUG] 容器初始日誌:\n{initial_logs}")
                except Exception:
                    pass

                # 記錄容器資訊於 running_tasks，供 resource_monitor 精準抓取單容器用量
                try:
                    with node.resources_lock:
                        if task_id not in node.running_tasks:
                            node.running_tasks[task_id] = {"status": "Executing", "resources": {}}
                        # 解析記憶體限制（例如 "1024m" -> 1024）為 MB
                        mem_limit_mb = None
                        try:
                            m = str(mem_limit).lower()
                            if m.endswith('m'):
                                mem_limit_mb = int(m[:-1])
                            elif m.endswith('g'):
                                mem_limit_mb = int(float(m[:-1]) * 1024)
                        except Exception:
                            mem_limit_mb = None
                        node.running_tasks[task_id].update({
                            "exec_mode": "docker",
                            "container_id": getattr(container, 'id', None),
                            "container_name": getattr(container, 'name', None),
                            "mem_limit_mb": mem_limit_mb,
                            "cpu_limit_nano": int(cpu_limit * 1e9) if cpu_limit else None,
                            "_usage_warmup": False,
                        })
                except Exception as e:
                    node._log(f"無法寫入任務容器資訊到 running_tasks: {e}")
            except Exception as docker_error:
                node._log(f"Docker 容器啟動失敗: {docker_error}")
                use_docker = False
                container = None

            if use_docker and container is not None:
                log_buffer: list[str] = []
                log_send_counter = 0
                last_log_fetch = time.time()

                while not stop_event.is_set():
                    try:
                        container.reload()
                        if container.status != 'running':
                            node._log(f"Container {container_name} stopped, status: {container.status}")
                            try:
                                exit_logs = container.logs().decode('utf-8', errors='replace')
                                if exit_logs.strip():
                                    for line in exit_logs.strip().split('\n'):
                                        if line.strip():
                                            node._log(f"[Task {task_id}]: {line}")
                                            task_logs.append(line)
                            except Exception:
                                pass
                            break

                        current_time = time.time()
                        if current_time - last_log_fetch > 1.0:
                            logs = container.logs(since=int(last_log_fetch)).decode('utf-8', errors='replace')
                            if logs.strip():
                                for line in logs.strip().split('\n'):
                                    if line.strip():
                                        node._log(f"[Task {task_id}]: {line}")
                                        log_buffer.append(line)
                                        task_logs.append(line)
                                        log_send_counter += 1
                                if log_send_counter >= 20 or len(log_buffer) > 0:
                                    node._send_task_logs(task_id, "\n".join(log_buffer))
                                    log_buffer.clear()
                                    log_send_counter = 0
                            last_log_fetch = current_time
                    except Exception as e:
                        node._log(f"Error monitoring container: {e}")
                        break
                    time.sleep(0.1)

                if stop_event.is_set():
                    stop_requested = True
                    stop_log = f"收到停止請求,立即終止任務 {task_id}"
                    node._log(stop_log)
                    task_logs.append(stop_log)
                    node._send_task_logs(task_id, stop_log)
                    try:
                        container.kill()
                    except Exception:
                        pass
                else:
                    try:
                        result = container.wait(timeout=2)
                        success = result.get('StatusCode', -1) == 0
                    except Exception:
                        success = False

        if not use_docker:
            worker_root = join(dirname(__file__), "..", "..")
            script_src = join(worker_root, "run_task.sh")
            script_dst = join(workspace, "run_task.sh")
            if exists(script_src):
                copy2(script_src, script_dst)
                chmod(script_dst, 0o755)

            import venv
            venv_dir = join(temp_dir, "venv")
            node._log(f"Creating virtual environment for task {task_id} at {venv_dir}")
            venv.create(venv_dir, with_pip=True)

            import subprocess
            process = None
            try:
                if os.name == 'nt':
                    venv_python = join(venv_dir, 'Scripts', 'python.exe')
                    venv_pip = join(venv_dir, 'Scripts', 'pip.exe')
                else:
                    venv_python = join(venv_dir, 'bin', 'python')
                    venv_pip = join(venv_dir, 'bin', 'pip')

                requirements_file = join(workspace, 'requirements.txt')
                if exists(requirements_file):
                    node._log(f"Installing requirements for task {task_id}")
                    pip_process = subprocess.Popen(
                        [venv_pip, 'install', '-r', 'requirements.txt'],
                        cwd=workspace,
                        stdout=subprocess.PIPE,
                        stderr=subprocess.STDOUT,
                        universal_newlines=True,
                    )
                    pip_output, _ = pip_process.communicate()
                    node._log(f"Pip install output: {pip_output}")

                script_path, cmd = _find_executable_script(node, workspace)
                if not script_path:
                    raise Exception("找不到可執行的腳本文件")
                if script_path.endswith('.py'):
                    cmd = [venv_python, script_path]

                process = subprocess.Popen(
                    cmd,
                    cwd=workspace,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.STDOUT,
                    universal_newlines=True,
                    bufsize=1,
                )

                log_buffer: list[str] = []
                log_send_counter = 0
                for line in iter(process.stdout.readline, ''):
                    if stop_event.is_set():
                        break
                    if line.strip():
                        node._log(f"[Task {task_id}]: {line.strip()}")
                        log_buffer.append(line.strip())
                        task_logs.append(line.strip())
                        log_send_counter += 1
                        if log_send_counter >= 20:
                            node._send_task_logs(task_id, "\n".join(log_buffer))
                            log_buffer.clear()
                            log_send_counter = 0

                process.wait(timeout=1)
                success = process.returncode == 0
            except subprocess.TimeoutExpired:
                node._log(f"Process timed out for task {task_id}")
                success = False

            if stop_event.is_set():
                stop_requested = True
                stop_log = f"收到停止請求，立即終止任務 {task_id}"
                node._log(stop_log)
                task_logs.append(stop_log)
                node._send_task_logs(task_id, stop_log)
                if process and process.poll() is None:
                    try:
                        process.terminate()
                        time.sleep(0.5)
                        if process.poll() is None:
                            process.kill()
                    except Exception:
                        pass

        if stop_requested:
            completion_log = f"任務 {task_id} 被用戶強制停止"
        else:
            completion_log = f"任務 {task_id} 執行完成，狀態: {'成功' if success else '失敗'}"
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
                    node._log(f"任務 {task_id} 結果已發送 (TaskResultUpload)")
                else:
                    node._log("worker_stub 未初始化，無法上傳結果")
            except Exception as e:
                node._log(f"發送任務結果失敗(TaskResultUpload): {e}")

    except Exception as e:
        error_log = f"任務 {task_id} 執行失敗: {e}"
        node._log(error_log)
        print(f"[ERROR] {error_log}")
        print(f"[DEBUG] 執行模式: {'Docker' if (node.docker_available and getattr(node, 'docker_client', None)) else 'venv'}")
        task_logs.append(error_log)
        node._send_task_logs(task_id, error_log)
        success = False

    finally:
        if container is not None:
            try:
                container.remove(force=True)
            except Exception:
                pass
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
