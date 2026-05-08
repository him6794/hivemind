#!/usr/bin/env python3
"""
獲取任務日誌的工具腳本
"""
import sys
import grpc
from pathlib import Path

# 添加 proto 路徑
REPO_ROOT = Path(__file__).resolve().parent
sys.path.insert(0, str(REPO_ROOT / "services" / "nodepool" / "pb"))

try:
    import hivemind_pb2
    import hivemind_pb2_grpc
except ImportError:
    print("錯誤: 找不到 hivemind_pb2 模組")
    print("請確保已生成 Python proto 文件")
    sys.exit(1)


def get_task_log(nodepool_addr, token, task_id):
    """獲取任務日誌"""
    channel = grpc.insecure_channel(nodepool_addr)
    stub = hivemind_pb2_grpc.MasterNodeServiceStub(channel)
    
    try:
        # 獲取任務日誌
        response = stub.GetTasklog(
            hivemind_pb2.TasklogRequest(
                token=token,
                task_id=task_id
            )
        )
        
        if response.success:
            print(f"\n{'='*60}")
            print(f"任務 ID: {task_id}")
            print(f"{'='*60}")
            print(response.log)
            print(f"{'='*60}\n")
            return True
        else:
            print(f"獲取日誌失敗: {response.log}")
            return False
            
    except grpc.RpcError as e:
        print(f"RPC 錯誤: {e}")
        return False
    finally:
        channel.close()


def get_all_tasks(nodepool_addr, token):
    """獲取所有任務"""
    channel = grpc.insecure_channel(nodepool_addr)
    stub = hivemind_pb2_grpc.MasterNodeServiceStub(channel)
    
    try:
        response = stub.GetAllUserTasks(
            hivemind_pb2.GetAllUserTasksRequest(token=token)
        )
        
        if not response.tasks:
            print("沒有找到任務")
            return []
        
        print(f"\n找到 {len(response.tasks)} 個任務:\n")
        for task in response.tasks:
            print(f"  任務 ID: {task.task_id}")
            print(f"  狀態: {task.status}")
            print(f"  訊息: {task.status_message}")
            print(f"  Worker IP: {task.worker_ip}")
            print(f"  CPU 使用率: {task.cpu_usage:.1f}%")
            print(f"  記憶體使用率: {task.memory_usage:.1f}%")
            print()
        
        return response.tasks
        
    except grpc.RpcError as e:
        print(f"RPC 錯誤: {e}")
        return []
    finally:
        channel.close()


def login(nodepool_addr, username, password):
    """登入並獲取 token"""
    channel = grpc.insecure_channel(nodepool_addr)
    stub = hivemind_pb2_grpc.UserServiceStub(channel)
    
    try:
        response = stub.Login(
            hivemind_pb2.LoginRequest(
                username=username,
                password=password
            )
        )
        
        if response.success:
            print(f"登入成功: {username}")
            return response.token
        else:
            print(f"登入失敗: {response.message}")
            return None
            
    except grpc.RpcError as e:
        print(f"RPC 錯誤: {e}")
        return None
    finally:
        channel.close()


def main():
    import argparse
    
    parser = argparse.ArgumentParser(description="獲取 HiveMind 任務日誌")
    parser.add_argument("--nodepool", default="localhost:50051", help="Nodepool 地址")
    parser.add_argument("--user", default="testuser", help="用戶名")
    parser.add_argument("--password", default="testpass123", help="密碼")
    parser.add_argument("--task-id", help="任務 ID（如果不指定，顯示所有任務）")
    parser.add_argument("--list", action="store_true", help="列出所有任務")
    
    args = parser.parse_args()
    
    # 登入
    token = login(args.nodepool, args.user, args.password)
    if not token:
        sys.exit(1)
    
    # 列出所有任務或獲取特定任務日誌
    if args.list or not args.task_id:
        tasks = get_all_tasks(args.nodepool, token)
        
        if tasks and not args.task_id:
            # 如果有任務且沒有指定 task_id，詢問用戶
            print("\n要查看哪個任務的日誌？")
            print("輸入任務 ID，或按 Enter 查看最新任務:")
            task_id = input().strip()
            
            if not task_id and tasks:
                # 使用最新的任務
                task_id = tasks[0].task_id
                print(f"\n使用最新任務: {task_id}\n")
            
            if task_id:
                get_task_log(args.nodepool, token, task_id)
    else:
        # 獲取指定任務的日誌
        get_task_log(args.nodepool, token, args.task_id)


if __name__ == "__main__":
    main()
