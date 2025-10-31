import grpc
import logging
import nodepool_pb2
import nodepool_pb2_grpc
from node_manager import NodeManager

logging.basicConfig(level=logging.INFO, format='%(asctime)s - %(levelname)s - %(message)s')

class NodeManagerServiceServicer(nodepool_pb2_grpc.NodeManagerServiceServicer):
    def __init__(self):
        self.node_manager = NodeManager()

    def RegisterWorkerNode(self, request, context):
        """註冊工作節點"""
        logging.info(f"收到節點註冊請求: {request.node_id}, GPU Name: {request.gpu_name}, Docker Status: {request.docker_status}")
        try:
            success = self.node_manager.register_worker_node(
                request.node_id,
                request.hostname,
                request.cpu_cores,
                request.memory_gb,
                request.cpu_score,
                request.gpu_score,
                request.gpu_memory_gb,
                request.location,
                request.port,
                request.gpu_name,
                request.docker_status
            )
            
            if success:
                logging.info(f"節點 {request.node_id} 註冊成功")
                return nodepool_pb2.StatusResponse(success=True, message="節點註冊成功")
            else:
                logging.warning(f"節點 {request.node_id} 註冊失敗")
                return nodepool_pb2.StatusResponse(success=False, message="節點註冊失敗")
                
        except Exception as e:
            logging.error(f"處理節點 {request.node_id} 註冊請求時發生錯誤: {e}", exc_info=True)
            return nodepool_pb2.StatusResponse(success=False, message=f"註冊錯誤: {str(e)}")