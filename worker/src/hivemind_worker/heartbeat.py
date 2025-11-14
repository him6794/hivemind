"""Status heartbeat loop for WorkerNode.

Expose run_status_reporter(node, stop_event) that encapsulates the original loop.
"""
from __future__ import annotations

import time
from logging import WARNING

try:
    from .resource_monitor import report_all_tasks_resource_usage
except Exception:
    from resource_monitor import report_all_tasks_resource_usage  # type: ignore

try:
    from . import nodepool_pb2
except Exception:
    import nodepool_pb2


def run_status_reporter(node, stop_event) -> None:
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
                        node._refresh_registration()
                        node._last_heartbeat_time = current_time
                    except Exception as e:
                        node._log(f"Heartbeat registration failed: {e}", WARNING)
                node._update_balance()

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

                    status_str = getattr(node, 'status', 'Unknown')

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

                    if getattr(node, 'node_stub', None):
                        try:
                            node.node_stub.ReportStatus(req, timeout=5)
                        except Exception as e:
                            node._log(f"ReportStatus RPC failed: {e}", WARNING)

                    # 另外用 TaskUsageRequest (task_id = "0") 回報節點層級資源使用
                    if getattr(node, 'worker_stub', None):
                        try:
                            tu_req = nodepool_pb2.TaskUsageRequest(
                                task_id="0",  # 特殊：0 代表節點狀態
                                cpu_usage=float(f"{cpu_pct:.4f}"),
                                memory_usage=float(f"{mem_pct:.4f}"),
                                gpu_usage=float(f"{gpu_pct:.4f}"),
                                gpu_memory_usage=0.0,
                                token=(node.username or '')  # 這裡攜帶 node_id 以便伺服端識別
                            )
                            node.worker_stub.TaskUsage(tu_req, timeout=5)
                        except Exception as e:
                            node._log(f"TaskUsage(0) RPC failed: {e}", WARNING)
                except Exception as e:
                    node._log(f"Failed to prepare/send running status: {e}", WARNING)
            except Exception as e:
                node._log(f"Status monitoring failed: {e}", WARNING)
    stop_event.wait(1)  # 每秒更新一次
