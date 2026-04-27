"""
最終性能測試 - V3 (C 共享庫)
"""

import sys
import os

sys.path.insert(0, os.path.join(os.path.dirname(__file__), 'src', 'hivemind_worker'))

from performance_calculator_v3 import calculate_performance, run_cpu_benchmark, run_gpu_benchmark

print("=" * 70)
print("HiveMind 最終效能測試 (C 共享庫)")
print("=" * 70)
print()

print("[1] CPU 效能測試 (實際 C 基準測試)")
print("-" * 70)
cpu_gops = run_cpu_benchmark(quick_mode=True)
print(f"✓ CPU GOPS: {cpu_gops:.2f}")
print()

print("[2] GPU 效能測試 (nvidia-smi/wmic 查詢)")
print("-" * 70)
gpu_gops, gpu_name, gpu_memory = run_gpu_benchmark()
print(f"✓ GPU 名稱: {gpu_name}")
print(f"✓ GPU 顯存: {gpu_memory:.2f} GB")
print(f"✓ GPU GOPS: {gpu_gops:,.2f}")
print()

print("[3] 完整效能資訊")
print("-" * 70)
perf = calculate_performance(quick_cpu=True)
print(f"✓ CPU GOPS:      {perf['cpu_tops']:>10,}")
print(f"✓ GPU GOPS:      {perf['gpu_tops']:>10,}")
print(f"✓ 總計 GOPS:     {perf['total_tops']:>10,}")
print()

print("[4] 版本對比")
print("-" * 70)
print("V1 (理論計算)：")
print("  - CPU: ~921 GOPS (基於架構 + 頻率)")
print("  - GPU: ~9,200 GOPS (基於 TFLOPS × 4 × 1000)")
print()
print("V2 (實際測試 + Go 估算)：")
print("  - CPU: ~3.5 GOPS (C 基準測試)")
print("  - GPU: ~55,345 GOPS (Go 估算 + GPU 名稱匹配)")
print()
print("V3 (C 共享庫)：")
print(f"  - CPU: ~{perf['cpu_tops']} GOPS (C 基準測試)")
print(f"  - GPU: ~{perf['gpu_tops']:,} GOPS (nvidia-smi 實際時鐘)")
print()

print("[5] 技術架構")
print("-" * 70)
print("✓ CPU 測試: C 語言實現 (整數、浮點、混合運算)")
print("✓ GPU 查詢: nvidia-smi (實際時鐘頻率 + 顯存)")
print("✓ 共享庫: benchmark.dll (Windows) / benchmark.so (Linux)")
print("✓ Python 調用: ctypes (零依賴)")
print("✓ 編譯優化: GCC -O3 -march=native")
print()

print("=" * 70)
print("測試完成！")
print("=" * 70)
