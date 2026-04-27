#!/usr/bin/env python3
"""測試 OpenMP 多執行緒是否真的在工作"""
import sys
import os
import time
sys.path.insert(0, '.')

# 設定 OpenMP 執行緒數
os.environ['OMP_NUM_THREADS'] = '12'

from main import compute_range_count_100percent

print("測試 OpenMP 執行緒數設定...")
print(f"OMP_NUM_THREADS={os.environ.get('OMP_NUM_THREADS', 'not set')}")
print("\n開始計算 1-10億 (應該看到 CPU 接近 100%)...")

t0 = time.time()
count, meta = compute_range_count_100percent(1, 1000000000, workers=12)
dt = time.time() - t0

print(f"\n完成！")
print(f"找到質數: {count:,}")
print(f"耗時: {dt:.2f}s")
print(f"速度: {(1000000000/dt)/1e6:.0f}M/s")
print(f"Meta: {meta}")
