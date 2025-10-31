"""gRPC servicer implementation for WorkerNode.

Separated from worker_node.py to keep responsibilities modular.
"""

from __future__ import annotations

import os
import time
from threading import Thread
from zipfile import ZipFile
from io import BytesIO

# Support both package-relative and direct module imports
try:
    from . import nodepool_pb2, nodepool_pb2_grpc  # type: ignore
except Exception:  # running as a direct script/module
    import nodepool_pb2  # type: ignore
    import nodepool_pb2_grpc  # type: ignore

try:
    from .system_metrics import cpu_percent, virtual_memory  # type: ignore
except Exception:
    from system_metrics import cpu_percent, virtual_memory  # type: ignore


class WorkerNodeServicer(nodepool_pb2_grpc.WorkerNodeServiceServicer):
    def __init__(self, worker_node):
        self.worker_node = worker_node

    def ExecuteTask(self, request, context):
        """執行任務 RPC"""
        task_id = request.task_id
        task_zip = request.task_zip

        # 獲取任務所需資源 - 根據 proto 定義修正字段名
        required_resources = {
            "cpu": request.cpu_usage,
            "memory_gb": request.memory_gb,
            "gpu": request.gpu_usage,
            "gpu_memory_gb": request.gpu_memory_gb
        }

        file_size_mb = len(task_zip) / (1024 * 1024)
        self.worker_node._log("===== 收到執行任務請求 =====")
        self.worker_node._log(f"任務ID: {task_id}")
        self.worker_node._log(
            f"請求資源: CPU={required_resources['cpu']}, RAM={required_resources['memory_gb']}GB, "
            f"GPU={required_resources['gpu']}, GPU_MEM={required_resources['gpu_memory_gb']}GB"
        )
        self.worker_node._log(f"當前節點狀態: {self.worker_node.status}")

        # 檢查是否有足夠資源
        if not self.worker_node._check_resources_available(required_resources):
            error_msg = f"資源不足，拒絕任務 {task_id}"
            self.worker_node._log(error_msg)
            return nodepool_pb2.ExecuteTaskResponse(
                success=False,
                message=error_msg
            )

        # 檢查Docker要求
        if not self.worker_node.docker_available and self.worker_node.trust_score <= 50:
            error_msg = f"無Docker環境且信任分數低，拒絕任務 {task_id}"
            self.worker_node._log(error_msg)
            return nodepool_pb2.ExecuteTaskResponse(
                success=False,
                message="Docker unavailable and trust score too low"
            )

        # 檢查任務數據完整性和大小
        if not task_zip:
            error_msg = f"任務 {task_id} 數據為空"
            self.worker_node._log(error_msg)
            return nodepool_pb2.ExecuteTaskResponse(
                success=False,
                message="Task data is empty"
            )

        # 檢查檔案大小限制（100MB）
        if file_size_mb > 100:
            error_msg = f"任務 {task_id} 檔案太大: {file_size_mb:.1f}MB，超過100MB限制"
            self.worker_node._log(error_msg)
            return nodepool_pb2.ExecuteTaskResponse(
                success=False,
                message=f"Task file too large: {file_size_mb:.1f}MB (limit: 100MB)"
            )

        try:
            # 驗證ZIP檔案
            try:
                with ZipFile(BytesIO(task_zip), 'r') as zip_ref:
                    zip_ref.testzip()
                self.worker_node._log(f"任務 {task_id} ZIP 檔案驗證成功")
            except Exception as zip_error:
                error_msg = f"任務 {task_id} ZIP 檔案損壞: {zip_error}"
                self.worker_node._log(error_msg)
                return nodepool_pb2.ExecuteTaskResponse(
                    success=False,
                    message=f"Invalid ZIP file: {str(zip_error)}"
                )

            # 分配資源
            if not self.worker_node._allocate_resources(task_id, required_resources):
                error_msg = f"資源分配失敗，拒絕任務 {task_id}"
                self.worker_node._log(error_msg)
                return nodepool_pb2.ExecuteTaskResponse(
                    success=False,
                    message="Failed to allocate resources"
                )

            # 啟動執行線程
            execution_thread = Thread(
                target=self.worker_node._execute_task,
                args=(task_id, task_zip, required_resources),
                daemon=True,
                name=f"TaskExecution-{task_id}"
            )
            execution_thread.start()

            success_msg = f"任務 {task_id} 已接受並開始準備執行 (檔案大小: {file_size_mb:.1f}MB)"
            self.worker_node._log(success_msg)

            return nodepool_pb2.ExecuteTaskResponse(
                success=True,
                message=success_msg
            )

        except Exception as e:
            # 如果出錯，釋放資源
            self.worker_node._release_resources(task_id)
            error_msg = f"接受任務 {task_id} 時發生錯誤: {e}"
            self.worker_node._log(error_msg)
            return nodepool_pb2.ExecuteTaskResponse(
                success=False,
                message=f"Failed to accept task: {str(e)}"
            )

    def ReportOutput(self, request, context):
        """報告任務輸出"""
        node_id = request.node_id
        task_id = request.task_id
        output = request.output

        if node_id != self.worker_node.node_id:
            return nodepool_pb2.StatusResponse(
                success=False,
                message=f"Node ID mismatch: {node_id} != {self.worker_node.node_id}"
            )

        try:
            # 檢查任務是否存在
            with self.worker_node.resources_lock:
                if task_id not in self.worker_node.running_tasks:
                    return nodepool_pb2.StatusResponse(
                        success=False,
                        message=f"Task {task_id} not found"
                    )

            # 發送輸出到主節點（目前本地記錄）
            self.worker_node._send_task_logs(task_id, output)

            return nodepool_pb2.StatusResponse(
                success=True,
                message=f"Output reported for task {task_id}"
            )
        except Exception as e:
            return nodepool_pb2.StatusResponse(
                success=False,
                message=f"Failed to report output: {str(e)}"
            )

    def ReportRunningStatus(self, request, context):
        """報告運行狀態"""
        task_id = request.task_id

        # 檢查任務是否存在
        with self.worker_node.resources_lock:
            if task_id not in self.worker_node.running_tasks:
                return nodepool_pb2.RunningStatusResponse(
                    success=False,
                    message=f"Not running task {task_id}"
                )

        # 獲取真實的系統資源使用情況
        try:
            cpu_usage = cpu_percent()
            memory_info = virtual_memory()
            memory_usage = int((memory_info.total - memory_info.available) / (1024 * 1024))

            # GPU 使用情況（需要專門的庫來獲取，暫時設為 0）
            gpu_usage = 0
            gpu_memory_usage = 0
        except Exception as e:
            self.worker_node._log(f"獲取系統資源使用失敗: {e}")
            # 使用請求中的值作為備用
            cpu_usage = request.cpu_usage
            memory_usage = request.memory_usage
            gpu_usage = request.gpu_usage
            gpu_memory_usage = request.gpu_memory_usage

        # 發送狀態報告
        try:
            self.worker_node._log(
                f"Reporting real status for task {task_id}: CPU={cpu_usage:.1f}%, "
                f"MEM={memory_usage}MB, GPU={gpu_usage}%, GPU_MEM={gpu_memory_usage}MB"
            )

            # 根據真實資源使用情況計算 CPT 獎勵
            base_reward = 0.1  # 基礎獎勵
            cpu_reward = (cpu_usage / 100) * 0.5  # CPU 使用率獎勵
            memory_reward = (memory_usage / 1024) * 0.1  # 每 GB 內存使用獎勵 0.1 CPT

            cpt_reward = base_reward + cpu_reward + memory_reward

            return nodepool_pb2.RunningStatusResponse(
                success=True,
                message=f"Task {task_id} running - CPU: {cpu_usage:.1f}%, Memory: {memory_usage}MB",
                cpt_reward=cpt_reward
            )
        except Exception as e:
            self.worker_node._log(f"Failed to report status: {e}")
            return nodepool_pb2.RunningStatusResponse(
                success=False,
                message=f"Failed to report status: {str(e)}"
            )

    def StopTaskExecution(self, request, context):
        """立即強制停止任務執行並打包結果"""
        task_id = request.task_id

        # 檢查任務是否存在
        with self.worker_node.resources_lock:
            if task_id not in self.worker_node.running_tasks:
                return nodepool_pb2.StopTaskExecutionResponse(
                    success=False,
                    message=f"Task {task_id} not running"
                )

        self.worker_node._log(f"收到停止任務 {task_id} 的請求，立即執行強制停止")

        # 發送停止信號
        success = self.worker_node._stop_task(task_id)

        if success:
            return nodepool_pb2.StopTaskExecutionResponse(
                success=True,
                message=f"Stop signal sent to task {task_id}"
            )
        else:
            return nodepool_pb2.StopTaskExecutionResponse(
                success=False,
                message=f"Failed to send stop signal to task {task_id}"
            )

    def _execute_task_safely(self, task_id, task_dir, cmd):
        """安全地執行任務"""
        import subprocess

        self.worker_node._log(f"執行命令: {' '.join(cmd)}")
        self.worker_node._log(f"工作目錄: {task_dir}")

        # 創建結果目錄
        result_dir = os.path.join(task_dir, 'result')
        os.makedirs(result_dir, exist_ok=True)

        try:
            # 設置環境變數
            env = os.environ.copy()
            env['TASK_ID'] = task_id
            env['TASK_DIR'] = task_dir
            env['RESULT_DIR'] = result_dir

            # 執行任務
            process = subprocess.Popen(
                cmd,
                cwd=task_dir,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                text=True,
                bufsize=1,
                universal_newlines=True,
                env=env
            )

            # 實時讀取輸出
            output_lines = []
            start_time = time.time()
            timeout = 3600  # 1小時超時

            while True:
                if process.poll() is not None:
                    break

                if time.time() - start_time > timeout:
                    process.kill()
                    self.worker_node._log(f"任務 {task_id} 執行超時，已終止")
                    return False, "任務執行超時"

                try:
                    line = process.stdout.readline()
                    if line:
                        line = line.strip()
                        output_lines.append(line)
                        self.worker_node._log(f"[任務輸出] {line}")
                except Exception:
                    break

            # 等待進程完成
            return_code = process.wait()

            # 讀取剩餘輸出
            remaining_output, _ = process.communicate()
            if remaining_output:
                for line in remaining_output.strip().split('\n'):
                    if line.strip():
                        output_lines.append(line.strip())
                        self.worker_node._log(f"[任務輸出] {line.strip()}")

            if return_code == 0:
                self.worker_node._log(f"任務 {task_id} 執行成功完成")
                return True, "任務執行成功"
            else:
                error_msg = f"任務執行失敗，退出代碼: {return_code}"
                self.worker_node._log(f"任務 {task_id} {error_msg}")
                return False, error_msg

        except FileNotFoundError as e:
            error_msg = f"找不到執行檔案: {e}"
            self.worker_node._log(f"任務 {task_id} 執行失敗: {error_msg}")
            return False, error_msg
        except Exception as e:
            error_msg = f"任務執行異常: {e}"
            self.worker_node._log(f"任務 {task_id} 執行失敗: {error_msg}")
            return False, error_msg

    def TaskOutputUpload(self, request, context):
        """任務輸出上傳"""
        try:
            task_id = request.task_id
            output = request.output
            # token = request.token  # TODO: 驗證 token

            # 處理任務輸出（目前本地記錄）
            self.worker_node._send_task_logs(task_id, output)

            return nodepool_pb2.TaskOutputUploadResponse(
                success=True,
                message="Output uploaded successfully"
            )
        except Exception as e:
            self.worker_node._log(f"TaskOutputUpload error: {e}")
            return nodepool_pb2.TaskOutputUploadResponse(
                success=False,
                message=f"Upload failed: {str(e)}"
            )

    def TaskResultUpload(self, request, context):
        """任務結果上傳"""
        try:
            # task_id = request.task_id
            # result_zip = request.result_zip
            # token = request.token
            # TODO: 實現結果上傳邏輯
            return nodepool_pb2.TaskResultUploadResponse(
                success=True,
                message="Result uploaded successfully"
            )
        except Exception as e:
            self.worker_node._log(f"TaskResultUpload error: {e}")
            return nodepool_pb2.TaskResultUploadResponse(
                success=False,
                message=f"Upload failed: {str(e)}"
            )

    def TaskOutput(self, request, context):
        """任務輸出處理"""
        try:
            task_id = request.task_id
            output = request.output
            # token = request.token  # TODO: 驗證 token

            self.worker_node._log(f"Task {task_id} output: {output}")
            return nodepool_pb2.TaskOutputResponse(
                success=True,
                message="Output processed successfully"
            )
        except Exception as e:
            self.worker_node._log(f"TaskOutput error: {e}")
            return nodepool_pb2.TaskOutputResponse(
                success=False,
                message=f"Processing failed: {str(e)}"
            )

    def TaskUsage(self, request, context):
        """任務資源使用情況報告"""
        try:
            task_id = request.task_id
            cpu_usage = request.cpu_usage
            memory_usage = request.memory_usage
            gpu_usage = request.gpu_usage
            gpu_memory_usage = request.gpu_memory_usage
            # token = request.token  # TODO: 驗證 token

            # 更新任務資源使用情況
            with self.worker_node.resources_lock:
                if task_id in self.worker_node.running_tasks:
                    self.worker_node.running_tasks[task_id].update({
                        "cpu_usage": cpu_usage,
                        "memory_usage": memory_usage,
                        "gpu_usage": gpu_usage,
                        "gpu_memory_usage": gpu_memory_usage,
                    })

            self.worker_node._log(
                f"Task {task_id} usage updated: CPU={cpu_usage}%, Memory={memory_usage}%, "
                f"GPU={gpu_usage}%, GPU_Memory={gpu_memory_usage}%"
            )

            return nodepool_pb2.TaskUsageResponse(
                success=True,
                message="Usage reported successfully"
            )
        except Exception as e:
            self.worker_node._log(f"TaskUsage error: {e}")
            return nodepool_pb2.TaskUsageResponse(
                success=False,
                message=f"Report failed: {str(e)}"
            )
