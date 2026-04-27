"""System metrics and optional Go-based psutil replacement.

This module encapsulates CPU/memory/GPU info retrieval with an optional
Go DLL acceleration. Falls back to Windows API / stdlib when unavailable.
"""

from __future__ import annotations

import os
import ctypes
import time
import socket
from collections import namedtuple
from ctypes import c_int, c_double, c_ulonglong, c_char_p

# Memory info namedtuple compatible with psutil
VirtualMemory = namedtuple('VirtualMemory', ['total', 'available', 'percent', 'used', 'free'])

# Try to load Go DLL for better performance
try:
    dll_path = os.path.join(os.path.dirname(__file__), "psutil.dll")
    if os.path.exists(dll_path):
        _go_lib = ctypes.CDLL(dll_path)
        # Setup function signatures
        _go_lib.get_cpu_count.restype = c_int
        _go_lib.get_cpu_count.argtypes = []
        _go_lib.get_cpu_percent.restype = c_double
        _go_lib.get_cpu_percent.argtypes = []
        _go_lib.get_memory_total.restype = c_ulonglong
        _go_lib.get_memory_total.argtypes = []
        _go_lib.get_memory_available.restype = c_ulonglong
        _go_lib.get_memory_available.argtypes = []
        _go_lib.get_memory_percent.restype = c_double
        _go_lib.get_memory_percent.argtypes = []
        _go_lib.get_hostname_length.restype = c_int
        _go_lib.get_hostname_length.argtypes = []
        _go_lib.get_hostname_data.restype = c_int
        _go_lib.get_hostname_data.argtypes = [c_char_p, c_int]
        _go_lib.get_cpu_score.restype = c_int
        _go_lib.get_cpu_score.argtypes = []
        _go_lib.get_gpu_name_length.restype = c_int
        _go_lib.get_gpu_name_length.argtypes = []
        _go_lib.get_gpu_name_data.restype = c_int
        _go_lib.get_gpu_name_data.argtypes = [c_char_p, c_int]
        _go_lib.get_gpu_memory.restype = c_double
        _go_lib.get_gpu_memory.argtypes = []
        _go_lib.get_gpu_score.restype = c_int
        _go_lib.get_gpu_score.argtypes = []
        _go_lib.get_gpu_available.restype = c_int
        _go_lib.get_gpu_available.argtypes = []
        # Process-level exports
        _go_lib.get_process_cpu_percent.restype = c_double
        _go_lib.get_process_cpu_percent.argtypes = [c_int]
        _go_lib.get_process_rss_bytes.restype = c_ulonglong
        _go_lib.get_process_rss_bytes.argtypes = [c_int]
        USING_GO_PSUTIL = True
        print("[Worker] Using Go-based metrics DLL for better performance")
    else:
        _go_lib = None
        USING_GO_PSUTIL = False
        print(f"[Worker] Go DLL not found at {dll_path}, using Windows API / stdlib fallback")
except Exception as e:
    _go_lib = None
    USING_GO_PSUTIL = False
    print(f"[Worker] Failed to load Go DLL: {e}, using Windows API / stdlib fallback")


# ---------------------
# Windows API fallbacks
# ---------------------
_is_windows = os.name == 'nt'

# MEMORYSTATUSEX for GlobalMemoryStatusEx
class MEMORYSTATUSEX(ctypes.Structure):
    _fields_ = [
        ("dwLength", ctypes.c_ulong),
        ("dwMemoryLoad", ctypes.c_ulong),
        ("ullTotalPhys", ctypes.c_ulonglong),
        ("ullAvailPhys", ctypes.c_ulonglong),
        ("ullTotalPageFile", ctypes.c_ulonglong),
        ("ullAvailPageFile", ctypes.c_ulonglong),
        ("ullTotalVirtual", ctypes.c_ulonglong),
        ("ullAvailVirtual", ctypes.c_ulonglong),
        ("ullAvailExtendedVirtual", ctypes.c_ulonglong),
    ]

if _is_windows:
    _kernel32 = ctypes.WinDLL('kernel32', use_last_error=True)
    _GetSystemTimes = _kernel32.GetSystemTimes
    _GetSystemTimes.argtypes = [ctypes.POINTER(ctypes.c_uint64), ctypes.POINTER(ctypes.c_uint64), ctypes.POINTER(ctypes.c_uint64)]
    _GetSystemTimes.restype = ctypes.c_bool

    _GlobalMemoryStatusEx = _kernel32.GlobalMemoryStatusEx
    _GlobalMemoryStatusEx.argtypes = [ctypes.POINTER(MEMORYSTATUSEX)]
    _GlobalMemoryStatusEx.restype = ctypes.c_bool

# Helpers to convert FILETIME to int64 for GetSystemTimes equivalents
def _get_system_times():
    if not _is_windows:
        return None
    idle_time = ctypes.c_uint64(0)
    kernel_time = ctypes.c_uint64(0)
    user_time = ctypes.c_uint64(0)
    ok = _GetSystemTimes(ctypes.byref(idle_time), ctypes.byref(kernel_time), ctypes.byref(user_time))
    if not ok:
        return None
    return idle_time.value, kernel_time.value, user_time.value

