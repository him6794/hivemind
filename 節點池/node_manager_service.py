import grpc
import logging
import nodepool_pb2
import nodepool_pb2_grpc
import redis
import time
from node_manager import NodeManager

class NodeManagerServiceServicer(nodepool_pb2_grpc.NodeManagerServiceServicer):
    def __init__(self):
        self.node_manager = NodeManager()

    def RegisterWorkerNode(self, request, context):
        logging.info(f"收到節點註冊請求: {request.node_id}, GPU Name: {request.gpu_name}")
        try:
            success, message = self.node_manager.register_worker_node(
                request.node_id,
                request.hostname,
                request.cpu_cores,
                request.memory_gb,
                request.cpu_score,
                request.gpu_score,
                request.gpu_memory_gb,
                request.location,
                request.port,
                request.gpu_name
            )
            return nodepool_pb2.StatusResponse(success=success, message=message)
        except Exception as e:
            logging.error(f"處理節點 {request.node_id} 註冊請求時發生錯誤: {e}", exc_info=True)
            return nodepool_pb2.StatusResponse(success=False, message=f"伺服器內部錯誤: {e}")

    def HealthCheck(self, request, context):
        try:
            healthy, message = self.node_manager.health_check()
            return nodepool_pb2.HealthCheckResponse(healthy=healthy, message=message)
        except Exception as e:
            logging.error(f"健康檢查錯誤: {e}")
            return nodepool_pb2.HealthCheckResponse(healthy=False, message="伺服器內部錯誤")

    def ReportStatus(self, request, context):
        try:
            node_id = request.node_id
            status_message = request.status_message
            success, message = self.node_manager.report_status(node_id, status_message)
            return nodepool_pb2.StatusResponse(success=success, message=message)
        except Exception as e:
            logging.error(f"處理節點狀態回報錯誤: {e}")
            return nodepool_pb2.StatusResponse(success=False, message="伺服器內部錯誤")

    def GetNodeList(self, request, context):
        try:
            nodes = self.node_manager.get_node_list()
            return nodepool_pb2.GetNodeListResponse(
                success=True,
                message=f"成功取得節點列表，共 {len(nodes)} 個節點",
                nodes=nodes
            )
        except Exception as e:
            logging.error(f"取得節點列表錯誤: {e}")
            return nodepool_pb2.GetNodeListResponse(
                success=False,
                message="伺服器內部錯誤",
                nodes=[]
            )