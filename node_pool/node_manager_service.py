import grpc
import logging
import time
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

    def ReportStatus(self, request, context):
        """工作節點回報即時狀態（RunningStatusRequest）"""
        try:
            node_id = request.node_id
            if not node_id:
                return nodepool_pb2.RunningStatusResponse(success=False, message="缺少 node_id")

            node_key = f"node:{node_id}"
            if not self.node_manager.redis_client.exists(node_key):
                return nodepool_pb2.RunningStatusResponse(success=False, message=f"節點 {node_id} 未註冊")

            # 更新狀態與心跳
            try:
                self.node_manager.report_status(request)
            except Exception as e:
                logging.warning(f"report_status 執行警告: {e}")

            # 更新 docker_status / running_tasks
            mapping = {
                "updated_at": str(time.time()),
                "last_heartbeat": str(time.time()),
            }
            if getattr(request, 'docker_status', ''):
                mapping["docker_status"] = request.docker_status
            try:
                running_tasks = int(getattr(request, 'running_tasks', 0))
                mapping["current_tasks"] = str(max(0, running_tasks))
            except Exception:
                pass
            self.node_manager.redis_client.hset(node_key, mapping=mapping)

            # 更新即時使用情況
            try:
                self.node_manager.update_node_usage(
                    node_id=node_id,
                    cpu_usage=request.cpu_usage,
                    memory_usage=request.memory_usage,
                    gpu_usage=request.gpu_usage,
                    gpu_memory_usage=request.gpu_memory_usage
                )
            except Exception as e:
                logging.warning(f"update_node_usage 警告: {e}")

            return nodepool_pb2.RunningStatusResponse(success=True, message="狀態已更新")
        except Exception as e:
            logging.error(f"ReportStatus 錯誤: {e}", exc_info=True)
            return nodepool_pb2.RunningStatusResponse(success=False, message=str(e))