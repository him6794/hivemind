import grpc
import logging
import nodepool_pb2
import nodepool_pb2_grpc
import redis
import time
from node_manager import NodeManager

class NodeManagerServiceServicer(nodepool_pb2_grpc.NodeManagerServiceServicer):
    """节点管理服务实现"""

    def __init__(self):
        self.node_manager = NodeManager()

    # 修改 RegisterWorkerNode 方法
    def RegisterWorkerNode(self, request, context):
        logging.info(f"收到節點註冊請求: {request.node_id}, GPU Name: {request.gpu_name}") # <--- 加入日誌記錄 GPU Name
        try:
            # 調用 NodeManager 的方法，並傳入 request 中的所有相關欄位，包括 gpu_name
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
                request.gpu_name # <--- 將請求中的 gpu_name 傳遞下去
            )
            # 返回從 NodeManager 得到的結果
            return nodepool_pb2.StatusResponse(success=success, message=message)

        except Exception as e:
            logging.error(f"處理節點 {request.node_id} 註冊請求時發生錯誤: {e}", exc_info=True)
            return nodepool_pb2.StatusResponse(success=False, message=f"伺服器內部錯誤: {e}")

    def HealthCheck(self, request, context):
        """健康检查"""
        try:
            healthy, message = self.node_manager.health_check() # 调用 NodeManager 的健康检查方法，并接收返回值
            return nodepool_pb2.HealthCheckResponse(healthy=healthy, message=message) # 返回健康检查结果 (根据 NodeManager 的返回值)
        except Exception as e:
            logging.error(f"健康检查错误: {e}")
            return nodepool_pb2.HealthCheckResponse(healthy=False, message="服务器内部错误") # 错误处理
    def ReportStatus(self, request, context):
        """节点报告状态"""
        try:
            node_id = request.node_id # 获取节点 ID
            status_message = request.status_message # 获取状态信息

            success, message = self.node_manager.report_status(node_id, status_message) # 调用 NodeManager 的状态报告方法

            return nodepool_pb2.StatusResponse(success=success, message=message) # 返回状态报告处理结果
        except Exception as e:
            logging.error(f"处理节点状态报告错误: {e}")
            return nodepool_pb2.StatusResponse(success=False, message="服务器内部错误") # 错误处理
    def GetNodeList(self, request, context):
        """獲取節點列表"""
        try:
            nodes = self.node_manager.get_node_list()  # 調用 NodeManager 的 get_node_list 方法
            return nodepool_pb2.GetNodeListResponse(
                success=True,
                message=f"成功獲取節點列表，共 {len(nodes)} 個節點",
                nodes=nodes  # 返回節點資訊列表
            )
        except Exception as e:
            logging.error(f"獲取節點列表錯誤: {e}")
            return nodepool_pb2.GetNodeListResponse(
                success=False,
                message="服務器內部錯誤",
                nodes=[]  # 返回空列表
            )