"""
HiveMind Performance Calculator V3
使用 C 共享庫進行實際性能測試
"""

import os
import sys
import ctypes
import logging
from pathlib import Path
from typing import Dict, Tuple, Optional


def get_benchmark_lib() -> Optional[ctypes.CDLL]:
    """加載基準測試共享庫"""
    # 開發環境
    dev_lib = Path(__file__).parent.parent.parent / "benchmark" / "benchmark.dll"
    if dev_lib.exists():
        logging.info(f"Found benchmark library: {dev_lib}")
        return ctypes.CDLL(str(dev_lib))
    
    # Linux .so
    dev_lib_so = Path(__file__).parent.parent.parent / "benchmark" / "benchmark.so"
    if dev_lib_so.exists():
        logging.info(f"Found benchmark library: {dev_lib_so}")
        return ctypes.CDLL(str(dev_lib_so))
    
    # 編譯後環境
    if getattr(sys, 'frozen', False):
        exe_dir = Path(sys.executable).parent
        for lib_name in ["benchmark.dll", "benchmark.so", "libbenchmark.so"]:
            lib_path = exe_dir / lib_name
            if lib_path.exists():
                logging.info(f"Found benchmark library: {lib_path}")
                return ctypes.CDLL(str(lib_path))
    
    logging.error("Benchmark library not found")
    return None


def run_cpu_benchmark(quick_mode: bool = True) -> float:
    """
    運行 CPU 基準測試
    
    Args:
        quick_mode: True=快速模式(1億次), False=完整模式(10億次)
    
    Returns:
        CPU GOPS (每秒十億次操作)
    """
    try:
        lib = get_benchmark_lib()
        if not lib:
            logging.error("Cannot load benchmark library")
            return 0.0
        
        # 設置函數原型
        # double benchmark_cpu(int quick_mode)
        lib.benchmark_cpu.argtypes = [ctypes.c_int]
        lib.benchmark_cpu.restype = ctypes.c_double
        
        # 調用函數
        gops = lib.benchmark_cpu(1 if quick_mode else 0)
        
        logging.info(f"CPU Benchmark: {gops:.2f} GOPS")
        return gops
        
    except Exception as e:
        logging.error(f"CPU benchmark error: {e}")
        return 0.0


def run_gpu_benchmark() -> Tuple[float, str, float]:
    """
    運行 GPU 基準測試
    
    Returns:
        (gpu_gops, gpu_name, gpu_memory_gb)
    """
    try:
        lib = get_benchmark_lib()
        if not lib:
            logging.warning("Cannot load benchmark library")
            return 0.0, "Not Detected", 0.0
        
        # 設置函數原型
        # double benchmark_gpu(char* gpu_name, double* gpu_memory_gb)
        lib.benchmark_gpu.argtypes = [ctypes.c_char_p, ctypes.POINTER(ctypes.c_double)]
        lib.benchmark_gpu.restype = ctypes.c_double
        
        # 準備輸出緩衝區
        gpu_name_buf = ctypes.create_string_buffer(256)
        gpu_memory = ctypes.c_double()
        
        # 調用函數
        gops = lib.benchmark_gpu(gpu_name_buf, ctypes.byref(gpu_memory))
        
        gpu_name = gpu_name_buf.value.decode('utf-8', errors='ignore')
        memory_gb = gpu_memory.value
        
        logging.info(f"GPU Benchmark: {gpu_name} = {gops:.2f} GOPS, {memory_gb:.2f} GB")
        return gops, gpu_name, memory_gb
        
    except Exception as e:
        logging.error(f"GPU benchmark error: {e}")
        return 0.0, "Not Detected", 0.0


def calculate_performance(quick_cpu: bool = True) -> Dict[str, any]:
    """
    計算完整性能資訊
    
    Args:
        quick_cpu: 是否使用快速 CPU 測試模式
    
    Returns:
        {
            'cpu_tops': int,          # CPU GOPS
            'gpu_tops': int,          # GPU GOPS
            'total_tops': int,        # 總計 GOPS
            'gpu_name': str,          # GPU 名稱
            'gpu_memory_gb': float    # GPU 顯存 GB
        }
    """
    logging.info("Starting performance calculation with C library...")
    
    # CPU 測試
    cpu_gops = run_cpu_benchmark(quick_mode=quick_cpu)
    
    # GPU 測試
    gpu_gops, gpu_name, gpu_memory = run_gpu_benchmark()
    
    # 計算總計
    total_gops = cpu_gops + gpu_gops
    
    result = {
        'cpu_tops': int(cpu_gops),
        'gpu_tops': int(gpu_gops),
        'total_tops': int(total_gops),
        'gpu_name': gpu_name,
        'gpu_memory_gb': gpu_memory
    }
    
    logging.info(f"Performance calculation complete: CPU={result['cpu_tops']}, "
                f"GPU={result['gpu_tops']}, Total={result['total_tops']}")
    
    return result


def calculate_cpu_tops(quick_mode: bool = True) -> int:
    """計算 CPU 性能（GOPS）"""
    return int(run_cpu_benchmark(quick_mode))


def calculate_gpu_tops() -> int:
    """計算 GPU 性能（GOPS）"""
    gops, _, _ = run_gpu_benchmark()
    return int(gops)


if __name__ == "__main__":
    # 配置日誌
    logging.basicConfig(
        level=logging.INFO,
        format='%(asctime)s - %(levelname)s - %(message)s'
    )
    
    print("=" * 70)
    print("HiveMind Performance Calculator V3 (C Library)")
    print("=" * 70)
    print()
    
    # 檢查庫是否可用
    print("[1] Loading benchmark library...")
    lib = get_benchmark_lib()
    if lib:
        print(f"    ✓ Library loaded successfully")
    else:
        print(f"    ✗ Library not found")
        print("    Please run: powershell -ExecutionPolicy Bypass -File benchmark/compile_lib.ps1")
        sys.exit(1)
    print()
    
    # 運行性能測試
    print("[2] Running performance tests...")
    perf = calculate_performance(quick_cpu=True)
    print()
    
    # 顯示結果
    print("[3] Results:")
    print(f"    CPU GOPS:      {perf['cpu_tops']:>10,}")
    print(f"    GPU GOPS:      {perf['gpu_tops']:>10,}")
    print(f"    Total GOPS:    {perf['total_tops']:>10,}")
    print(f"    GPU Name:      {perf['gpu_name']}")
    print(f"    GPU Memory:    {perf['gpu_memory_gb']:.2f} GB")
    print()
