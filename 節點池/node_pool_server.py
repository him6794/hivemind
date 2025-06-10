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
    server = grpc.server(futures.ThreadPoolExecutor(max_workers=10))
    
    user_service = UserServiceServicer()
    node_manager_service = NodeManagerServiceServicer()
    master_node_service = MasterNodeServiceServicer()
    
    master_node_service.auth_manager = user_service
    
    nodepool_pb2_grpc.add_UserServiceServicer_to_server(user_service, server)
    nodepool_pb2_grpc.add_NodeManagerServiceServicer_to_server(node_manager_service, server)
    nodepool_pb2_grpc.add_MasterNodeServiceServicer_to_server(master_node_service, server)
    
    server.add_insecure_port(f'[::]:{DEFAULT_PORT}')
    logging.info(f"服務已啟動，端口: {DEFAULT_PORT}")
    server.start()
    server.wait_for_termination()
    
if __name__ == "__main__":
    serve()