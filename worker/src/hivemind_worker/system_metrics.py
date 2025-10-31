"""System metrics and optional Go-based psutil replacement.

This module encapsulates CPU/memory/GPU info retrieval with an optional
Go DLL acceleration. Falls back to Python psutil when unavailable.
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
        USING_GO_PSUTIL = True
        print("[Worker] Using Go-based psutil replacement for better performance")
    else:
        _go_lib = None
        USING_GO_PSUTIL = False
        print(f"[Worker] Go DLL not found at {dll_path}, falling back to Python psutil")
except Exception as e:
    _go_lib = None
    USING_GO_PSUTIL = False
    print(f"[Worker] Failed to load Go DLL: {e}, falling back to Python psutil")


def cpu_count(logical: bool = True) -> int:
    """Get CPU count using Go library or fallback to Python"""
    if USING_GO_PSUTIL:
        return _go_lib.get_cpu_count()
    else:
        from psutil import cpu_count as psutil_cpu_count  # type: ignore
        return psutil_cpu_count(logical=logical)


def virtual_memory() -> VirtualMemory:
    """Get memory info using Go library or fallback to Python"""
    if USING_GO_PSUTIL:
        total = _go_lib.get_memory_total()
        available = _go_lib.get_memory_available()
        percent = _go_lib.get_memory_percent()
        used = total - available
        free = available
        return VirtualMemory(total=total, available=available, percent=percent, used=used, free=free)
    else:
        from psutil import virtual_memory as psutil_virtual_memory  # type: ignore
        return psutil_virtual_memory()


def cpu_percent() -> float:
    """Get CPU usage percentage - fallback to Python psutil with proper interval handling"""
    from psutil import cpu_percent as psutil_cpu_percent  # type: ignore
    try:
        usage = psutil_cpu_percent(interval=0.1)
        return usage if usage > 0 else 0.1  # 避免返回 0
    except Exception:
        return 5.0


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
