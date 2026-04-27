"""Resource allocation helpers for WorkerNode."""
from __future__ import annotations


def check_resources_available(node, required_resources: dict) -> bool:
    with node.resources_lock:
        for rtype, required in required_resources.items():
            if rtype in node.available_resources and node.available_resources[rtype] < required:
                return False
        return True


def allocate_resources(node, task_id: str, required_resources: dict) -> bool:
    with node.resources_lock:
        for rtype, required in required_resources.items():
            if rtype in node.available_resources and node.available_resources[rtype] < required:
                return False
        for rtype, required in required_resources.items():
            if rtype in node.available_resources:
                node.available_resources[rtype] -= required
        node.running_tasks[task_id] = {
            "status": "Allocated",
            "resources": required_resources,
            "start_time": __import__('time').time(),
        }
        node.task_locks[task_id] = node.Lock() if hasattr(node, 'Lock') else __import__('threading').Lock()
        node.task_stop_events[task_id] = node.Event() if hasattr(node, 'Event') else __import__('threading').Event()
        return True


def release_resources(node, task_id: str) -> None:
    with node.resources_lock:
        if task_id not in node.running_tasks:
            return
        task_resources = node.running_tasks[task_id].get('resources', {})
        for rtype, amount in task_resources.items():
            if rtype in node.available_resources:
                node.available_resources[rtype] += amount
        del node.running_tasks[task_id]
        if task_id in node.task_locks:
            del node.task_locks[task_id]
        if task_id in node.task_stop_events:
            del node.task_stop_events[task_id]
