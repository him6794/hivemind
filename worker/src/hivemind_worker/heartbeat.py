"""Status heartbeat loop for WorkerNode.

Expose run_status_reporter(node, stop_event) that encapsulates the original loop.
"""
from __future__ import annotations

import time
from logging import WARNING
import traceback

# Throttling intervals (seconds)
# Adjust these to control how often certain RPCs are called.
BALANCE_UPDATE_INTERVAL = 60       # How often to refresh user balance
STATUS_PUSH_INTERVAL = 10          # How often to push node status (ReportStatus)
TASK_USAGE_INTERVAL = 15           # How often to send TaskUsage for task_id="0"

# Significant change thresholds to trigger immediate status push
CPU_DELTA_THRESHOLD = 5.0          # in percentage points
MEM_DELTA_THRESHOLD = 5.0          # in percentage points

try:
    from .resource_monitor import report_all_tasks_resource_usage
except Exception:
    from resource_monitor import report_all_tasks_resource_usage  # type: ignore

try:
    from . import nodepool_pb2
except Exception:
    import nodepool_pb2


def run_status_reporter(node, stop_event) -> None:
    # Local state for throttling
    last_balance_ts = 0.0
    last_status_push_ts = 0.0
    last_task_usage_ts = 0.0
    last_cpu = None  # type: float | None
    last_mem = None  # type: float | None

    while not stop_event.is_set():
        if node.is_registered and node.token:
            try:
                with node.resources_lock:
                    task_count = len(node.running_tasks)
                status_msg = f"Running {task_count} tasks" if task_count > 0 else node.status
                if not hasattr(node, '_last_reported_status') or node._last_reported_status != status_msg:
                    node._log(f"Local status: {status_msg}")
                    node._last_reported_status = status_msg
                current_time = time.time()
                if not hasattr(node, '_last_heartbeat_time') or current_time - node._last_heartbeat_time >= 30:
                    try:
                        # log RPC target info when available to aid debugging
                        try:
                            target_info = getattr(node, 'nodepool_address', None) or getattr(node, 'master_address', None) or 'unknown'
                        except Exception:
                            target_info = 'unknown'
                        node._log(f"Refreshing registration -> target: {target_info}")
                        node._refresh_registration()
                        node._last_heartbeat_time = current_time
                    except Exception as e:
                        node._log(f"Heartbeat registration failed: {e}", WARNING)
                        node._log(traceback.format_exc(), WARNING)
                # Throttled balance updates
                if current_time - last_balance_ts >= BALANCE_UPDATE_INTERVAL:
                    try:
                        node._update_balance()
                    except Exception as e:
                        node._log(f"Balance update failed: {e}", WARNING)
                    last_balance_ts = current_time

                # 回報所有任務資源使用（本地）
                report_all_tasks_resource_usage(node)

                # 向 node_pool 回報節點整體狀態（CPU/MEM/任務數）
                try:
                    # 取得 CPU 與記憶體百分比（node 提供的方法或屬性）
                    try:
                        cpu_pct = float(node.cpu_percent())
                    except Exception:
                        cpu_pct = 0.0
                    try:
                        vm = node.virtual_memory()
                        mem_pct = float(getattr(vm, 'percent', 0.0))
                    except Exception:
                        mem_pct = 0.0

                    gpu_pct = float(getattr(node, 'gpu_usage_percent', 0.0)) if hasattr(node, 'gpu_usage_percent') else 0.0

                    with node.resources_lock:
                        running_tasks_count = len(node.running_tasks)

                    # 根據運行中的任務數量決定狀態
                    base_status = getattr(node, 'status', 'Unknown')
                    if running_tasks_count > 0:
                        status_str = f"Running {running_tasks_count} tasks"
                    else:
                        status_str = base_status

                    # Decide whether to push status now (interval or significant change)
                    cpu_changed = (last_cpu is None) or abs(cpu_pct - last_cpu) >= CPU_DELTA_THRESHOLD
                    mem_changed = (last_mem is None) or abs(mem_pct - last_mem) >= MEM_DELTA_THRESHOLD
                    if cpu_changed or mem_changed or (current_time - last_status_push_ts >= STATUS_PUSH_INTERVAL):
                        req = nodepool_pb2.RunningStatusRequest(
                            node_id=node.username or '',
                            status=status_str,
                            cpu_usage=float(f"{cpu_pct:.4f}"),
                            memory_usage=float(f"{mem_pct:.4f}"),
                            gpu_usage=float(f"{gpu_pct:.4f}"),
                            gpu_memory_usage=0.0,
                            docker_status=node.docker_status if hasattr(node, 'docker_status') else 'unknown',
                            running_tasks=running_tasks_count
                        )

                        # Report status via node_stub; log details if stub missing or RPC fails
                        if not getattr(node, 'node_stub', None):
                            node._log("ReportStatus skipped: node.node_stub is missing", WARNING)
                        else:
                            try:
                                metadata = [('authorization', f'Bearer {node.token}')] if node.token else []
                                node.node_stub.ReportStatus(req, metadata=metadata, timeout=5)
                            except Exception as e:
                                node._log(f"ReportStatus RPC failed: {e}", WARNING)
                                try:
                                    node._log(traceback.format_exc(), WARNING)
                                except Exception:
                                    pass

                        last_status_push_ts = current_time
                        last_cpu = cpu_pct
                        last_mem = mem_pct

                    # 另外用 TaskUsageRequest (task_id = "0") 回報節點層級資源使用
                    if current_time - last_task_usage_ts >= TASK_USAGE_INTERVAL:
                        if not getattr(node, 'worker_stub', None):
                            node._log("TaskUsage(0) skipped: node.worker_stub is missing", WARNING)
                        else:
                            try:
                                tu_req = nodepool_pb2.TaskUsageRequest(
                                    task_id="0",  # 特殊：0 代表節點狀態
                                    cpu_usage=float(f"{cpu_pct:.4f}"),
                                    memory_usage=float(f"{mem_pct:.4f}"),
                                    gpu_usage=float(f"{gpu_pct:.4f}"),
                                    gpu_memory_usage=0.0,
                                    token=(node.username or '')  # 這裡攜帶 node_id 以便伺服端識別
                                )
                                metadata = [('authorization', f'Bearer {node.token}')] if node.token else []
                                node.worker_stub.TaskUsage(tu_req, metadata=metadata, timeout=5)
                            except Exception as e:
                                node._log(f"TaskUsage(0) RPC failed: {e}", WARNING)
                                try:
                                    node._log(traceback.format_exc(), WARNING)
                                except Exception:
                                    pass
                        last_task_usage_ts = current_time
                except Exception as e:
                    node._log(f"Failed to prepare/send running status: {e}", WARNING)
            except Exception as e:
                node._log(f"Status monitoring failed: {e}", WARNING)
        # Wait a bit between iterations to avoid busy loop even when not registered
        stop_event.wait(1)  # 每秒更新一次
