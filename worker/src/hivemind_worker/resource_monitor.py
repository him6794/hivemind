"""Resource usage reporting helpers for WorkerNode.

- report_task_resource_usage(node, task_id)
- report_all_tasks_resource_usage(node)
"""
from __future__ import annotations

from logging import WARNING
import time
from typing import Any

try:
    from . import nodepool_pb2
except Exception:
    import nodepool_pb2

# Optional Go-based metrics wrappers
try:
    from .system_metrics import USING_GO_PSUTIL, go_process_cpu_percent, go_process_rss_bytes, virtual_memory as _vm_wrap  # type: ignore
except Exception:
    USING_GO_PSUTIL = False  # type: ignore
    def go_process_cpu_percent(pid: int) -> float:  # type: ignore
        return 0.0
    def go_process_rss_bytes(pid: int) -> int:  # type: ignore
        return 0
    def _vm_wrap():  # type: ignore
        return None


def report_all_tasks_resource_usage(node) -> None:
    """Iterate all running tasks and report their resource usage."""
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
    """Report per-task resource usage.

    Priority:
    - Docker container stats when exec_mode == 'docker'
    - Process stats when exec_mode == 'process'
    - Fallback to node-level metrics if neither available
    """
    if not node.token:
        return
    try:
        with node.resources_lock:
            if task_id not in node.running_tasks:
                return
            task_data = node.running_tasks[task_id]
            resources = task_data.get('resources', {})
            exec_mode = task_data.get('exec_mode')
            container_id = task_data.get('container_id')
            pid = task_data.get('pid')
            warmup_done = task_data.get('_usage_warmup', False)

        current_cpu_percent = 0.0
        mem_percent = 0.0
        gpu_usage_percent = 0
        gpu_memory_used_mb = 0

        # Docker container mode (manual delta calculation to avoid stream=False precpu_stats issues)
        if exec_mode == 'docker' and getattr(node, 'docker_available', False) and getattr(node, 'docker_client', None) and container_id:
            try:
                c = node.docker_client.containers.get(container_id)  # type: ignore[attr-defined]
                stats: dict[str, Any] = c.stats(stream=False)

                cpu_stats = stats.get('cpu_stats', {}) or {}
                cpu_usage_stats = cpu_stats.get('cpu_usage', {}) or {}
                cpu_total = int(cpu_usage_stats.get('total_usage', 0) or 0)
                system_cpu = int(cpu_stats.get('system_cpu_usage', 0) or 0)
                online_cpus = int(cpu_stats.get('online_cpus') or len(cpu_usage_stats.get('percpu_usage', []) or []) or 1)

                # Retrieve last snapshot
                last_cpu_total = task_data.get('last_cpu_total')
                last_system_cpu = task_data.get('last_system_cpu')

                cpu_pct = 0.0
                if last_cpu_total is not None and last_system_cpu is not None and system_cpu > last_system_cpu:
                    cpu_delta = max(0, cpu_total - last_cpu_total)
                    system_delta = max(1, system_cpu - last_system_cpu)
                    cpu_pct = (cpu_delta / system_delta) * online_cpus * 100.0

                # Memory: prefer (usage - cache) per Docker docs
                mem_stats = stats.get('memory_stats', {}) or {}
                raw_usage = float(mem_stats.get('usage', 0.0) or 0.0)
                cache_val = 0.0
                try:
                    cache_val = float((mem_stats.get('stats', {}) or {}).get('cache', 0.0) or 0.0)
                except Exception:
                    cache_val = 0.0
                effective_usage = max(0.0, raw_usage - cache_val)
                mem_limit = float(mem_stats.get('limit', 1.0) or 1.0)
                mem_pct = (effective_usage / mem_limit) * 100.0 if mem_limit > 0 else 0.0
                mem_used_mb = int(round(effective_usage / (1024 * 1024)))
                mem_limit_mb = int(round(mem_limit / (1024 * 1024)))

                # Update snapshot & warm-up flag
                with node.resources_lock:
                    node.running_tasks[task_id]['last_cpu_total'] = cpu_total
                    node.running_tasks[task_id]['last_system_cpu'] = system_cpu
                    node.running_tasks[task_id]['mem_used_mb'] = mem_used_mb
                    node.running_tasks[task_id]['mem_limit_mb'] = mem_limit_mb
                    if not warmup_done:
                        node.running_tasks[task_id]['_usage_warmup'] = True

                if warmup_done:
                    current_cpu_percent = cpu_pct
                    mem_percent = mem_pct
            except Exception as e:
                node._log(f"Docker stats 取得失敗（task {task_id}）: {e}", WARNING)

        # Process mode
        elif exec_mode == 'process' and pid:
            try:
                # Prefer Go DLL if available to minimize Python low-level API usage
                if USING_GO_PSUTIL:
                    if not warmup_done:
                        _ = go_process_cpu_percent(int(pid))
                        with node.resources_lock:
                            node.running_tasks[task_id]['_usage_warmup'] = True
                    else:
                        current_cpu_percent = float(go_process_cpu_percent(int(pid)))
                    rss_bytes = float(go_process_rss_bytes(int(pid)))
                    vm2 = _vm_wrap()
                    total_mem = float(getattr(vm2, 'total', 0.0)) if vm2 else 0.0
                    mem_percent = float((rss_bytes / total_mem) * 100.0) if total_mem > 0 else 0.0
                else:
                    # Windows-only implementation using WinAPI
                    import os
                    import ctypes
                    from ctypes import wintypes
                    # Open process
                    PROCESS_QUERY_LIMITED_INFORMATION = 0x1000
                    PROCESS_QUERY_INFORMATION = 0x0400
                    PROCESS_VM_READ = 0x0010
                    access = PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_QUERY_INFORMATION | PROCESS_VM_READ
                    kernel32 = ctypes.WinDLL('kernel32', use_last_error=True)
                    psapi = ctypes.WinDLL('psapi', use_last_error=True)

                    OpenProcess = kernel32.OpenProcess
                    OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
                    OpenProcess.restype = wintypes.HANDLE

                    GetProcessTimes = kernel32.GetProcessTimes
                    GetProcessTimes.argtypes = [wintypes.HANDLE,
                                                 ctypes.POINTER(wintypes.FILETIME), ctypes.POINTER(wintypes.FILETIME),
                                                 ctypes.POINTER(wintypes.FILETIME), ctypes.POINTER(wintypes.FILETIME)]
                    GetProcessTimes.restype = wintypes.BOOL

                    class PROCESS_MEMORY_COUNTERS(ctypes.Structure):
                        _fields_ = [
                            ('cb', wintypes.DWORD),
                            ('PageFaultCount', wintypes.DWORD),
                            ('PeakWorkingSetSize', wintypes.SIZE_T),
                            ('WorkingSetSize', wintypes.SIZE_T),
                            ('QuotaPeakPagedPoolUsage', wintypes.SIZE_T),
                            ('QuotaPagedPoolUsage', wintypes.SIZE_T),
                            ('QuotaPeakNonPagedPoolUsage', wintypes.SIZE_T),
                            ('QuotaNonPagedPoolUsage', wintypes.SIZE_T),
                            ('PagefileUsage', wintypes.SIZE_T),
                            ('PeakPagefileUsage', wintypes.SIZE_T),
                        ]

                    GetProcessMemoryInfo = psapi.GetProcessMemoryInfo
                    GetProcessMemoryInfo.argtypes = [wintypes.HANDLE, ctypes.POINTER(PROCESS_MEMORY_COUNTERS), wintypes.DWORD]
                    GetProcessMemoryInfo.restype = wintypes.BOOL

                    CloseHandle = kernel32.CloseHandle
                    CloseHandle.argtypes = [wintypes.HANDLE]
                    CloseHandle.restype = wintypes.BOOL

                h = OpenProcess(access, False, int(pid))
                if not h:
                    raise OSError('OpenProcess failed')
                try:
                    # CPU percent: compute delta of (kernel+user) time over wall time and CPU cores
                    creation = wintypes.FILETIME()
                    exit = wintypes.FILETIME()
                    kernel = wintypes.FILETIME()
                    user = wintypes.FILETIME()
                    if not GetProcessTimes(h, ctypes.byref(creation), ctypes.byref(exit), ctypes.byref(kernel), ctypes.byref(user)):
                        raise OSError('GetProcessTimes failed')

                    def _filetime_to_seconds(ft: wintypes.FILETIME) -> float:
                        # FILETIME in 100-ns units
                        return (ft.dwHighDateTime << 32 | ft.dwLowDateTime) / 10_000_000.0

                    proc_time = _filetime_to_seconds(kernel) + _filetime_to_seconds(user)

                    now_ts = time.time()
                    last_proc_time = task_data.get('last_proc_time')
                    last_wall = task_data.get('last_wall_time')
                    cpu_cores = max(1, getattr(node, 'cpu_cores', os.cpu_count() or 1))
                    if not warmup_done or last_proc_time is None or last_wall is None:
                        with node.resources_lock:
                            node.running_tasks[task_id]['_usage_warmup'] = True
                            node.running_tasks[task_id]['last_proc_time'] = proc_time
                            node.running_tasks[task_id]['last_wall_time'] = now_ts
                    else:
                        dt = max(1e-3, now_ts - float(last_wall))
                        dproc = max(0.0, proc_time - float(last_proc_time))
                        current_cpu_percent = float(min(100.0, max(0.0, (dproc / (dt * cpu_cores)) * 100.0)))
                        with node.resources_lock:
                            node.running_tasks[task_id]['last_proc_time'] = proc_time
                            node.running_tasks[task_id]['last_wall_time'] = now_ts

                    # Memory percent using WorkingSetSize (RSS)
                    pmc = PROCESS_MEMORY_COUNTERS()
                    pmc.cb = ctypes.sizeof(PROCESS_MEMORY_COUNTERS)
                    if GetProcessMemoryInfo(h, ctypes.byref(pmc), pmc.cb):
                        rss = float(pmc.WorkingSetSize)
                        try:
                            vm2 = node.virtual_memory()
                            total_mem = float(getattr(vm2, 'total', 0.0))
                        except Exception:
                            total_mem = 0.0
                        mem_percent = float((rss / total_mem) * 100.0) if total_mem > 0 else 0.0
                finally:
                    CloseHandle(h)
            except Exception as e:
                node._log(f"Process stats 取得失敗（task {task_id}）: {e}", WARNING)

        # Fallback to node-level (not ideal)
        else:
            try:
                current_cpu_percent = float(node.cpu_percent())
            except Exception:
                current_cpu_percent = 0.0
            try:
                vm2 = node.virtual_memory()
                mem_percent = float(getattr(vm2, 'percent', 0.0))
            except Exception:
                mem_percent = 0.0

        # Save latest usage snapshot for local UI/API
        try:
            with node.resources_lock:
                if task_id in node.running_tasks:
                    usage = node.running_tasks[task_id].setdefault('usage', {})
                    usage.update(
                        {
                            'cpu_percent': round(float(current_cpu_percent), 2),
                            'memory_percent': round(float(mem_percent), 2),
                            'gpu_percent': round(float(gpu_usage_percent), 2),
                            'gpu_memory_mb': round(float(gpu_memory_used_mb), 2),
                            'updated_at': time.time(),
                        }
                    )
                    mem_used_mb_dbg = node.running_tasks[task_id].get('mem_used_mb')
                    mem_limit_mb_dbg = node.running_tasks[task_id].get('mem_limit_mb')
                    if mem_used_mb_dbg is not None:
                        usage['memory_used_mb'] = int(mem_used_mb_dbg)
                    if mem_limit_mb_dbg is not None:
                        usage['memory_limit_mb'] = int(mem_limit_mb_dbg)
        except Exception:
            pass

        # Log and send
        try:
            # Compose extended debug info
            extra_mem = ''
            try:
                mem_used_mb_dbg = task_data.get('mem_used_mb')
                mem_limit_mb_dbg = task_data.get('mem_limit_mb')
                if mem_used_mb_dbg is not None and mem_limit_mb_dbg is not None:
                    extra_mem = f" (mem_used={mem_used_mb_dbg}MB / {mem_limit_mb_dbg}MB)"
            except Exception:
                pass
            node._log(
                f"Task {task_id} resource usage: CPU={current_cpu_percent:.2f}%, MEM%={mem_percent:.2f}%{extra_mem}, "
                f"GPU={gpu_usage_percent}%, GPU_MEM={gpu_memory_used_mb}MB"
            )
            usage_info = (f"{{\"cpu_percent\":{current_cpu_percent:.2f},\"memory_percent\":{mem_percent:.2f},"
                          f"\"gpu_percent\":{gpu_usage_percent},\"gpu_memory_mb\":{gpu_memory_used_mb}}}")
            node._log(f"Local resource usage for task {task_id}: {usage_info}")

            if getattr(node, 'worker_stub', None):
                try:
                    # 直接使用浮點百分比（proto 已改為 float）
                    tu_req = nodepool_pb2.TaskUsageRequest(
                        task_id=str(task_id),
                        cpu_usage=float(f"{current_cpu_percent:.4f}"),
                        memory_usage=float(f"{max(0.0, min(100.0, mem_percent)):.4f}"),
                        gpu_usage=float(f"{gpu_usage_percent:.4f}"),
                        gpu_memory_usage=float(f"{gpu_memory_used_mb:.2f}"),
                        token=(node.username or ''),
                    )
                    metadata = [('authorization', f'Bearer {node.token}')] if node.token else []
                    node.worker_stub.TaskUsage(tu_req, metadata=metadata, timeout=3)
                except Exception as e:
                    node._log(f"TaskUsage RPC failed for task {task_id}: {e}", WARNING)
        except Exception as e:
            node._log(f"Resource monitoring failed: {e}", WARNING)
    except Exception as e:
        node._log(f"回報任務 {task_id} 資源使用失敗: {e}", WARNING)
