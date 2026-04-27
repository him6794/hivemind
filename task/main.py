# client_100percent.py   ← 完全使用 C 加速，記憶體控制
import os
from multiprocessing import cpu_count

# 必須在任何其他 import 之前設定 OpenMP 執行緒數
_num_threads = cpu_count() or 12
os.environ['OMP_NUM_THREADS'] = str(_num_threads)
os.environ['OMP_DYNAMIC'] = 'false'  # 禁用動態調整
os.environ['OMP_PROC_BIND'] = 'close'  # 執行緒綁定到核心

import requests
import sys
import time
import json
import ctypes
import uuid
import threading
from pathlib import Path

SERVER_URL = "http://127.0.0.1:5001"
REPORT_WINDOW = 1000
COMPUTE_CHUNK = 50000
BATCH_WINDOWS = 100

# 強制載入 C 函式庫（.so/.dll with OpenMP）
_c_lib = None
try:
    # Windows 需要先加入 mingw64/bin 到 DLL 搜尋路徑
    if sys.platform.startswith('win'):
        # 讓 prime_sieve.dll 的相依 DLL（例如 libgomp-1.dll）可以從 task 目錄被找到
        try:
            os.add_dll_directory(str(Path(__file__).parent))
        except Exception:
            pass
        mingw_bin = Path('C:/msys64/mingw64/bin')
        if mingw_bin.exists():
            os.add_dll_directory(str(mingw_bin))
    
    lib_path = None
    for ext in ['.dll', '.so']:
        candidate = Path(__file__).parent / f'prime_sieve{ext}'
        if candidate.exists():
            lib_path = candidate
            break
    
    if not lib_path:
        print("✗ 找不到 prime_sieve.dll 或 prime_sieve.so")
        print("請執行: gcc -shared -O3 -march=native -fopenmp -o prime_sieve.dll prime_sieve.c -lm")
        sys.exit(1)
    
    _c_lib = ctypes.CDLL(str(lib_path))
    _c_lib.count_primes.argtypes = [ctypes.c_int64, ctypes.c_int64]
    _c_lib.count_primes.restype = ctypes.c_int64
    _c_lib.get_primes.argtypes = [ctypes.c_int64, ctypes.c_int64, ctypes.POINTER(ctypes.c_int)]
    _c_lib.get_primes.restype = ctypes.POINTER(ctypes.c_int)
    _c_lib.get_primes_parallel.argtypes = [ctypes.c_int64, ctypes.c_int64, ctypes.POINTER(ctypes.c_int), ctypes.c_int]
    _c_lib.get_primes_parallel.restype = ctypes.POINTER(ctypes.c_int)
    _c_lib.count_primes_max_parallel.argtypes = [ctypes.c_int64, ctypes.c_int64, ctypes.POINTER(ctypes.c_int64), ctypes.c_int]
    _c_lib.count_primes_max_parallel.restype = ctypes.c_int64
    _c_lib.free_primes.argtypes = [ctypes.POINTER(ctypes.c_int)]
    print(f"✓ 已載入 C 加速函式庫 ({lib_path.name})")
except Exception as e:
    print(f"✗ 無法載入 C 函式庫: {e}")
    print("本系統要求 C 加速，無法使用純 Python")
    sys.exit(1)

def segmented_sieve(start, end, num_threads=0):
    """使用 C 實作的分段篩法（強制，支援多執行緒）"""
    if start >= end:
        return []
    
    if start < 2:
        start = 2
    
    try:
        count = ctypes.c_int()
        # 優先使用多執行緒版本
        ptr = _c_lib.get_primes_parallel(start, end, ctypes.byref(count), num_threads)
        if ptr and count.value > 0:
            result = [ptr[i] for i in range(count.value)]
            _c_lib.free_primes(ptr)
            return result
        return []
    except Exception as e:
        print(f"✗ C 函式錯誤: {e}", flush=True)
        raise RuntimeError("C 函式庫執行失敗，無法繼續計算")


