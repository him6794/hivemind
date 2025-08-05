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
        logging.info(f"收到節點註冊請求: {request.node_id}, GPU Name: {request.gpu_name}, Docker Status: {request.docker_status}")
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
                request.gpu_name,
                request.docker_status  # 確保Docker狀態參數傳遞
            )
            
            if success:
                logging.info(f"節點 {request.node_id} 註冊成功")
            
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
            response = self.node_manager.report_status(request, context)
            return response
        except Exception as e:
            logging.error(f"處理節點狀態回報錯誤: {e}")
            return nodepool_pb2.StatusResponse(success=False, message="伺服器內部錯誤")

    def GetNodeList(self, request, context):
        """獲取節點列表"""
        try:
            nodes_data = self.node_manager.get_node_list()
            
            # 轉換為 protobuf 對象
            proto_nodes = []
            for node_data in nodes_data:
                try:
                    proto_node = nodepool_pb2.WorkerNodeInfo(
                        node_id=node_data['node_id'],
                        hostname=node_data['hostname'],
                        cpu_cores=node_data['cpu_cores'],
                        memory_gb=node_data['memory_gb'],
                        status=node_data['status'],
                        last_heartbeat=node_data['last_heartbeat'],
                        cpu_score=node_data['cpu_score'],
                        gpu_score=node_data['gpu_score'],
                        gpu_memory_gb=node_data['gpu_memory_gb'],
                        location=node_data['location'],
                        port=node_data['port'],
                        gpu_name=node_data['gpu_name']
                    )
                    proto_nodes.append(proto_node)
                except Exception as e:
                    logging.warning(f"轉換節點 {node_data.get('node_id', 'unknown')} 為 protobuf 失敗: {e}")
                    continue
            
            return nodepool_pb2.GetNodeListResponse(
                success=True,
                message=f"找到 {len(proto_nodes)} 個節點",
                nodes=proto_nodes
            )
        except Exception as e:
            logging.error(f"GetNodeList 服務錯誤: {e}", exc_info=True)
            return nodepool_pb2.GetNodeListResponse(
                success=False,
                message=f"獲取節點列表失敗: {str(e)}",
                nodes=[]
            )