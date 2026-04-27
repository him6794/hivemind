"""
HiveMind 平台端對端整合測試（pytest）
=====================================
執行方式：
  pytest test_e2e_platform.py -v
  pytest test_e2e_platform.py -v --host 192.168.1.10 --port 50051

啟動順序（測試前請確認）：
  1. Redis
  2. python node_pool/node_pool_server.py
  3. python worker/src/hivemind_worker/__main__.py（至少一個 worker）
"""

from __future__ import annotations

import io
import os
import re
import sys
import time
import zipfile
import sqlite3

import bcrypt
import grpc
import pytest

# ── nodepool_pb2 路徑 ──────────────────────────────────────────────────────
NODE_POOL_DIR = os.path.join(os.path.dirname(__file__), 'node_pool')
if NODE_POOL_DIR not in sys.path:
    sys.path.insert(0, NODE_POOL_DIR)

import nodepool_pb2
import nodepool_pb2_grpc

# ── 預設參數 ──────────────────────────────────────────────────────────────
DEFAULT_HOST  = 'localhost'
DEFAULT_PORT  = 50051
TEST_USER     = 'e2e_test_user'
TEST_PASSWORD = 'e2e_test_pass_2026'
TEST_BALANCE  = 99999
POLL_INTERVAL = 2
POLL_TIMEOUT  = 300

# 任務腳本：純 Python、無 import，pydantic-monty 可直接執行
TASK_SCRIPT = '''\
def is_prime(n):
    if n < 2:
        return False
    if n == 2:
        return True
    if n % 2 == 0:
        return False
    i = 3
    while i * i <= n:
        if n % i == 0:
            return False
        i += 2
    return True

primes = [n for n in range(2, 200) if is_prime(n)]
print(f"[TEST] 200 以内共 {len(primes)} 個質數")
print(f"[TEST] 前十個質數: {primes[:10]}")
print(f"[TEST] is_prime(197) = {is_prime(197)}")
print(f"[TEST] is_prime(198) = {is_prime(198)}")
print("[TEST] 質數驗證任務完成")
'''


# ── pytest CLI 參數 ────────────────────────────────────────────────────────
def pytest_addoption(parser):
    parser.addoption('--host', default=DEFAULT_HOST, help='NodePool gRPC 位址')
    parser.addoption('--port', type=int, default=DEFAULT_PORT, help='NodePool gRPC 埠')


# ── 輔助函式 ──────────────────────────────────────────────────────────────
def _get_db_path() -> str:
    path = os.environ.get('DB_PATH', os.path.join(NODE_POOL_DIR, 'users.db'))
    return os.path.normpath(path)


def _make_task_zip() -> bytes:
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, 'w', zipfile.ZIP_DEFLATED) as zf:
        zf.writestr('main.py', TASK_SCRIPT)
    return buf.getvalue()


# ── session 共用狀態（各 test 透過此物件傳遞 task_id / status）──────────────
class _SharedState:
    task_id: str | None = None
    final_status: str | None = None

_state = _SharedState()


# ── Fixtures ──────────────────────────────────────────────────────────────

@pytest.fixture(scope='session')
def grpc_address(request) -> str:
    host = request.config.getoption('--host')
    port = request.config.getoption('--port')
    return f'{host}:{port}'


@pytest.fixture(scope='session')
def db_path() -> str:
    return _get_db_path()


@pytest.fixture(scope='session', autouse=True)
def test_user_setup(db_path):
    """建立測試用戶（直接寫 DB，繞過 email 驗證），session 結束後自動清除"""
    hashed_pw = bcrypt.hashpw(TEST_PASSWORD.encode(), bcrypt.gensalt()).decode()
    conn = sqlite3.connect(db_path)
    conn.row_factory = sqlite3.Row
    cur = conn.cursor()
    cur.execute("SELECT id FROM users WHERE username = ?", (TEST_USER,))
    if cur.fetchone():
        cur.execute(
            "UPDATE users SET tokens = ?, updated_at = strftime('%s','now') WHERE username = ?",
            (TEST_BALANCE, TEST_USER),
        )
    else:
        cur.execute(
            "INSERT INTO users (username, password, tokens) VALUES (?, ?, ?)",
            (TEST_USER, hashed_pw, TEST_BALANCE),
        )
    conn.commit()
    conn.close()

    yield

    conn = sqlite3.connect(db_path)
    conn.execute("DELETE FROM users WHERE username = ?", (TEST_USER,))
    conn.commit()
    conn.close()


@pytest.fixture(scope='session')
def grpc_channel(grpc_address):
    channel = grpc.insecure_channel(grpc_address)
    yield channel
    channel.close()


@pytest.fixture(scope='session')
def user_stub(grpc_channel):
    return nodepool_pb2_grpc.UserServiceStub(grpc_channel)


@pytest.fixture(scope='session')
def master_stub(grpc_channel):
    return nodepool_pb2_grpc.MasterNodeServiceStub(grpc_channel)


@pytest.fixture(scope='session')
def auth_token(user_stub):
    resp = user_stub.Login(
        nodepool_pb2.LoginRequest(username=TEST_USER, password=TEST_PASSWORD),
        timeout=10,
    )
    assert resp.success, f"登入失敗: {resp.message}"
    assert resp.token, "token 為空"
    return resp.token


