import grpc
from concurrent import futures
import logging
import nodepool_pb2
import nodepool_pb2_grpc
from user_service import UserServiceServicer
from node_manager_service import NodeManagerServiceServicer
from master_node_service import MasterNodeServiceServicer

logging.basicConfig(level=logging.INFO, format='%(asctime)s - %(levelname)s - %(message)s')
DEFAULT_PORT=50051

def serve():
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
    
    user_service = UserServiceServicer()
    node_manager_service = NodeManagerServiceServicer()
    master_node_service = MasterNodeServiceServicer()
    
    master_node_service.auth_manager = user_service
    
    nodepool_pb2_grpc.add_UserServiceServicer_to_server(user_service, server)
    nodepool_pb2_grpc.add_NodeManagerServiceServicer_to_server(node_manager_service, server)
    nodepool_pb2_grpc.add_MasterNodeServiceServicer_to_server(master_node_service, server)
    
    server.add_insecure_port(f'[::]:{DEFAULT_PORT}')
    logging.info(f"節點池服務已啟動，端口: {DEFAULT_PORT}")
    server.start()
    server.wait_for_termination()
    
if __name__ == "__main__":
    serve()