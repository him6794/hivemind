"""
HiveMind 本機測試初始化腳本（無 Docker）
=========================================
功能：
  1. 建立 NodePool 測試使用者
  2. 設定 worker_credentials.json（可選）
  3. 驗證 Redis 連線
  4. 驗證 NodePool gRPC 可連線

使用方式（一次性執行）：
  python scripts/init_local_test.py [--nodepool localhost:50051]
"""
from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
NODE_POOL_DIR = REPO_ROOT / "node_pool"
WORKER_CRED = REPO_ROOT / "worker" / "src" / "hivemind_worker" / "worker_credentials.json"

TEST_USER = "testuser"
TEST_PASS = "testpass"
WORKER_PORT = 50101
DEFAULT_NODEPOOL = "localhost:50051"


# ── 1. 建立測試使用者 ────────────────────────────────────────────────────────

def create_test_user():
    print(f"\n[1] 建立 NodePool 測試使用者 ({TEST_USER})...")
    sys.path.insert(0, str(NODE_POOL_DIR))
    try:
        from user_manager import UserManager  # type: ignore
    except ImportError as e:
        print(f"  [SKIP] 無法 import UserManager: {e}")
        return
    mgr = UserManager()
    result = mgr.register_user(TEST_USER, TEST_PASS)
    ok, msg = result[0], result[1]
    if ok:
        print(f"  [ok] 建立成功：{TEST_USER} / {TEST_PASS}")
    elif "已存在" in msg or "already" in msg.lower() or "exist" in msg.lower():
        print(f"  [ok] 使用者已存在，跳過")
    else:
        print(f"  [warn] {msg}")


# ── 2. 設定 worker_credentials.json ─────────────────────────────────────────

def setup_worker_credentials(nodepool_addr: str):
    print(f"\n[2] 設定 worker_credentials.json...")
    existing = {}
    if WORKER_CRED.exists():
        try:
            with open(WORKER_CRED, encoding="utf-8") as f:
                existing = json.load(f)
        except Exception:
            pass

    # 只更新 node_port / nodepool_address，不覆蓋已有的 username/password
    existing["node_port"] = existing.get("node_port", WORKER_PORT)
    existing["nodepool_address"] = nodepool_addr

    WORKER_CRED.parent.mkdir(parents=True, exist_ok=True)
    with open(WORKER_CRED, "w", encoding="utf-8") as f:
        json.dump(existing, f, indent=2, ensure_ascii=False)
    print(f"  [ok] 寫入 {WORKER_CRED}")
    print(f"       node_port        = {existing['node_port']}")
    print(f"       nodepool_address = {existing['nodepool_address']}")
    print(f"  [note] Worker 的帳密請透過 Worker UI (http://127.0.0.1:<FLASK_PORT>/login) 登入後儲存")


# ── 3. 驗證 Redis ────────────────────────────────────────────────────────────

def check_redis():
    print("\n[3] 驗證 Redis 連線...")
    try:
        import redis  # type: ignore
        r = redis.Redis(host="localhost", port=6379, socket_connect_timeout=3)
        r.ping()
        print("  [ok] Redis 連線正常 (localhost:6379)")
    except ImportError:
        print("  [skip] redis 套件未安裝")
    except Exception as e:
        print(f"  [FAIL] Redis 無法連線: {e}")
        print("         請確認 Redis 已啟動：redis-server 或 docker run redis")


# ── 4. 驗證 NodePool gRPC ────────────────────────────────────────────────────

def check_nodepool(addr: str):
    print(f"\n[4] 驗證 NodePool gRPC ({addr})...")
    sys.path.insert(0, str(NODE_POOL_DIR))
    try:
        import grpc  # type: ignore
        import nodepool_pb2  # type: ignore
        import nodepool_pb2_grpc  # type: ignore
    except ImportError as e:
        print(f"  [skip] grpc 套件未安裝: {e}")
        return

    try:
        channel = grpc.insecure_channel(addr)
        stub = nodepool_pb2_grpc.UserServiceStub(channel)
        resp = stub.Login(
            nodepool_pb2.LoginRequest(username=TEST_USER, password=TEST_PASS),
            timeout=5,
        )
        if resp.success:
            print(f"  [ok] NodePool 可連線，登入測試成功")
        else:
            print(f"  [warn] NodePool 可連線，但登入回傳: {resp.message}")
        channel.close()
    except Exception as e:
        print(f"  [FAIL] NodePool 無法連線: {e}")
        print(f"         請確認 NodePool 已啟動：cd node_pool && python node_pool_server.py")


# ── 5. 印出後續步驟 ──────────────────────────────────────────────────────────

def print_next_steps(nodepool_addr: str):
    task_storage = os.environ.get("TASK_STORAGE_PATH", "")
    if not task_storage:
        win_default = r"C:\hivemind_storage"
        task_storage = win_default
    print(f"""
{'='*60}
  後續步驟：

  A. 確認 TASK_STORAGE_PATH 環境變數（NodePool 存儲目錄）
     Windows 範例：
       $env:TASK_STORAGE_PATH = "C:\\\\hivemind_storage"

  B. 啟動服務（按此順序）：

     # Terminal 1 - NodePool
     $env:TASK_STORAGE_PATH = "C:\\\\hivemind_storage"
     cd node_pool
     python node_pool_server.py

     # Terminal 2 - Worker（首次需透過 Web UI 登入）
     cd worker
     python -m hivemind_worker
     # 打開 http://127.0.0.1:<FLASK_PORT> 輸入 {TEST_USER} / {TEST_PASS}

  C. 執行 E2E 煙霧測試：
     python scripts/e2e_smoke.py \\
         --nodepool {nodepool_addr} \\
         --user {TEST_USER} \\
         --password {TEST_PASS}
{'='*60}
""")


# ── 主入口 ───────────────────────────────────────────────────────────────────

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--nodepool", default=DEFAULT_NODEPOOL)
    parser.add_argument("--skip-user", action="store_true", help="跳過建立使用者步驟")
    args = parser.parse_args()

    create_test_user() if not args.skip_user else print("\n[1] 跳過建立使用者")
    setup_worker_credentials(args.nodepool)
    check_redis()
    check_nodepool(args.nodepool)
    print_next_steps(args.nodepool)


if __name__ == "__main__":
    main()
