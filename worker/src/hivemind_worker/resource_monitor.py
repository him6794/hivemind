"""Resource usage reporting helpers for WorkerNode.

- report_task_resource_usage(node, task_id)
- report_all_tasks_resource_usage(node)
"""
from __future__ import annotations

from logging import WARNING


def report_all_tasks_resource_usage(node) -> None:
    if not node.running_tasks:
        return
    try:
        with node.resources_lock:
            task_ids = list(node.running_tasks.keys())
        for task_id in task_ids:
            try:
                report_task_resource_usage(node, task_id)
            except Exception as e:
                node._log(f"回報任務 {task_id} 資源使用失敗: {e}", WARNING)
    except Exception as e:
        node._log(f"回報任務資源使用發生錯誤: {e}", WARNING)


def report_task_resource_usage(node, task_id: str) -> None:
    if not node.token:
        return
    try:
        with node.resources_lock:
            if task_id not in node.running_tasks:
                return
            task_data = node.running_tasks[task_id]
            resources = task_data.get('resources', {})
        try:
            current_cpu_percent = node.cpu_percent()
            memory_info = node.virtual_memory()
            memory_used_mb = int((memory_info.total - memory_info.available) / (1024 * 1024))
            gpu_usage_percent = 0
            gpu_memory_used_mb = 0
            if resources.get('gpu', 0) > 0:
                gpu_usage_percent = 0
                gpu_memory_used_mb = 0
        except Exception as e:
            node._log(f"獲取系統資源使用失敗: {e}", WARNING)
            current_cpu_percent = 50
            memory_used_mb = int(resources.get('memory_gb', 1) * 512)
            gpu_usage_percent = 0
            gpu_memory_used_mb = 0
        try:
            node._log(
                f"Task {task_id} resource usage: CPU={current_cpu_percent:.1f}%, "
                f"MEM={memory_used_mb}MB, GPU={gpu_usage_percent}%, GPU_MEM={gpu_memory_used_mb}MB"
            )
            usage_info = (
                f"{{\"cpu_percent\":{current_cpu_percent:.1f},\"memory_mb\":{memory_used_mb},"
                f"\"gpu_percent\":{gpu_usage_percent},\"gpu_memory_mb\":{gpu_memory_used_mb}}}"
            )
            node._log(f"Local resource usage for task {task_id}: {usage_info}")
        except Exception as e:
            node._log(f"Resource monitoring failed: {e}", WARNING)
    except Exception as e:
        node._log(f"回報任務 {task_id} 資源使用失敗: {e}", WARNING)
