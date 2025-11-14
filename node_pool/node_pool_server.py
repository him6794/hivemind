"""Node Pool gRPC Server.

這是 HiveMind 節點池的 gRPC 服務器，負責管理用戶服務、節點管理服務、
主節點服務和工作節點服務。
"""

import logging
from concurrent import futures

import grpc

import nodepool_pb2_grpc
from config import Config
from master_node_service import MasterNodeServiceServicer
from node_manager_service import NodeManagerServiceServicer
from user_service import UserServiceServicer
from worker_node_service import WorkerNodeServiceServicer

logging.basicConfig(level=logging.INFO, format='%(asctime)s - %(levelname)s - %(message)s')
DEFAULT_PORT = 50051

def serve():
    """啟動並運行 gRPC 服務器。"""
    # 配置 gRPC 服務器選項以提高性能和穩定性，支援大檔案傳輸
    options = [
        ('grpc.keepalive_time_ms', 10000),
        ('grpc.keepalive_timeout_ms', 5000),
        ('grpc.keepalive_permit_without_calls', True),
        ('grpc.http2.max_pings_without_data', 0),
        ('grpc.http2.min_time_between_pings_ms', 10000),
        ('grpc.http2.min_ping_interval_without_data_ms', 5000),
        ('grpc.max_receive_message_length', 100 * 1024 * 1024),  # 100MB
        ('grpc.max_send_message_length', 100 * 1024 * 1024),     # 100MB
        ('grpc.http2.max_frame_size', 16 * 1024 * 1024),         # 16MB frames
    ]

    server = grpc.server(
        futures.ThreadPoolExecutor(max_workers=20),  # 增加工作線程
        options=options
    )

    # 初始化所有服務
    user_service = UserServiceServicer()
    node_manager_service = NodeManagerServiceServicer()
    master_node_service = MasterNodeServiceServicer()
    worker_node_service = WorkerNodeServiceServicer()

    # 添加服務到服務器
    nodepool_pb2_grpc.add_UserServiceServicer_to_server(user_service, server)
    nodepool_pb2_grpc.add_NodeManagerServiceServicer_to_server(node_manager_service, server)
    nodepool_pb2_grpc.add_MasterNodeServiceServicer_to_server(master_node_service, server)
    nodepool_pb2_grpc.add_WorkerNodeServiceServicer_to_server(worker_node_service, server)
    
    server.add_insecure_port(f'[::]:{DEFAULT_PORT}')

    logging.info("節點池服務已啟動，端口: %s", DEFAULT_PORT)
    task_storage_path = getattr(
        Config, 'TASK_STORAGE_PATH', '/mnt/myusb/hivemind/task_storage'
    )
    logging.info("任務文件存儲目錄: %s", task_storage_path)
    server.start()
    server.wait_for_termination()

if __name__ == "__main__":
    serve()
