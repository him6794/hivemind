# node_pool/node_pool_server.py
import grpc
from concurrent import futures
import logging
import nodepool_pb2
import nodepool_pb2_grpc
from user_service import UserServiceServicer
# 修正這裡的匯入
from node_manager_service import NodeManagerServiceServicer # <--- 從正確的檔案匯入
from master_node_service import MasterNodeServiceServicer # 假設這個檔案和匯入是正確的

logging.basicConfig(level=logging.INFO, format='%(asctime)s - %(levelname)s - %(message)s')
DEFAULT_PORT=50051

def serve():
    server = grpc.server(futures.ThreadPoolExecutor(max_workers=10))
    nodepool_pb2_grpc.add_UserServiceServicer_to_server(UserServiceServicer(), server)
    # 這裡會使用從 node_manager_service.py 匯入的正確 Servicer
    nodepool_pb2_grpc.add_NodeManagerServiceServicer_to_server(NodeManagerServiceServicer(), server)
    nodepool_pb2_grpc.add_MasterNodeServiceServicer_to_server(MasterNodeServiceServicer(), server)
    server.add_insecure_port(f'[::]:{DEFAULT_PORT}') # 移除後面的 options
    logging.info(f"服務已啟動，端口: {DEFAULT_PORT}")
    server.start()
    server.wait_for_termination()
if __name__ == "__main__":
    serve()