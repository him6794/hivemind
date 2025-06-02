import grpc
import logging
import nodepool_pb2
import nodepool_pb2_grpc
from concurrent.futures import ThreadPoolExecutor
import subprocess
import os
import zipfile
import io
import time
import threading
import json

logging.basicConfig(level=logging.INFO, format='%(asctime)s - %(levelname)s - %(message)s')

class WorkerNodeServiceServicer(nodepool_pb2_grpc.WorkerNodeServiceServicer):
    def __init__(self, node_id, master_channel):
        self.node_id = node_id
        self.task_executor = ThreadPoolExecutor(max_workers=1)
        self.status = "空閒狀態"
        self.current_task_id = None
        self.master_channel = master_channel
        self.master_stub = nodepool_pb2_grpc.MasterNodeServiceStub(self.master_channel)
        self.node_stub = nodepool_pb2_grpc.NodeManagerServiceStub(self.master_channel)
        self.cpt_reward = 0  # Initialize reward
        self.logs_buffer = []
        self.logs_lock = threading.Lock()
        self.start_status_reporter()
        self.start_logs_reporter()

    def start_status_reporter(self):
        """啟動一個後台線程，定期報告節點狀態"""
        def report_status():
            while True:
                try:
                    self.node_stub.ReportStatus(nodepool_pb2.ReportStatusRequest(
                        node_id=self.node_id,
                        status=self.status
                    ))
                    logging.info(f"節點 {self.node_id} 報告狀態: {self.status}")
                except grpc.RpcError as e:
                    logging.error(f"節點 {self.node_id} 報告狀態失敗: {e.details()}")
                time.sleep(5)

        status_thread = threading.Thread(target=report_status, daemon=True)
        status_thread.start()

    def start_logs_reporter(self):
        """啟動日誌回報線程"""
        def report_logs():
            while True:
                try:
                    with self.logs_lock:
                        if self.logs_buffer:
                            self.master_stub.StoreLogs(nodepool_pb2.StoreLogsRequest(
                                node_id=self.node_id,
                                task_id=self.current_task_id,
                                logs=json.dumps(self.logs_buffer),
                                timestamp=int(time.time())
                            ))
                            self.logs_buffer = []
                except Exception as e:
                    logging.error(f"回報日誌失敗: {e}")
                time.sleep(5)

        reporter = threading.Thread(target=report_logs, daemon=True)
        reporter.start()

    def add_log(self, message, level="INFO"):
        """添加新的日誌"""
        log_entry = {
            'timestamp': time.time(),
            'message': message,
            'level': level
        }
        self.logs_buffer.append(log_entry)

    def _log_and_buffer(self, message, level=logging.INFO):
        """記錄日誌並添加到緩衝區"""
        timestamp = time.time()
        with self.logs_lock:
            self.logs_buffer.append({
                'timestamp': timestamp,
                'message': message,
                'level': logging.getLevelName(level),
                'node_id': self.node_id,
                'task_id': self.current_task_id
            })

    def ExecuteTask(self, request, context):
        """執行分配的運算任務"""
        if self.status != "空閒狀態":
            logging.error(f"節點 {self.node_id} 正在執行其他任務，無法接受新任務 {request.task_id}")
            self.add_log(f"節點正在執行其他任務，無法接受新任務 {request.task_id}", level="ERROR")
            return nodepool_pb2.ExecuteTaskResponse(
                success=False,
                message="節點正在執行其他任務",
                result=b""
            )

        logging.info(f"節點 {self.node_id} 開始執行任務 {request.task_id}")
        self.add_log(f"開始執行任務 {request.task_id}")
        self._log_and_buffer(f"開始執行任務 {request.task_id}")
        self.status = "執行中"
        self.cpt_reward += 10
        self.current_task_id = request.task_id
        self.report_status()

        try:
            # 1. 將 task_zip 解壓到臨時目錄
            logging.info(f"解壓任務 {request.task_id} 的 ZIP 檔案")
            self.add_log(f"解壓任務 {request.task_id} 的 ZIP 檔案")
            self._log_and_buffer(f"解壓任務 {request.task_id} 的 ZIP 檔案")
            task_dir = f"task_{request.task_id}"
            os.makedirs(task_dir, exist_ok=True)
            with zipfile.ZipFile(io.BytesIO(request.task_zip), "r") as zip_ref:
                zip_ref.extractall(task_dir)

            # 2. 安裝依賴（假設有 requirements.txt）
            logging.info(f"檢查並安裝任務 {request.task_id} 的依賴")
            self.add_log(f"檢查並安裝任務 {request.task_id} 的依賴")
            self._log_and_buffer(f"檢查並安裝任務 {request.task_id} 的依賴")
            requirements_file = os.path.join(task_dir, "requirements.txt")
            if os.path.exists(requirements_file):
                subprocess.run(["pip", "install", "-r", requirements_file], check=True)

            # 3. 執行任務腳本（假設主腳本為 task_script.py）
            logging.info(f"執行任務 {request.task_id} 的腳本")
            self.add_log(f"執行任務 {request.task_id} 的腳本")
            self._log_and_buffer(f"執行任務 {request.task_id} 的腳本")
            script_file = os.path.join(task_dir, "task_script.py")
            if not os.path.exists(script_file):
                raise FileNotFoundError(f"任務腳本 {script_file} 不存在")

            # 執行腳本並捕獲輸出
            process = subprocess.Popen(
                ["python", script_file],
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True
            )

            # 4. 實時讀取輸出並報告中途輸出
            output_lines = []
            while process.poll() is None:
                line = process.stdout.readline().strip()
                if line:
                    output_lines.append(line)
                    self.master_stub.StoreOutput(nodepool_pb2.StoreOutputRequest(
                        task_id=request.task_id,
                        output=line
                    ))
                    logging.info(f"任務 {request.task_id} 中途輸出: {line}")
                    self.add_log(f"任務 {request.task_id} 中途輸出: {line}")
                    self._log_and_buffer(f"任務 {request.task_id} 中途輸出: {line}")
                time.sleep(0.1)

            # 5. 獲取最終輸出
            logging.info(f"獲取任務 {request.task_id} 的最終輸出")
            self.add_log(f"獲取任務 {request.task_id} 的最終輸出")
            self._log_and_buffer(f"獲取任務 {request.task_id} 的最終輸出")
            stdout, stderr = process.communicate()
            if stderr:
                logging.error(f"任務 {request.task_id} 執行錯誤: {stderr}")
                self.add_log(f"任務 {request.task_id} 執行錯誤: {stderr}", level="ERROR")
                self._log_and_buffer(f"任務 {request.task_id} 執行錯誤: {stderr}", logging.ERROR)
                raise RuntimeError(stderr)
            output_lines.extend(stdout.splitlines())
            final_output = "\n".join(output_lines)
            logging.info(f"任務 {request.task_id} 最終輸出: {final_output}")
            self.add_log(f"任務 {request.task_id} 最終輸出: {final_output}")
            self._log_and_buffer(f"任務 {request.task_id} 最終輸出: {final_output}")

            # 6. 將結果壓縮為 ZIP
            logging.info(f"壓縮任務 {request.task_id} 的結果")
            self.add_log(f"壓縮任務 {request.task_id} 的結果")
            self._log_and_buffer(f"壓縮任務 {request.task_id} 的結果")
            result_zip_path = f"result_{request.task_id}.zip"
            with zipfile.ZipFile(result_zip_path, "w", zipfile.ZIP_DEFLATED) as zip_ref:
                for root, _, files in os.walk(task_dir):
                    for file in files:
                        file_path = os.path.join(root, file)
                        arcname = os.path.relpath(file_path, task_dir)
                        zip_ref.write(file_path, arcname)

            # 7. 讀取結果 ZIP 並上傳
            logging.info(f"上傳任務 {request.task_id} 的結果")
            self.add_log(f"上傳任務 {request.task_id} 的結果")
            self._log_and_buffer(f"上傳任務 {request.task_id} 的結果")
            with open(result_zip_path, "rb") as f:
                result_zip = f.read()
            store_response = self.master_stub.StoreResult(nodepool_pb2.StoreResultRequest(
                task_id=request.task_id,
                result_zip=result_zip
            ))
            if not store_response.success:
                raise RuntimeError(f"上傳任務 {request.task_id} 結果失敗: {store_response.message}")

            # 8. 通知主控端任務完成
            logging.info(f"通知主控端任務 {request.task_id} 已完成")
            self.add_log(f"通知主控端任務 {request.task_id} 已完成")
            self._log_and_buffer(f"通知主控端任務 {request.task_id} 已完成")
            completion_response = self.master_stub.TaskCompleted(nodepool_pb2.TaskCompletedRequest(
                task_id=request.task_id,
                node_id=self.node_id,
                success=True
            ))
            if not completion_response.success:
                logging.warning(f"通知主控端任務完成失敗: {completion_response.message}")
                self.add_log(f"通知主控端任務完成失敗: {completion_response.message}", level="WARNING")
                self._log_and_buffer(f"通知主控端任務完成失敗: {completion_response.message}", logging.WARNING)

            # 9. 清理臨時檔案
            logging.info(f"清理任務 {request.task_id} 的臨時檔案")
            self.add_log(f"清理任務 {request.task_id} 的臨時檔案")
            self._log_and_buffer(f"清理任務 {request.task_id} 的臨時檔案")
            for root, dirs, files in os.walk(task_dir, topdown=False):
                for file in files:
                    os.remove(os.path.join(root, file))
                for dir in dirs:
                    os.rmdir(os.path.join(root, dir))
            os.rmdir(task_dir)
            os.remove(result_zip_path)

            # 10. 更新節點狀態
            logging.info(f"任務 {request.task_id} 執行完成，更新節點狀態")
            self.add_log(f"任務 {request.task_id} 執行完成，更新節點狀態")
            self._log_and_buffer(f"任務 {request.task_id} 執行完成，更新節點狀態")
            self.status = "空閒狀態"
            self.cpt_reward = 0
            self.current_task_id = None
            self.report_status()
            logging.info(f"節點 {self.node_id} 完成任務 {request.task_id}，恢復空閒狀態")
            self.add_log(f"節點 {self.node_id} 完成任務 {request.task_id}，恢復空閒狀態")
            self._log_and_buffer(f"節點 {self.node_id} 完成任務 {request.task_id}，恢復空閒狀態")

            return nodepool_pb2.ExecuteTaskResponse(
                success=True,
                message=f"任務 {request.task_id} 執行成功",
                result=result_zip
            )

        except Exception as e:
            logging.error(f"任務 {request.task_id} 執行錯誤: {e}")
            self.add_log(f"任務 {request.task_id} 執行錯誤: {e}", level="ERROR")
            self._log_and_buffer(f"任務 {request.task_id} 執行錯誤: {e}", logging.ERROR)
            # 通知主控端任務失敗
            try:
                self.master_stub.TaskCompleted(nodepool_pb2.TaskCompletedRequest(
                    task_id=request.task_id,
                    node_id=self.node_id,
                    success=False
                ))
            except Exception as notify_error:
                logging.error(f"通知主控端任務失敗時發生錯誤: {notify_error}")
                self.add_log(f"通知主控端任務失敗時發生錯誤: {notify_error}", level="ERROR")
                self._log_and_buffer(f"通知主控端任務失敗時發生錯誤: {notify_error}", logging.ERROR)

            self.status = "空閒狀態"
            self.cpt_reward = 0
            self.current_task_id = None
            self.report_status()
            logging.info(f"節點 {self.node_id} 因錯誤恢復空閒狀態")
            self.add_log(f"節點 {self.node_id} 因錯誤恢復空閒狀態")
            self._log_and_buffer(f"節點 {self.node_id} 因錯誤恢復空閒狀態")
            return nodepool_pb2.ExecuteTaskResponse(
                success=False,
                message=f"任務執行失敗: {str(e)}",
                result=b""
            )

    def ReportOutput(self, request, context):
        logging.info(f"節點 {request.node_id} 報告中途輸出: {request.output}")
        self.add_log(f"節點 {request.node_id} 報告中途輸出: {request.output}")
        return nodepool_pb2.StatusResponse(success=True, message="中途輸出已接收")

    def ReportRunningStatus(self, request, context):
        logging.info(f"節點 {self.node_id} 報告運行狀態")
        self.add_log(f"節點 {self.node_id} 報告運行狀態")
        cpt_reward =  self.cpt_reward if self.status == "執行中" else 0
        return nodepool_pb2.RunningStatusResponse(
            success=True,
            message="運行中" if self.status == "執行中" else "空閒",
            cpt_reward=cpt_reward
        )

    def report_status(self):
        try:
            self.node_stub.ReportStatus(nodepool_pb2.ReportStatusRequest(
                node_id=self.node_id,
                status=self.status
            ))
            logging.info(f"節點 {self.node_id} 報告狀態: {self.status}")
            self.add_log(f"節點 {self.node_id} 報告狀態: {self.status}")
        except grpc.RpcError as e:
            logging.error(f"節點 {self.node_id} 報告狀態失敗: {e.details()}")
            self.add_log(f"節點 {self.node_id} 報告狀態失敗: {e.details()}", level="ERROR")