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
                report_all_tasks_resource_usage(node)
            except Exception as e:
                node._log(f"Status monitoring failed: {e}", WARNING)
        stop_event.wait(5)