_last_cpu_times = None  # type: tuple[int,int,int] | None
_last_cpu_check = 0.0


def cpu_count(logical: bool = True) -> int:
    """Get CPU count using Go library or stdlib."""
    if USING_GO_PSUTIL:
        return _go_lib.get_cpu_count()
    # Python stdlib logical cpu count
    cnt = os.cpu_count() or 1
    return cnt if logical else max(1, cnt // 2)


def virtual_memory() -> VirtualMemory:
    """Get memory info using Go library or OS API (Windows)."""
    if USING_GO_PSUTIL:
        total = _go_lib.get_memory_total()
        available = _go_lib.get_memory_available()
        percent = _go_lib.get_memory_percent()
        used = total - available
        free = available
        return VirtualMemory(total=total, available=available, percent=percent, used=used, free=free)
    if _is_windows:
        msex = MEMORYSTATUSEX()
        msex.dwLength = ctypes.sizeof(MEMORYSTATUSEX)
        if not _GlobalMemoryStatusEx(ctypes.byref(msex)):
            # Fallback if API fails
            total = available = used = free = 0
            percent = 0.0
        else:
            total = int(msex.ullTotalPhys)
            available = int(msex.ullAvailPhys)
            used = total - available
            free = available
            percent = float(msex.dwMemoryLoad)
        return VirtualMemory(total=total, available=available, percent=percent, used=used, free=free)
    # Non-Windows very simple fallback using mmap size not available; return zeros
    return VirtualMemory(total=0, available=0, percent=0.0, used=0, free=0)


def cpu_percent() -> float:
    """Get CPU usage percentage using Go DLL or Windows GetSystemTimes delta.

    Returns a percentage in [0,100]. On non-Windows without Go DLL, returns 0.
    """
    global _last_cpu_times, _last_cpu_check
    if USING_GO_PSUTIL:
        try:
            return float(_go_lib.get_cpu_percent())
        except Exception:
            pass
    if not _is_windows:
        return 0.0

    now = time.time()
    times = _get_system_times()
    if times is None:
        return 0.0
    idle, kernel, user = times

    if _last_cpu_times is None:
        _last_cpu_times = (idle, kernel, user)
        _last_cpu_check = now
        time.sleep(0.1)
        times2 = _get_system_times()
        if times2 is None:
            return 0.0
        idle2, kernel2, user2 = times2
        di = max(0, idle2 - idle)
        dk = max(0, kernel2 - kernel)
        du = max(0, user2 - user)
        busy = max(0, (dk + du) - di)
        total = max(1, dk + du)
        pct = (busy / total) * 100.0
        _last_cpu_times = (idle2, kernel2, user2)
        _last_cpu_check = time.time()
        return float(max(0.0, min(100.0, pct)))

    idle0, kernel0, user0 = _last_cpu_times
    di = max(0, idle - idle0)
    dk = max(0, kernel - kernel0)
    du = max(0, user - user0)
    busy = max(0, (dk + du) - di)
    total = max(1, dk + du)
    pct = (busy / total) * 100.0
    _last_cpu_times = (idle, kernel, user)
    _last_cpu_check = now
    return float(max(0.0, min(100.0, pct)))


# Optional wrappers for process-level metrics via Go DLL
def go_process_cpu_percent(pid: int) -> float:
    if USING_GO_PSUTIL:
        try:
            return float(_go_lib.get_process_cpu_percent(int(pid)))
        except Exception:
            return 0.0
    return 0.0


def go_process_rss_bytes(pid: int) -> int:
    if USING_GO_PSUTIL:
        try:
            return int(_go_lib.get_process_rss_bytes(int(pid)))
        except Exception:
            return 0
    return 0


def get_hostname() -> str:
    """Get hostname using Go library or fallback"""
    if USING_GO_PSUTIL:
        length = _go_lib.get_hostname_length()
        if length <= 0:
            return "unknown"
        buffer = ctypes.create_string_buffer(length + 1)
        _go_lib.get_hostname_data(buffer, length + 1)
        return buffer.value.decode('utf-8')
    else:
        return socket.gethostname()


def get_cpu_score() -> int:
    """Get CPU benchmark score using Go library or fallback"""
    if USING_GO_PSUTIL:
        return _go_lib.get_cpu_score()
    else:
        start_time = time.time()
        result = 0
        for i in range(100000):
            result += i % 1000
        duration = time.time() - start_time
        return int((100000 / duration) / 10) if duration > 0.01 else 1000


def get_gpu_info() -> dict:
    """Get GPU info using Go library or fallback"""
    if USING_GO_PSUTIL:
        name_length = _go_lib.get_gpu_name_length()
        if name_length <= 0:
            name = "Not Detected"
        else:
            buffer = ctypes.create_string_buffer(name_length + 1)
            _go_lib.get_gpu_name_data(buffer, name_length + 1)
            name = buffer.value.decode('utf-8')

        memory = _go_lib.get_gpu_memory()
        score = _go_lib.get_gpu_score()
        available = _go_lib.get_gpu_available() == 1

        return {
            'name': name,
            'memory_gb': memory,
            'score': score,
            'available': available
        }
    else:
        return {
            'name': 'Not Detected (Python fallback)',
            'memory_gb': 0.0,
            'score': 0,
            'available': False
        }