def count_primes_and_max(start: int, end: int, num_threads: int = 0) -> tuple[int, int]:
    """使用 C 端直接計算 count + max prime（不回傳 primes list）。"""
    if start >= end:
        return 0, 0
    if start < 2:
        start = 2

    max_p = ctypes.c_int64(0)
    try:
        c = _c_lib.count_primes_max_parallel(int(start), int(end), ctypes.byref(max_p), int(num_threads or 0))
        return int(c), int(max_p.value)
    except Exception as e:
        print(f"✗ C count_primes_max_parallel 錯誤: {e}", flush=True)
        raise


def _plan_work(start: int, end: int, workers: int | None) -> tuple[int, int, list[tuple[int, int]]]:
    if workers is None:
        workers = cpu_count() or 8

    # 計算用 chunk（降低避免記憶體累積）
    chunk_size = COMPUTE_CHUNK
    if chunk_size % REPORT_WINDOW != 0:
        chunk_size = (chunk_size // REPORT_WINDOW) * REPORT_WINDOW
        if chunk_size <= 0:
            chunk_size = REPORT_WINDOW
    
    # 記憶體保護：範圍太大時進一步降低 chunk
    range_size = end - start
    if range_size > 100000000:  # 超過 1 億
        chunk_size = min(chunk_size, 10000)

    tasks: list[tuple[int, int]] = []
    cur = start
    while cur < end:
        chunk_end = min(cur + chunk_size, end)
        tasks.append((cur, chunk_end))
        cur = chunk_end

    return workers, chunk_size, tasks


def compute_range_count_100percent(start: int, end: int, workers: int = None, progress_callback=None) -> tuple[int, dict]:
    """使用 C OpenMP 多執行緒的質數計算（完全並行，不切分 chunk）"""
    if workers is None:
        workers = cpu_count() or 8
    
    # 直接讓 C 處理整個範圍，內部用 OpenMP 並行化
    total_primes = 0
    processed_numbers = int(end - start)
    
    # 如果有 progress_callback，需要分段回報（否則一次算完沒進度）
    if progress_callback:
        # 重要：避免一次把超巨量 primes 拉回 Python 造成記憶體/序列化爆炸。
        report_chunk_size = 50_000_000  # 5000 萬一段（兼顧吞吐與記憶體）

        current = start
        segment_num = 0
        total_segments = (end - start + report_chunk_size - 1) // report_chunk_size

        while current < end:
            seg_end = min(current + report_chunk_size, end)
            print(f"[計算段] {current:,} ~ {seg_end:,} ({seg_end-current:,} 數字)", flush=True)
            primes_count, seg_max_prime = count_primes_and_max(current, seg_end, num_threads=workers)
            total_primes += int(primes_count)
            segment_num += 1
            
            # 回報進度（傳遞質數列表供 callback 處理）
            progress_percent = (segment_num / total_segments) * 100
            seg_processed = int(seg_end - start)
            progress_callback(current, seg_end, int(primes_count), int(seg_max_prime), seg_processed, processed_numbers, progress_percent)
            
            current = seg_end
    else:
        # 無需進度回報：一次性計算整個範圍（完全並行）
        primes_count, seg_max_prime = count_primes_and_max(start, end, num_threads=workers)
        total_primes = int(primes_count)
    
    meta = {
        "numbers_processed": processed_numbers,
        "workers_used": int(workers),
        "chunk_size": int(end - start),  # 單一大 chunk
        "subtasks": 1,  # C 內部並行，外層不切分
        "algo": "c_openmp_parallel",
        "inner_segment_max": 1000000,
        "cpu_count": int(cpu_count() or 0),
        "python": sys.version,
        "platform": sys.platform,
    }

    return total_primes, meta

def compute_range_100percent(start: int, end: int, workers: int = None, progress_callback=None) -> tuple[int, dict]:
    """向下相容：原本回傳 primes list，現在回傳 primes_count（避免 IPC 傳巨大資料）"""
    return compute_range_count_100percent(start, end, workers=workers, progress_callback=progress_callback)

def main():
    if len(sys.argv) > 1:
        global SERVER_URL
        SERVER_URL = sys.argv[1].rstrip("/")

    session = requests.Session()
    
    # Worker ID for tracking
    import socket
    import os
    # 讓同機器多 worker 也能區分：hostname + MAC + PID
    worker_id = f"{socket.gethostname()}-{uuid.getnode():012x}-{os.getpid()}"

    # ---- Node registration + heartbeat/load ----
    node_id: int | None = None
    _cpu_prev = {'idle': None, 'kernel': None, 'user': None}

    def _win_filetime_to_int(ft) -> int:
        return (int(ft.dwHighDateTime) << 32) | int(ft.dwLowDateTime)

    def _get_load_snapshot() -> dict:
        """Best-effort load snapshot.

        Windows: CPU% via GetSystemTimes delta, MEM% via GlobalMemoryStatusEx.
        Other OS: returns empty dict.
        """
        if not sys.platform.startswith('win'):
            return {'ts': time.time()}

        snap: dict = {'ts': time.time()}

        # CPU usage
        try:
            class FILETIME(ctypes.Structure):
                _fields_ = [
                    ('dwLowDateTime', ctypes.c_uint32),
                    ('dwHighDateTime', ctypes.c_uint32),
                ]

            idle = FILETIME()
            kernel = FILETIME()
            user = FILETIME()
            if ctypes.windll.kernel32.GetSystemTimes(ctypes.byref(idle), ctypes.byref(kernel), ctypes.byref(user)):
                idle_i = _win_filetime_to_int(idle)
                kernel_i = _win_filetime_to_int(kernel)
                user_i = _win_filetime_to_int(user)

                if _cpu_prev['idle'] is not None:
                    idle_delta = idle_i - int(_cpu_prev['idle'])
                    kernel_delta = kernel_i - int(_cpu_prev['kernel'])
                    user_delta = user_i - int(_cpu_prev['user'])
                    total = max(0, kernel_delta + user_delta)
                    busy = max(0, total - idle_delta)
                    cpu_pct = (busy / total * 100.0) if total > 0 else None
                    if cpu_pct is not None:
                        snap['cpu_percent'] = float(max(0.0, min(100.0, cpu_pct)))

                _cpu_prev['idle'] = idle_i
                _cpu_prev['kernel'] = kernel_i
                _cpu_prev['user'] = user_i
        except Exception:
            pass

        # Memory usage
        try:
            class MEMORYSTATUSEX(ctypes.Structure):
                _fields_ = [
                    ('dwLength', ctypes.c_uint32),
                    ('dwMemoryLoad', ctypes.c_uint32),
                    ('ullTotalPhys', ctypes.c_uint64),
                    ('ullAvailPhys', ctypes.c_uint64),
                    ('ullTotalPageFile', ctypes.c_uint64),
                    ('ullAvailPageFile', ctypes.c_uint64),
                    ('ullTotalVirtual', ctypes.c_uint64),
                    ('ullAvailVirtual', ctypes.c_uint64),
                    ('ullAvailExtendedVirtual', ctypes.c_uint64),
                ]

            ms = MEMORYSTATUSEX()
            ms.dwLength = ctypes.sizeof(MEMORYSTATUSEX)
            if ctypes.windll.kernel32.GlobalMemoryStatusEx(ctypes.byref(ms)):
                snap['mem_percent'] = float(ms.dwMemoryLoad)
                snap['mem_total_mb'] = float(ms.ullTotalPhys / 1024 / 1024)
                snap['mem_avail_mb'] = float(ms.ullAvailPhys / 1024 / 1024)
        except Exception:
            pass

        return snap

    def _register_node_forever() -> int:
        while True:
            try:
                resp = session.post(f"{SERVER_URL}/register_node", json={'worker_id': worker_id}, timeout=10)
                if resp.ok:
                    data = resp.json() or {}
                    nid = int(data.get('node_id'))
                    print(f"✓ 已註冊節點序號: {nid}", flush=True)
                    return nid
            except Exception:
                pass
            time.sleep(3)

    node_id = _register_node_forever()

    def _heartbeat_loop():
        nonlocal node_id
        while True:
            try:
                payload = {
                    'worker_id': worker_id,
                    'node_id': int(node_id) if node_id is not None else None,
                    'load': _get_load_snapshot(),
                }
                resp = session.post(f"{SERVER_URL}/node_heartbeat", json=payload, timeout=10)
                if resp.status_code == 409:
                    # server lost state or restarted; re-register
                    node_id = _register_node_forever()
            except Exception:
                pass
            time.sleep(5)

    threading.Thread(target=_heartbeat_loop, daemon=True).start()
    
    # 本地累積結果 - 改用範圍命名
    total_primes_found = 0
    current_task_progress = 0

    last_takeover_attempt = 0.0

    def _try_get_task(take_over: bool = False):
        params = {"worker_id": worker_id}
        if take_over:
            params["take_over"] = "1"
        return session.get(f"{SERVER_URL}/get_task", params=params, timeout=10)

    def _maybe_take_over_on_idle() -> bool:
        """當伺服器顯示任務仍在跑、但沒有 remaining_chunks 時，嘗試 take_over 接手。

        回傳 True 表示已嘗試 take_over（不代表一定成功拿到任務）。
        """
        nonlocal last_takeover_attempt
        now = time.time()
        # 節流：避免多 worker 每秒搶一次造成 ping-pong
        if now - last_takeover_attempt < 30:
            return False

        try:
            s = session.get(f"{SERVER_URL}/task_state", timeout=5)
            if not s.ok:
                return False
            try:
                state = s.json() or {}
            except Exception:
                # 伺服器可能回 HTML 錯誤頁而非 JSON
                return False

            if not state.get('initialized') or not state.get('running'):
                return False

            if int(state.get('remaining_chunks') or 0) > 0:
                return False

            if int(state.get('in_flight') or 0) <= 0:
                return False

            preview = state.get('in_flight_preview') or []
            # 預設：只要是單一 chunk 任務（最常見卡住場景），就直接接手
            total_chunks = int(state.get('total_chunks') or 0)
            should_take_over = (total_chunks == 1)

            # 若有 preview，且 assigned_at 太舊，也視為可接手
            try:
                if preview:
                    assigned_at = float(preview[0].get('assigned_at') or 0)
                    age = now - assigned_at if assigned_at else 0
                    if age > 120:
                        should_take_over = True
            except Exception:
                pass

            if not should_take_over:
                return False

            last_takeover_attempt = now
            print("偵測到任務卡在 in-flight，嘗試 take_over 接手...", flush=True)
            return True
        except Exception:
            return False

    while True:
        try:
            r = _try_get_task(take_over=False)
            if r.status_code == 204:
                # 可能是 chunks=1 的任務已被其他 worker 持有（in-flight），這裡自動嘗試接手
                if _maybe_take_over_on_idle():
                    r2 = _try_get_task(take_over=True)
                    if r2.status_code != 204:
                        r = r2
                    else:
                        # 沒接到就稍等再試
                        time.sleep(2)
                        continue
                else:
                    # 沒任務也不要退出：持續輪詢，等待隊列/節點數條件滿足
                    time.sleep(2)
                    continue

            task = r.json()

            task_id = task.get("task_id")
            start = int(task.get("start", 1))
            end = int(task.get("end", 100))
            print(f"拿到任務 → {start:,} ~ {end-1:,}  共 {end-start:,} 個數字")

            # 檔案名稱使用範圍
            local_results_file = f"{start}-{end}.json"
            
            # 進度回調函數
            batch_items = []

            def _post_json_with_retry(url: str, payload: dict, attempts: int = 5) -> bool:
                """可靠送出 JSON（短重試）。成功回 True，失敗回 False。"""
                backoff = 0.6
                for i in range(max(1, int(attempts))):
                    try:
                        resp = session.post(url, json=payload, timeout=30)
                        if resp.ok:
                            return True
                    except Exception:
                        pass
                    time.sleep(backoff)
                    backoff = min(backoff * 1.6, 5.0)
                return False

            def _flush_batch(done: bool = False, duration: float | None = None, meta: dict | None = None):
                nonlocal batch_items
                if not batch_items and not done:
                    return
                try:
                    # 重要：保持 window 有序，避免後端 cursor 因順序錯亂而跳過較早的 window
                    if batch_items:
                        batch_items.sort(key=lambda it: int(it.get('window_end') or 0))

                    payload = {
                        "worker_id": worker_id,
                        "node_id": int(node_id) if node_id is not None else None,
                        "load": _get_load_snapshot(),
                        "task_id": task_id,
                        "range_start": int(start),
                        "range_end": int(end),
                        "items": batch_items,
                        "done": bool(done),
                        "store_primes": False,
                        "store_progress": True,
                    }
                    if duration is not None:
                        payload["duration"] = float(duration)
                    if meta is not None:
                        payload["meta"] = meta
                    ok = _post_json_with_retry(f"{SERVER_URL}/report_progress_batch", payload, attempts=5)
                    if ok:
                        batch_items = []
                    else:
                        # 不清空：保留未送出的 window，避免漏報導致總數不一致
                        # done=true 失敗時也先保留，後續會在任務結尾重試。
                        return
                except Exception:
                    # 不清空：保留，後續重試
                    return

            def progress_callback(block_start, block_end, primes_count, max_prime, processed_so_far, total_numbers, percent):
                nonlocal current_task_progress, batch_items
                current_task_progress = percent

                try:
                    overall_percent = (processed_so_far / total_numbers) * 100 if total_numbers else percent
                except Exception:
                    overall_percent = percent

                mp = int(max_prime or 0)
                print(
                    f"  計算進度: {block_start:,} ~ {block_end:,} | {overall_percent:.1f}% | 質數數量: {int(primes_count):,} | 最大質數: {mp:,}",
                    flush=True,
                )

                # A 版本：只送該塊摘要（count + max），不送 primes list
                batch_items.append({
                    "window_start": int(block_start),
                    "window_end": int(block_end),
                    "numbers_processed_delta": int(block_end - block_start),
                    "primes": [],
                    "primes_count_delta": int(primes_count),
                    "max_prime": int(mp),
                })

                # 這裡每段都 flush 一次，確保 heartbeat/進度可見，且 payload 很小不會拖垮 CPU
                print(f"  發送進度批次: {len(batch_items)} 項", flush=True)
                _flush_batch(done=False)

            t0 = time.perf_counter()
            primes_count, compute_meta = compute_range_100percent(start, end, progress_callback=progress_callback)
            duration = time.perf_counter() - t0

            total_primes_found += primes_count
            
            print(f"\n完成！找到質數 {primes_count:,} 個，耗時 {duration:.3f}s (累計: {total_primes_found:,})")

            # 依需求：其他不存（不落本地檔）

            # 任務完成通知（讓伺服器把這個 chunk 算作完成）
            try:
                # flush 剩餘 batch，再送 done=true
                _flush_batch(done=False)
                # done=true 若送失敗，這裡阻塞重試直到成功，確保伺服器不會卡在 in-flight
                while True:
                    before = len(batch_items)
                    _flush_batch(done=True, duration=float(duration), meta=compute_meta)
                    if len(batch_items) == 0:
                        break
                    # 若一直失敗，稍等再重試
                    time.sleep(2.0)
                print("✓ 已通知伺服器：此任務完成")
            except Exception as e:
                print(f"通知伺服器完成失敗: {e}")

        except Exception as e:
            print(f"錯誤：{e}　5 秒後重試")
            time.sleep(5)

    print(f"總共找到: {total_primes_found:,} 個質數")

if __name__ == "__main__":
    # Windows 需要這行避免遞迴生成子進程爆炸
    if sys.platform.startswith("win"):
        import multiprocessing as mp
        mp.freeze_support()
        mp.set_start_method('spawn', force=True)
    main()