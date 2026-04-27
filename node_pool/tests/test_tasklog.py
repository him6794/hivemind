import sys
import os
sys.path.insert(0, os.path.join(os.path.dirname(__file__), 'node_pool'))

import grpc
import nodepool_pb2
import nodepool_pb2_grpc

# 模擬 Master 的請求
def test_get_tasklog():
    # 直接使用 Master UI 的 token（從瀏覽器複製）
    print("請從 Master UI 登入後，在瀏覽器複製 token")
    print("或者直接使用測試 token\n")
    
    # 你可以從 Master UI 的請求中複製 token，或者讓程序生成一個
    token = input("請輸入 token（直接按 Enter 嘗試新登入）: ").strip()
    
    if not token:
        # 嘗試登入
        channel = grpc.insecure_channel('127.0.0.1:50051')
        user_stub = nodepool_pb2_grpc.UserServiceStub(channel)
        
        username = input("用戶名 [justin]: ").strip() or "justin"
        password = input("密碼: ").strip()
        
        print(f"\n1. 登入中 ({username})...")
        login_req = nodepool_pb2.LoginRequest(username=username, password=password)
        login_resp = user_stub.Login(login_req)
        
        if not login_resp.success:
            print(f"❌ 登入失敗: {login_resp.message}")
            return
        
        token = login_resp.token
        print(f"✅ 登入成功")
    
    # 連接到 NodePool
    channel = grpc.insecure_channel('127.0.0.1:50051')
    master_stub = nodepool_pb2_grpc.MasterNodeServiceStub(channel)
    
    # 測試獲取任務日誌
    task_id = "task_20260102_124653_task_533617c6_4"
    print(f"\n2. 請求任務日誌: {task_id}")
    print(f"   使用 token: {token[:20]}...")
    
    try:
        log_req = nodepool_pb2.TasklogRequest(task_id=task_id, token=token)
        metadata = [('authorization', f'Bearer {token}')]
        log_resp = master_stub.GetTasklog(log_req, metadata=metadata, timeout=10)
        
        print(f"✅ 成功獲取日誌，長度: {len(log_resp.log)} bytes")
        if log_resp.log:
            print(f"\n日誌內容（前 500 字元）:\n{log_resp.log[:500]}")
    except grpc.RpcError as e:
        print(f"❌ gRPC 錯誤:")
        print(f"   狀態碼: {e.code()}")
        print(f"   詳情: {e.details()}")
        
        # 顯示更多調試信息
        if e.code() == grpc.StatusCode.PERMISSION_DENIED:
            print("\n🔍 權限被拒絕，檢查以下內容:")
            print(f"   - Task ID: {task_id}")
            print(f"   - Token: {token[:20]}...")
            print(f"   - 用戶: justin")

if __name__ == "__main__":
    test_get_tasklog()
