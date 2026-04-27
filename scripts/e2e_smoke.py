"""
HiveMind End-to-End Smoke Test (No Docker)
===========================================
測試整套無 Docker 流程：NodePool gRPC -> Worker (pydantic-monty) -> 結果回收

使用方式：
  python scripts/e2e_smoke.py \
      --nodepool localhost:50051 \
      --user testuser \
      --password testpass \
      [--task task/hello_monty.py]

前置條件：
  1. Redis 已啟動（預設 localhost:6379）
  2. NodePool gRPC 已啟動（預設 localhost:50051）
  3. Worker 已啟動並連線至 NodePool
  4. NodePool 已有對應使用者帳號（見下方「建立測試帳號」說明）

建立測試帳號（首次執行前）：
  cd node_pool
  python -c "
  import sys; sys.path.insert(0, '.')
  from user_manager import UserManager
  m = UserManager()
  ok, msg, _ = m.register_user('testuser', 'testpass')
  print(ok, msg)
  "
"""
from __future__ import annotations

import argparse
import io
import json
import os
import sys
import time
import zipfile
from pathlib import Path

# ── 確保能 import nodepool_pb2 ──────────────────────────────────────────────
REPO_ROOT = Path(__file__).resolve().parents[1]
NODE_POOL_DIR = REPO_ROOT / "node_pool"
sys.path.insert(0, str(NODE_POOL_DIR))

try:
    import grpc
    import nodepool_pb2
    import nodepool_pb2_grpc
except ImportError as e:
    sys.exit(f"[ERROR] 缺少依賴: {e}\n  請執行: pip install grpcio")


# ── 工具函數 ────────────────────────────────────────────────────────────────

def make_task_zip(script_path: Path) -> bytes:
    """把單一 Python 腳本打包成 main.py 的 ZIP。"""
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w", zipfile.ZIP_DEFLATED) as z:
        z.writestr("main.py", script_path.read_text(encoding="utf-8"))
    return buf.getvalue()


def poll_task_status(stub, task_id: str, token: str, timeout: int = 120) -> str:
    """輪詢任務狀態直到完成或逾時。回傳最終 status 字串。"""
    deadline = time.time() + timeout
    last_status = "UNKNOWN"
    while time.time() < deadline:
        resp = stub.GetAllUserTasks(
            nodepool_pb2.GetAllUserTasksRequest(token=token)
        )
        for t in resp.tasks:
            if t.task_id == task_id:
                last_status = t.status
                print(f"  [poll] task={task_id}  status={last_status}")
                if last_status in ("COMPLETED", "FAILED", "STOPPED", "ERROR"):
                    return last_status
                break
        time.sleep(3)
    print(f"  [poll] 逾時 ({timeout}s)，最後 status={last_status}")
    return last_status


def fetch_and_print_result(stub, task_id: str, token: str):
    """下載結果 ZIP 並印出 output.txt 內容。"""
    resp = stub.GetTaskResult(
        nodepool_pb2.GetTaskResultRequest(task_id=task_id, token=token)
    )
    if not resp.success:
        print(f"  [result] GetTaskResult 失敗: {resp.message}")
        return
    if not resp.result_zip:
        print("  [result] result_zip 為空")
        return
    with zipfile.ZipFile(io.BytesIO(resp.result_zip)) as z:
        names = z.namelist()
        print(f"  [result] ZIP 內容: {names}")
        for name in names:
            print(f"\n  ── {name} ──")
            try:
                print(z.read(name).decode("utf-8", errors="replace"))
            except Exception:
                print("  (binary)")


# ── 主流程 ──────────────────────────────────────────────────────────────────

def main():
    parser = argparse.ArgumentParser(description="HiveMind E2E smoke test (no Docker)")
    parser.add_argument("--nodepool", default="localhost:50051", help="NodePool gRPC 地址")
    parser.add_argument("--user",     default="testuser",        help="NodePool 使用者名稱")
    parser.add_argument("--password", default="testpass",        help="NodePool 密碼")
    parser.add_argument("--task",     default=str(REPO_ROOT / "task" / "hello_monty.py"),
                        help="要上傳的 Python 腳本（會被打包成 main.py）")
    parser.add_argument("--timeout",  type=int, default=120,     help="等待完成的秒數")
    args = parser.parse_args()

    task_path = Path(args.task)
    if not task_path.exists():
        sys.exit(f"[ERROR] 找不到任務腳本: {task_path}")

    print(f"\n{'='*60}")
    print(f"  HiveMind E2E Smoke Test")
    print(f"  NodePool : {args.nodepool}")
    print(f"  User     : {args.user}")
    print(f"  Task     : {task_path.name}")
    print(f"{'='*60}\n")

    # ── 1. 連線 NodePool ─────────────────────────────────────────────────────
    channel = grpc.insecure_channel(args.nodepool)
    user_stub    = nodepool_pb2_grpc.UserServiceStub(channel)
    master_stub  = nodepool_pb2_grpc.MasterNodeServiceStub(channel)

    # ── 2. 登入 ──────────────────────────────────────────────────────────────
    print("[step 1] 登入 NodePool...")
    login_resp = user_stub.Login(
        nodepool_pb2.LoginRequest(username=args.user, password=args.password)
    )
    if not login_resp.success:
        sys.exit(f"[FAIL] 登入失敗: {login_resp.message}")
    token = login_resp.token
    print(f"  [ok] 登入成功，token 取得（前 20 字）: {token[:20]}...")

    # ── 3. 上傳任務 ──────────────────────────────────────────────────────────
    task_id = f"smoke-{int(time.time())}"
    task_zip = make_task_zip(task_path)
    print(f"\n[step 2] 上傳任務 (task_id={task_id}, zip={len(task_zip)} bytes)...")
    upload_resp = master_stub.UploadTask(
        nodepool_pb2.UploadTaskRequest(
            task_id=task_id,
            task_zip=task_zip,
            memory_gb=1,
            cpu_score=1,
            gpu_score=0,
            gpu_memory_gb=0,
            token=token,
            host_count=1,
        )
    )
    if not upload_resp.success:
        sys.exit(f"[FAIL] 上傳失敗: {upload_resp.message}")
    print(f"  [ok] 上傳成功: {upload_resp.message}")

    # ── 4. 輪詢狀態 ──────────────────────────────────────────────────────────
    print(f"\n[step 3] 等待任務完成（最多 {args.timeout}s）...")
    final_status = poll_task_status(master_stub, task_id, token, args.timeout)

    # ── 5. 取回結果 ──────────────────────────────────────────────────────────
    print(f"\n[step 4] 下載結果 (status={final_status})...")
    fetch_and_print_result(master_stub, task_id, token)

    # ── 6. 結論 ───────────────────────────────────────────────────────────────
    print(f"\n{'='*60}")
    if final_status == "COMPLETED":
        print("  ✓ PASS — 任務成功完成（無 Docker 模式）")
    else:
        print(f"  ✗ FAIL — 最終狀態: {final_status}")
    print(f"{'='*60}\n")

    channel.close()
    sys.exit(0 if final_status == "COMPLETED" else 1)


if __name__ == "__main__":
    main()
