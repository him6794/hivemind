# node_pool/worker_node_service.py
import grpc
import logging
import nodepool_pb2
import nodepool_pb2_grpc
from master_node_service import TaskManager
from node_manager import NodeManager

logging.basicConfig(level=logging.INFO, format='%(asctime)s - %(levelname)s - %(message)s')

class WorkerNodeServiceServicer(nodepool_pb2_grpc.WorkerNodeServiceServicer):
    def __init__(self):
        # 與 MasterNodeService 共用相同的資料層（Redis/檔案系統），不需要共享同一實例
        self.task_manager = TaskManager()
        self.node_manager = NodeManager()

    def ExecuteTask(self, request, context):
        """執行任務 - 這個方法通常由實際的工作節點實現"""
        logging.info(f"收到任務執行請求: {request.task_id}")
        try:
            # 這裡應該是實際執行任務的邏輯
            # 由於這是 node_pool 服務，這個方法主要用於接口定義
            return nodepool_pb2.ExecuteTaskResponse(
                success=True,
                message="任務執行中"
            )
        except Exception as e:
            logging.error(f"ExecuteTask 錯誤: {e}")
            return nodepool_pb2.ExecuteTaskResponse(
                success=False,
                message=f"執行錯誤: {str(e)}"
            )

    def TaskOutputUpload(self, request, context):
        """任務輸出上傳"""
        logging.info(f"收到任務輸出上傳: {request.task_id}")
        try:
            # 儲存累積的任務輸出到 Redis
            ok = self.task_manager.store_output(request.task_id, request.output)
            if ok:
                return nodepool_pb2.TaskOutputUploadResponse(
                    success=True,
                    message="輸出上傳成功"
                )
            else:
                return nodepool_pb2.TaskOutputUploadResponse(
                    success=False,
                    message="輸出上傳失敗"
                )
        except Exception as e:
            logging.error(f"TaskOutputUpload 錯誤: {e}")
            return nodepool_pb2.TaskOutputUploadResponse(
                success=False,
                message=f"上傳錯誤: {str(e)}"
            )

    def TaskResultUpload(self, request, context):
        """任務結果上傳：寫入硬碟並更新任務狀態，釋放節點資源"""
        logging.info(f"收到任務結果上傳: {request.task_id} ({len(request.result_zip)} bytes)")
        try:
            task_id = request.task_id
            # 儲存結果ZIP並標記為 COMPLETED
            stored = self.task_manager.store_result(task_id, request.result_zip)
            if not stored:
                return nodepool_pb2.TaskResultUploadResponse(
                    success=False,
                    message="結果存儲失敗"
                )

            # 嘗試釋放節點資源
            info = self.task_manager.get_task_info(task_id, include_zip=False)
            if info:
                assigned_node = info.get("assigned_node") or ""
                try:
                    cpu = int(info.get("cpu_score") or 0)
                    mem = int(info.get("memory_gb") or 0)
                    gpu = int(info.get("gpu_score") or 0)
                    gpumem = int(info.get("gpu_memory_gb") or 0)
                except Exception:
                    cpu = mem = gpu = gpumem = 0

                if assigned_node:
                    ok, msg = self.node_manager.release_node_resources(assigned_node, task_id, cpu, mem, gpu, gpumem)
                    if ok:
                        logging.info(f"任務 {task_id} 已釋放節點 {assigned_node} 資源")
                    else:
                        logging.warning(f"任務 {task_id} 資源釋放失敗: {msg}")

                # 清空 assigned_node 標記
                self.task_manager.update_task_status(task_id, info.get("status", "COMPLETED"), assigned_node=None)

            logging.info(f"任務 {task_id} 結果上傳並處理完成")
            return nodepool_pb2.TaskResultUploadResponse(
                success=True,
                message="結果上傳成功"
            )
        except Exception as e:
            logging.error(f"TaskResultUpload 錯誤: {e}")
            return nodepool_pb2.TaskResultUploadResponse(
                success=False,
                message=f"上傳錯誤: {str(e)}"
            )

    def TaskOutput(self, request, context):
        """任務輸出"""
        logging.info(f"收到任務輸出請求: {request.task_id}")
        try:
            return nodepool_pb2.TaskOutputResponse(
                success=True,
                message="輸出處理成功"
            )
        except Exception as e:
            logging.error(f"TaskOutput 錯誤: {e}")
            return nodepool_pb2.TaskOutputResponse(
                success=False,
                message=f"輸出錯誤: {str(e)}"
            )

    def StopTaskExecution(self, request, context):
        """停止任務執行"""
        logging.info(f"收到停止任務請求: {request.task_id}")
        try:
            return nodepool_pb2.StopTaskExecutionResponse(
                success=True,
                message="任務已停止"
            )
        except Exception as e:
            logging.error(f"StopTaskExecution 錯誤: {e}")
            return nodepool_pb2.StopTaskExecutionResponse(
                success=False,
                message=f"停止錯誤: {str(e)}"
            )

    def TaskUsage(self, request, context):
        """任務資源使用情況"""
        logging.info(f"收到任務使用情況報告: {request.task_id}")
        try:
            return nodepool_pb2.TaskUsageResponse(
                success=True,
                message="使用情況已記錄"
            )
        except Exception as e:
            logging.error(f"TaskUsage 錯誤: {e}")
            return nodepool_pb2.TaskUsageResponse(
                success=False,
                message=f"記錄錯誤: {str(e)}"
            )