@pytest.fixture(scope='session')
def auth_meta(auth_token):
    return [('authorization', f'Bearer {auth_token}')]


# ── 測試案例（pytest 依字母順序執行，故用數字前綴保證順序）────────────────────

class TestE2EPlatform:

    def test_01_nodepool_connection(self, grpc_channel, grpc_address):
        """NodePool gRPC 連線是否正常"""
        try:
            grpc.channel_ready_future(grpc_channel).result(timeout=5)
        except grpc.FutureTimeoutError:
            pytest.fail(
                f"無法連線到 NodePool ({grpc_address})，"
                "請確認 node_pool_server.py 已啟動"
            )

    def test_02_login(self, auth_token):
        """登入並取得 JWT Token"""
        assert len(auth_token) > 10, "token 長度異常"

    def test_03_upload_task(self, master_stub, auth_meta, auth_token):
        """上傳任務 ZIP"""
        task_zip_bytes = _make_task_zip()
        assert len(task_zip_bytes) > 0, "task zip 為空"

        resp = master_stub.UploadTask(
            nodepool_pb2.UploadTaskRequest(
                task_zip=task_zip_bytes,
                memory_gb=1,
                cpu_score=1,
                gpu_score=0,
                gpu_memory_gb=0,
                token=auth_token,
                host_count=1,
            ),
            metadata=auth_meta,
            timeout=30,
        )
        assert resp.success, f"UploadTask 失敗: {resp.message}"

        # 從訊息中解析 task_id
        m = re.search(r'task[_\s]?id[:\s]+([A-Za-z0-9_-]+)', resp.message, re.IGNORECASE)
        if not m:
            m = re.search(r'\b([A-Za-z0-9_-]{8,})\b', resp.message)
        if m:
            _state.task_id = m.group(1)

    def test_04_task_completes(self, master_stub, auth_meta, auth_token):
        """任務在 POLL_TIMEOUT 內完成，最終狀態為 Completed"""
        elapsed = 0
        while elapsed < POLL_TIMEOUT:
            resp = master_stub.GetAllUserTasks(
                nodepool_pb2.GetAllUserTasksRequest(token=auth_token),
                metadata=auth_meta,
                timeout=10,
            )
            tasks = list(resp.tasks)
            if tasks:
                target = (
                    next((t for t in tasks if t.task_id == _state.task_id), None)
                    or tasks[-1]
                )
                if _state.task_id is None:
                    _state.task_id = target.task_id

                status = target.status
                if status in {'Completed', 'Failed', 'Stopped', 'Error'}:
                    _state.final_status = status
                    break

            time.sleep(POLL_INTERVAL)
            elapsed += POLL_INTERVAL

        assert _state.final_status is not None, \
            f"任務在 {POLL_TIMEOUT}s 內未結束（task_id={_state.task_id}）"
        assert _state.final_status == 'Completed', \
            f"任務非正常完成，實際狀態: {_state.final_status}"

    def test_05_task_logs_contain_output(self, master_stub, auth_meta, auth_token):
        """任務日誌包含預期的 [TEST] 輸出"""
        if not _state.task_id:
            pytest.skip("task_id 未知，跳過日誌驗證")

        resp = master_stub.GetTasklog(
            nodepool_pb2.TasklogRequest(task_id=_state.task_id, token=auth_token),
            metadata=auth_meta,
            timeout=10,
        )
        assert resp.success, f"GetTasklog 失敗"
        assert '[TEST]' in resp.log or '質數' in resp.log, \
            f"日誌中未找到 [TEST] 輸出，實際日誌（前 500 字）:\n{resp.log[:500]}"

    def test_06_result_zip_valid(self, master_stub, auth_meta, auth_token):
        """結果 ZIP 可下載且包含執行日誌與任務輸出"""
        if not _state.task_id:
            pytest.skip("task_id 未知，跳過結果驗證")
        if _state.final_status != 'Completed':
            pytest.skip(f"任務狀態 {_state.final_status}，跳過結果驗證")

        resp = master_stub.GetTaskResult(
            nodepool_pb2.GetTaskResultRequest(task_id=_state.task_id, token=auth_token),
            metadata=auth_meta,
            timeout=30,
        )
        assert resp.success, f"GetTaskResult 失敗: {resp.message}"
        assert len(resp.result_zip) > 0, "結果 ZIP 為空"

        with zipfile.ZipFile(io.BytesIO(resp.result_zip)) as zf:
            names = zf.namelist()
            assert len(names) > 0, "結果 ZIP 是空的"

            # 至少要有執行日誌檔
            has_log = any('execution_log' in n or 'task_logs' in n for n in names)
            assert has_log, f"結果 ZIP 中沒有日誌檔，包含: {names}"

            # 找 [TEST] 輸出
            found_output = any(
                '[TEST]' in zf.read(n).decode('utf-8', errors='replace')
                for n in names if n.endswith('.txt')
            )
            assert found_output, \
                f"結果 ZIP 的 txt 檔中未找到 [TEST] 輸出，檔案列表: {names}"
