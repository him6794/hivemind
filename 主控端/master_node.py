import grpc
import os
import time
import logging
import zipfile
import datetime
from concurrent.futures import ThreadPoolExecutor, as_completed
import nodepool_pb2
import nodepool_pb2_grpc
import threading
from flask import Flask, jsonify, request, render_template, redirect, url_for, flash
import io
from functools import wraps
import uuid

# --- Configuration ---
GRPC_SERVER_ADDRESS = os.environ.get('GRPC_SERVER_ADDRESS', '127.0.0.1:50051')
FLASK_SECRET_KEY = os.environ.get('FLASK_SECRET_KEY', 'a-default-master-secret-key')
MASTER_USERNAME = os.environ.get('MASTER_USERNAME', 'test')
MASTER_PASSWORD = os.environ.get('MASTER_PASSWORD', 'password')
REWARD_INTERVAL_SECONDS = int(os.environ.get('REWARD_INTERVAL_SECONDS', 60))
REWARD_TRANSFER_RETRIES = int(os.environ.get('REWARD_TRANSFER_RETRIES', 3))
REWARD_RETRY_DELAY = float(os.environ.get('REWARD_RETRY_DELAY', 5.0))
UI_HOST = '0.0.0.0'
UI_PORT = 5001
logging.basicConfig(level=logging.INFO, format='%(asctime)s - %(levelname)s - [%(threadName)s] - %(message)s')

class MasterNodeUI:
    def __init__(self, username, password, grpc_address):
        self.username = username
        self.password = password
        self.grpc_address = grpc_address
        self.channel = None
        self.user_stub = None
        self.master_stub = None
        self.node_stub = None
        self.token = None  # 只在 MasterNodeUI 實例內部維護
        self.reward_thread = None
        self.task_poll_thread = None
        self.auto_retrieve_thread = None
        self._stop_event = threading.Event()
        self.task_status_cache = {}
        self.task_cache_lock = threading.Lock()
        self.poll_interval = 5

        self.app = Flask(__name__, template_folder="templates_master", static_folder="static_master")
        self.setup_flask_routes()
        
        # 配置Flask應用，避免與工作端衝突
        self.app.secret_key = FLASK_SECRET_KEY
        self.app.config.update(
            # 使用不同的session cookie名稱，避免與工作端衝突
            SESSION_COOKIE_NAME='master_session',
            SESSION_COOKIE_SECURE=False,
            SESSION_COOKIE_HTTPONLY=True,
            SESSION_COOKIE_SAMESITE='Lax',
            SESSION_COOKIE_PATH='/',
            SESSION_COOKIE_DOMAIN=None,  # 確保cookie域名不衝突
            PERMANENT_SESSION_LIFETIME=datetime.timedelta(hours=24),  # 24小時會話
            # 添加這個import
            SESSION_REFRESH_EACH_REQUEST=True  # 每次請求刷新會話
        )
        
        self.task_status_updater = TaskStatusUpdater(self)
        self.task_status_updater.start()

        # 用戶會話管理 - 存在後端陣列，不存在瀏覽器
        self.user_list = []  # [{username, token, cpt_balance, login_time}]
        self.user_list_lock = threading.Lock()

    def add_or_update_user(self, username, token):
        with self.user_list_lock:
            for user in self.user_list:
                if user['username'] == username:
                    user['token'] = token
                    user['login_time'] = datetime.datetime.now()
                    return
            self.user_list.append({
                'username': username,
                'token': token,
                'cpt_balance': 0,
                'login_time': datetime.datetime.now()
            })

    def get_user(self, username):
        with self.user_list_lock:
            for user in self.user_list:
                if user['username'] == username:
                    return user
        return None

    def remove_user(self, username):
        with self.user_list_lock:
            self.user_list = [u for u in self.user_list if u['username'] != username]

    def get_balance(self, username):
        user = self.get_user(username)
        if not user:
            return 0
        try:
            req = nodepool_pb2.GetBalanceRequest(username=username, token=user['token'])
            resp = self.user_stub.GetBalance(req, timeout=30)
            if resp.success:
                user['cpt_balance'] = resp.balance
                return resp.balance
            else:
                return 0
        except Exception:
            return 0

    def get_tasks(self, username):
        user = self.get_user(username)
        if not user:
            return []
        try:
            req = nodepool_pb2.GetAllTasksRequest(token=user['token'])
            resp = self.master_stub.GetAllTasks(req, timeout=30)
            if resp.success:
                return resp.tasks
            else:
                return []
        except Exception:
            return []

    def _connect_grpc(self):
        try:
            self.channel = grpc.insecure_channel(self.grpc_address)
            grpc.channel_ready_future(self.channel).result(timeout=10)
            self.user_stub = nodepool_pb2_grpc.UserServiceStub(self.channel)
            self.master_stub = nodepool_pb2_grpc.MasterNodeServiceStub(self.channel)
            self.node_stub = nodepool_pb2_grpc.NodeManagerServiceStub(self.channel)
            logging.info(f"Successfully connected to gRPC server at {self.grpc_address}")
            return True
        except grpc.FutureTimeoutError:
            logging.error(f"Timeout connecting to gRPC server at {self.grpc_address}")
            return False
        except Exception as e:
            logging.error(f"Failed to connect to gRPC server: {e}", exc_info=True)
            self.channel = None
            return False

    def login(self, username=None, password=None):
        # 支援外部傳入帳密
        username = username or self.username
        password = password or self.password
        if not self.channel or not self.user_stub:
            logging.error("gRPC connection not established. Cannot login.")
            return False

        request = nodepool_pb2.LoginRequest(username=username, password=password)
        try:
            response = self.user_stub.Login(request, timeout=15)
            logging.info(f"LoginResponse: success={response.success}, token={response.token}")
            if response.success and response.token:
                self.add_or_update_user(username, response.token)
                # 若是自己登入，也更新 self.token
                if username == self.username:
                    self.token = response.token
                    logging.info(f"MasterNodeUI self.token set: {self.token}")
                return True
            else:
                return False
        except grpc.RpcError as e:
            logging.error(f"MasterNodeUI login gRPC error: {e.code()} - {e.details()}")
            return False
        except Exception as e:
            logging.error(f"MasterNodeUI login unexpected error: {e}", exc_info=True)
            return False

    def _get_grpc_metadata(self):
        # 只用 self.token，不用 session['token']
        if self.token:
            return [('authorization', f'Bearer {self.token}')]
        return None

    def _get_user_id_from_token(self):
        """從 token 中獲取用戶ID（修正版本）"""
        try:
            # 主控端始終使用自己的用戶名，不依賴session
            return self.username
            
        except Exception as e:
            logging.error(f"獲取用戶ID失敗: {e}")
            return self.username

    def ensure_authenticated(self):
        """確保主控端有有效的 token（簡化版本）"""
        return bool(self.token)

    def upload_task(self, task_id, task_zip_bytes, requirements):
        # 只用 self.token，不用 session['token']
        if not self.ensure_authenticated():
            logging.error("認證失敗，無法上傳任務")
            return task_id, False
        if not self.master_stub or not self.token:
            logging.error("Cannot upload task: Not connected or not logged in.")
            return task_id, False

        logging.info(f"Uploading task: {task_id}, Memory: {requirements.get('memory_gb')}GB, "
                     f"CPU: {requirements.get('cpu_score')}, GPU: {requirements.get('gpu_score')}, "
                     f"VRAM: {requirements.get('gpu_memory_gb')}GB, Loc: {requirements.get('location')}, "
                     f"GPU Name: {requirements.get('gpu_name', 'Any')}")
        try:
            # 從 token 中獲取用戶ID
            user_id = self._get_user_id_from_token()
            if not user_id:
                logging.error("無法從 token 獲取用戶ID")
                return task_id, False

            # 計算 cpt_cost，與節點池一致
            memory_gb_val = float(requirements.get("memory_gb", 0))
            cpu_score_val = float(requirements.get("cpu_score", 0))
            gpu_score_val = float(requirements.get("gpu_score", 0))
            gpu_memory_gb_val = float(requirements.get("gpu_memory_gb", 0))
            cpt_cost = memory_gb_val + cpu_score_val / 100 + gpu_score_val / 100 + gpu_memory_gb_val
            if cpt_cost < 1.0:
                cpt_cost = 1.0
            cpt_cost = int(cpt_cost)
            logging.info(f"任務 {task_id} 計算所需代幣: {cpt_cost} CPT")
            
            # 檢查用戶餘額是否足夠
            try:
                balance_request = nodepool_pb2.GetBalanceRequest(
                    username=user_id,
                    token=self.token
                )
                balance_response = self.user_stub.GetBalance(balance_request, timeout=30)  # 增加超時時間
                if balance_response.success:
                    user_balance = balance_response.balance
                    if user_balance < cpt_cost:
                        logging.error(f"用戶餘額不足: 需要 {cpt_cost} CPT，但只有 {user_balance} CPT")
                        return task_id, False
                else:
                    logging.error(f"無法獲取用戶餘額: {balance_response.message}")
                    return task_id, False
            except Exception as e:
                logging.error(f"檢查用戶餘額時發生錯誤: {e}")
                return task_id, False

            request = nodepool_pb2.UploadTaskRequest(
                task_id=task_id,
                task_zip=task_zip_bytes,
                memory_gb=int(requirements.get("memory_gb", 0)),
                cpu_score=int(requirements.get("cpu_score", 0)),
                gpu_score=int(requirements.get("gpu_score", 0)),
                gpu_memory_gb=int(requirements.get("gpu_memory_gb", 0)),
                location=requirements.get("location", "Any"),
                gpu_name=requirements.get("gpu_name", ""),
                user_id=user_id
            )
            response = self.master_stub.UploadTask(request, timeout=60)  # 增加超時時間
            if response.success:
                logging.info(f"Task {task_id} uploaded successfully: {response.message}")
                # 添加到任務緩存
                with self.task_cache_lock:
                    self.task_status_cache[task_id] = {
                        "task_id": task_id,
                        "status": "PENDING",
                        "message": "Task submitted",
                        "last_polled": time.time()
                    }
                return task_id, True
            else:
                logging.error(f"Task {task_id} upload failed on server: {response.message}")
                return task_id, False
        except grpc.RpcError as e:
            logging.error(f"Task {task_id} upload gRPC error: {e.details()}")
            return task_id, False
        except Exception as e:
            logging.error(f"Task {task_id} upload unexpected error: {e}", exc_info=True)
            return task_id, False

    def upload_task_with_user(self, username, task_id, task_zip_bytes, requirements):
        user = self.get_user(username)
        if not user:
            logging.error(f"找不到用戶 {username}，無法上傳任務")
            return task_id, False
        token = user['token']

        logging.info(f"Uploading task: {task_id}, Memory: {requirements.get('memory_gb')}GB, "
                     f"CPU: {requirements.get('cpu_score')}, GPU: {requirements.get('gpu_score')}, "
                     f"VRAM: {requirements.get('gpu_memory_gb')}GB, Loc: {requirements.get('location')}, "
                     f"GPU Name: {requirements.get('gpu_name', 'Any')}")
        try:
            # 計算 cpt_cost，與節點池一致
            memory_gb_val = float(requirements.get("memory_gb", 0))
            cpu_score_val = float(requirements.get("cpu_score", 0))
            gpu_score_val = float(requirements.get("gpu_score", 0))
            gpu_memory_gb_val = float(requirements.get("gpu_memory_gb", 0))
            cpt_cost = memory_gb_val + cpu_score_val / 100 + gpu_score_val / 100 + gpu_memory_gb_val
            if cpt_cost < 1.0:
                cpt_cost = 1.0
            cpt_cost = int(cpt_cost)
            logging.info(f"任務 {task_id} 計算所需代幣: {cpt_cost} CPT")
            
            # 檢查用戶餘額是否足夠
            try:
                balance_request = nodepool_pb2.GetBalanceRequest(
                    username=username,
                    token=token
                )
                balance_response = self.user_stub.GetBalance(balance_request, timeout=30)  # 增加超時時間
                if balance_response.success:
                    user['cpt_balance'] = balance_response.balance
                    if balance_response.balance < cpt_cost:
                        logging.error(f"用戶 {username} 餘額不足: 需要 {cpt_cost} CPT，但只有 {balance_response.balance} CPT")
                        return task_id, False
                else:
                    logging.error(f"無法獲取用戶 {username} 餘額: {balance_response.message}")
                    return task_id, False
            except Exception as e:
                logging.error(f"檢查用戶 {username} 餘額時發生錯誤: {e}")
                return task_id, False

            request = nodepool_pb2.UploadTaskRequest(
                task_id=task_id,
                task_zip=task_zip_bytes,
                memory_gb=int(requirements.get("memory_gb", 0)),
                cpu_score=int(requirements.get("cpu_score", 0)),
                gpu_score=int(requirements.get("gpu_score", 0)),
                gpu_memory_gb=int(requirements.get("gpu_memory_gb", 0)),
                location=requirements.get("location", "Any"),
                gpu_name=requirements.get("gpu_name", ""),
                user_id=username
            )
            metadata = [('authorization', f'Bearer {token}')]
            response = self.master_stub.UploadTask(request, metadata=metadata, timeout=60)
            if response.success:
                logging.info(f"Task {task_id} uploaded successfully: {response.message}")
                with self.task_cache_lock:
                    self.task_status_cache[task_id] = {
                        "task_id": task_id,
                        "status": "PENDING",
                        "message": "Task submitted",
                        "last_polled": time.time()
                    }
                return task_id, True
            else:
                logging.error(f"Task {task_id} upload failed on server: {response.message}")
                return task_id, False
        except grpc.RpcError as e:
            logging.error(f"Task {task_id} upload gRPC error: {e.details()}")
            return task_id, False
        except Exception as e:
            logging.error(f"Task {task_id} upload unexpected error: {e}", exc_info=True)
            return task_id, False

    def poll_task_status(self, task_id):
        if not self.master_stub:
            logging.error("Cannot poll task status: Not connected.")
            return {"status": "ERROR", "message": "Not connected"}

        if not self.ensure_authenticated():
            logging.error("Authentication failed, cannot poll task status")
            return {"status": "ERROR", "message": "Authentication failed"}

        try:
            request = nodepool_pb2.PollTaskStatusRequest(task_id=task_id)
            response = self.master_stub.PollTaskStatus(request, metadata=self._get_grpc_metadata(), timeout=30)  # 增加超時時間
            logging.debug(f"Polled task {task_id} status: {response.status}")
            with self.task_cache_lock:
                self.task_status_cache[task_id] = {
                    "task_id": task_id,
                    "status": response.status,
                    "output_tail": response.output[-5:] if response.output else [],
                    "message": response.message,
                    "last_polled": time.time(),
                    "progress": "100%" if response.status == "COMPLETED" else "0%"
                }
            return self.task_status_cache[task_id]
        except grpc.RpcError as e:
            logging.error(f"Polling task {task_id} gRPC error: {e.details()}")
            return {"status": "ERROR", "message": f"gRPC Error: {e.details()}"}
        except Exception as e:
            logging.error(f"Polling task {task_id} unexpected error: {e}", exc_info=True)
            return {"status": "ERROR", "message": f"Unexpected Error: {e}"}

    def get_task_result(self, task_id, download_path):
        if not self.master_stub:
            logging.error("Cannot get task result: Not connected.")
            return False
        try:
            # 使用主控端的token
            request = nodepool_pb2.GetTaskResultRequest(
                task_id=task_id,
                token=self.token  # 只用 self.token
            )
            response = self.master_stub.GetTaskResult(request, timeout=60)
            if response.success and response.result_zip:
                os.makedirs(os.path.dirname(download_path), exist_ok=True)
                with open(download_path, "wb") as f:
                    f.write(response.result_zip)
                logging.info(f"Task {task_id} result saved to {download_path}")
                return True
            else:
                logging.error(f"Failed to get task {task_id} result: {response.message}")
                return False
        except grpc.RpcError as e:
            logging.error(f"Getting task {task_id} result gRPC error: {e.details()}")
            return False
        except Exception as e:
            logging.error(f"Getting task {task_id} result unexpected error: {e}", exc_info=True)
            return False

    def distribute_rewards(self):
        logging.info("Reward distribution thread started.")
        while not self._stop_event.is_set():
            if not self.node_stub or not self.user_stub or not self.token:
                logging.warning("Reward distribution skipped: Not connected or not logged in.")
                self._stop_event.wait(REWARD_INTERVAL_SECONDS)
                continue

            logging.debug("Starting reward distribution cycle...")
            try:
                list_request = nodepool_pb2.GetNodeListRequest()
                nodes_response = self.node_stub.GetNodeList(list_request, timeout=15)
                if not nodes_response.success:
                    logging.warning(f"Could not get node list for rewards: {nodes_response.message}")
                    self._stop_event.wait(REWARD_INTERVAL_SECONDS)
                    continue

                nodes_to_reward = [node for node in nodes_response.nodes if node.port > 0]
                logging.debug(f"Found {len(nodes_to_reward)} eligible nodes for reward processing.")

                for node in nodes_to_reward:
                    if self._stop_event.is_set():
                        break
                    reward_amount = 0
                    worker_channel = None
                    try:
                        node_address = f'127.0.0.1:{node.port}'
                        worker_channel = grpc.insecure_channel(node_address)
                        grpc.channel_ready_future(worker_channel).result(timeout=2)
                        worker_stub = nodepool_pb2_grpc.WorkerNodeServiceStub(worker_channel)
                        status_req = nodepool_pb2.RunningStatusRequest(node_id=node.node_id, task_id="")
                        status_response = worker_stub.ReportRunningStatus(status_req, timeout=3)

                        if status_response.success and status_response.cpt_reward > 0:
                            reward_amount = status_response.cpt_reward
                            logging.debug(f"Node {node.node_id} eligible for {reward_amount} CPT reward.")
                            for attempt in range(REWARD_TRANSFER_RETRIES):
                                transfer_success, transfer_message = self.transfer_tokens(node.node_id, reward_amount)
                                if transfer_success:
                                    logging.info(f"Transferred {reward_amount} CPT to node {node.node_id}: {transfer_message}")
                                    break
                                else:
                                    logging.warning(f"Attempt {attempt + 1}/{REWARD_TRANSFER_RETRIES} failed for node {node.node_id}: {transfer_message}")
                                    if attempt < REWARD_TRANSFER_RETRIES - 1:
                                        time.sleep(REWARD_RETRY_DELAY)
                            else:
                                logging.error(f"Failed to transfer {reward_amount} CPT to node {node.node_id} after {REWARD_TRANSFER_RETRIES} attempts.")
                        else:
                            logging.debug(f"Node {node.node_id} not eligible for reward: {status_response.message}")

                    except grpc.FutureTimeoutError:
                        logging.warning(f"Node {node.node_id} at {node_address} is unreachable (timeout).")
                    except grpc.RpcError as e:
                        logging.error(f"Error communicating with node {node.node_id}: {e.code()} - {e.details()}")
                    except Exception as e:
                        logging.error(f"Unexpected error processing reward for node {node.node_id}: {e}", exc_info=True)
                    finally:
                        if worker_channel:
                            worker_channel.close()

            except grpc.RpcError as e:
                logging.error(f"Error getting node list for rewards: {e.details()}")
            except Exception as e:
                logging.error(f"Unexpected error during reward distribution: {e}", exc_info=True)

            logging.debug("Reward distribution cycle finished.")
            self._stop_event.wait(REWARD_INTERVAL_SECONDS)

        logging.info("Reward distribution thread stopped.")

    def transfer_tokens(self, receiver_username, amount):
        if not self.user_stub or not self.token:
            return False, "Master not logged in or connected"
        if amount <= 0:
            return False, "Amount must be positive"

        try:
            request = nodepool_pb2.TransferRequest(
                token=self.token,
                receiver_username=receiver_username,
                amount=int(amount)
            )
            response = self.user_stub.Transfer(request, timeout=15)
            return response.success, response.message
        except grpc.RpcError as e:
            logging.error(f"Token transfer gRPC error to {receiver_username}: {e.details()}")
            return False, f"gRPC Error: {e.details()}"
        except Exception as e:
            logging.error(f"Token transfer unexpected error to {receiver_username}: {e}", exc_info=True)
            return False, f"Unexpected Error: {e}"

    def clean_expired_task_cache(self):
        """清理過期的任務狀態緩存"""
        CACHE_EXPIRY_TIME = 3600  # 1小時後過期
        current_time = time.time()
        with self.task_cache_lock:
            expired_tasks = [
                task_id for task_id, info in self.task_status_cache.items()
                if current_time - info.get("last_polled", 0) > CACHE_EXPIRY_TIME
            ]
            for task_id in expired_tasks:
                del self.task_status_cache[task_id]

    def request_task_status_and_retrieve(self, task_id):
        """
        向節點池發送特定任務的狀態請求，並在完成時自動下載結果檔案
        
        Args:
            task_id: 要請求狀態的任務ID
        
        Returns:
            dict: 包含任務狀態和下載路徑(如果完成)的字典
        """
        if not self.ensure_authenticated():
            logging.error("認證失敗，無法請求任務狀態")
            return {"success": False, "status": "ERROR", "message": "Authentication failed"}
        
        try:
            # 請求任務狀態
            status_info = self.poll_task_status(task_id)
            logging.info(f"任務 {task_id} 目前狀態: {status_info.get('status')}")
            
            # 如果任務已完成，自動下載結果
            if status_info.get("status") == "COMPLETED":
                download_dir = os.path.join("results", task_id)
                os.makedirs(download_dir, exist_ok=True)
                download_path = os.path.join(download_dir, f"{task_id}_result.zip")
                
                logging.info(f"任務已完成，正在下載結果至 {download_path}")
                if self.get_task_result(task_id, download_path):
                    status_info["download_path"] = download_path
                    status_info["message"] += f" Results downloaded to {download_path}"
                    
                    # 嘗試自動解壓縮結果檔案
                    try:
                        extract_dir = os.path.join(download_dir, "extracted")
                        os.makedirs(extract_dir, exist_ok=True)
                        with zipfile.ZipFile(download_path, 'r') as zip_ref:
                            zip_ref.extractall(extract_dir)
                        status_info["extract_path"] = extract_dir
                        status_info["message"] += f" and extracted to {extract_dir}"
                        logging.info(f"結果檔案已解壓縮至 {extract_dir}")
                    except Exception as e:
                        logging.warning(f"解壓縮結果檔案失敗: {e}")
                else:
                    status_info["message"] += " Failed to download results."
                    logging.error(f"下載任務 {task_id} 結果失敗")
            
            return status_info
            
        except Exception as e:
            logging.error(f"請求任務狀態時發生錯誤: {e}", exc_info=True)
            return {"success": False, "status": "ERROR", "message": f"Error: {str(e)}"}

    def auto_retrieve_completed_tasks(self):
        """
        自動檢查所有未完成的任務，並在任務完成時下載結果
        """
        logging.info("Auto task retrieval thread started.")
        
        while not self._stop_event.is_set():
            if not self.master_stub or not self.token:
                logging.warning("Auto retrieval skipped: Not connected or not logged in.")
                self._stop_event.wait(30)  # 每30秒檢查一次
                continue
            
            try:
                # 從緩存中獲取所有任務
                with self.task_cache_lock:
                    pending_tasks = {
                        task_id: info for task_id, info in self.task_status_cache.items()
                        if info.get("task_id") # 確保這是一個任務，不是節點
                        and info.get("status") not in ["COMPLETED", "FAILED", "ERROR", "STOPPED"]
                    }
                
                if pending_tasks:
                    logging.debug(f"檢查 {len(pending_tasks)} 個待處理任務的狀態")
                    
                    for task_id in pending_tasks.keys():
                        if self._stop_event.is_set():
                            break
                        
                        try:
                            # 請求任務狀態
                            status_info = self.poll_task_status(task_id)
                            logging.info(f"任務 {task_id} 目前狀態: {status_info.get('status')}")
                            
                            # 如果任務完成，下載結果
                            if status_info.get("status") == "COMPLETED":
                                download_dir = os.path.join("results", task_id)
                                os.makedirs(download_dir, exist_ok=True)
                                download_path = os.path.join(download_dir, f"{task_id}_result.zip")
                                
                                logging.info(f"任務已完成，正在下載結果至 {download_path}")
                                if self.get_task_result(task_id, download_path):
                                    logging.info(f"任務 {task_id} 結果下載成功")
                                    
                                    # 嘗試解壓結果
                                    try:
                                        extract_dir = os.path.join(download_dir, "extracted")
                                        os.makedirs(extract_dir, exist_ok=True)
                                        with zipfile.ZipFile(download_path, 'r') as zip_ref:
                                            zip_ref.extractall(extract_dir)
                                        logging.info(f"任務 {task_id} 結果已解壓到 {extract_dir}")
                                    except Exception as e:
                                        logging.warning(f"解壓縮任務 {task_id} 結果失敗: {e}")
                                else:
                                    logging.error(f"下載任務 {task_id} 結果失敗")
                        except Exception as e:
                            logging.error(f"處理任務 {task_id} 時發生錯誤: {e}")
                        
                        # 避免過於頻繁請求
                        time.sleep(2)
            
            except Exception as e:
                logging.error(f"自動檢索任務結果時發生錯誤: {e}")
            
            # 等待下一次檢查
            self._stop_event.wait(30)
        
        logging.info("Auto task retrieval thread stopped.")

    def _create_user_session(self, username, token):
        """創建用戶會話"""
        session_id = str(uuid.uuid4())
        session_data = {
            'username': username,
            'token': token,
            'login_time': datetime.datetime.now(),
            'cpt_balance': 0,
            'created_at': time.time()
        }
        
        with self.session_lock:
            self.user_sessions[session_id] = session_data
        
        return session_id

    def _get_user_session(self, session_id):
        """根據會話ID獲取用戶資料"""
        with self.session_lock:
            return self.user_sessions.get(session_id)

    def _update_session_balance(self, session_id, balance):
        """更新會話中的餘額"""
        with self.session_lock:
            if session_id in self.user_sessions:
                self.user_sessions[session_id]['cpt_balance'] = balance

    def _clear_user_session(self, session_id):
        """清除用戶會話"""
        with self.session_lock:
            if session_id in self.user_sessions:
                del self.user_sessions[session_id]

    def setup_flask_routes(self):
        @self.app.route('/login', methods=['GET', 'POST'])
        def login():
            if request.method == 'POST':
                username = request.form.get('username')
                password = request.form.get('password')
                if not self.channel or not self.user_stub:
                    flash('主控端未連接到節點池，請稍後再試。', 'error')
                    return render_template('login.html')
                if self.login(username, password):
                    flash('Login successful!', 'success')
                    return redirect(url_for('index') + f"?user={username}")
                else:
                    flash('Invalid username or password', 'error')
            return render_template('login.html')

        @self.app.route('/logout')
        def logout():
            username = request.args.get('user')
            if username:
                self.remove_user(username)
            flash('You have been logged out.', 'success')
            return redirect(url_for('login'))

        @self.app.route('/')
        def index():
            username = request.args.get('user')
            if not username or not self.get_user(username):
                return redirect(url_for('login'))
            return render_template('master_dashboard.html', username=username)

        @self.app.route('/api/balance')
        def api_balance():
            username = request.args.get('user')
            if not username or not self.get_user(username):
                return jsonify({"error": "請先登入", "cpt_balance": 0}), 401
            balance = self.get_balance(username)
            return jsonify({"cpt_balance": balance})

        @self.app.route('/api/tasks')
        def api_tasks():
            username = request.args.get('user')
            if not username or not self.get_user(username):
                return jsonify({"error": "請先登入", "tasks": []}), 401
            tasks = self.get_tasks(username)
            task_list = []
            for task in tasks:
                created_time = ""
                if getattr(task, "created_at", None):
                    try:
                        created_timestamp = float(task.created_at)
                        created_time = time.strftime('%H:%M:%S', time.localtime(created_timestamp))
                    except:
                        created_time = "未知"
                task_list.append({
                    "task_id": task.task_id,
                    "status": task.status,
                    "progress": "100%" if task.status == "COMPLETED" else "50%" if task.status == "RUNNING" else "0%",
                    "message": f"狀態: {task.status}",
                    "last_update": created_time
                })
            return jsonify({"tasks": task_list})

        @self.app.route('/api/nodes')
        def api_nodes():
            username = request.args.get('user')
            if not username or not self.get_user(username):
                return jsonify({"error": "請先登入", "nodes": []}), 401
            if not self.node_stub:
                return jsonify({"error": "Not connected to gRPC server", "nodes": []}), 200
            try:
                grpc_request = nodepool_pb2.GetNodeListRequest()
                response = self.node_stub.GetNodeList(grpc_request, timeout=30)
                if response.success:
                    nodes_list = []
                    for node in response.nodes:
                        status = "ONLINE" if node.status else "OFFLINE"
                        nodes_list.append({
                            "node_id": node.node_id,
                            "status": status,
                            "cpu_cores": node.cpu_cores,
                            "memory_gb": node.memory_gb,
                            "cpu_score": node.cpu_score,
                            "gpu_score": node.gpu_score,
                            "last_heartbeat": time.strftime('%H:%M:%S', time.localtime(int(node.last_heartbeat))) if node.last_heartbeat else 'N/A',
                        })
                    return jsonify({"nodes": nodes_list})
                else:
                    return jsonify({"error": f"Failed to get node list: {response.message}", "nodes": []}), 200
            except grpc.RpcError as e:
                logging.error(f"API GetNodeList gRPC error: {e.details()}")
                return jsonify({"error": "節點池連接超時", "nodes": []}), 200
            except Exception as e:
                logging.error(f"API GetNodeList unexpected error: {e}", exc_info=True)
                return jsonify({"error": "獲取節點列表失敗", "nodes": []}), 200

        @self.app.route('/upload', methods=['GET', 'POST'])
        def upload_task_ui():
            username = request.args.get('user')
            if not username or not self.get_user(username):
                return redirect(url_for('login'))
            if request.method == 'POST':
                if 'task_zip' not in request.files:
                    flash('No task_zip file part', 'error')
                    return redirect(request.url)
                file = request.files['task_zip']
                if file.filename == '':
                    flash('No selected file', 'error')
                    return redirect(request.url)
                if file and file.filename.endswith('.zip'):
                    import uuid
                    import datetime
                    timestamp = datetime.datetime.now().strftime("%Y%m%d_%H%M%S")
                    task_uuid = str(uuid.uuid4())[:8]
                    task_id = f"task_{timestamp}_{task_uuid}"
                    requirements = {
                        "memory_gb": request.form.get('memory_gb', 0),
                        "cpu_score": request.form.get('cpu_score', 0),
                        "gpu_score": request.form.get('gpu_score', 0),
                        "gpu_memory_gb": request.form.get('gpu_memory_gb', 0),
                        "location": request.form.get('location', 'Any'),
                        "gpu_name": request.form.get('gpu_name', '')
                    }
                    task_zip_bytes = file.read()
                    _, success = self.upload_task_with_user(username, task_id, task_zip_bytes, requirements)
                    if success:
                        flash(f'任務 "{task_id}" 上傳成功！', 'success')
                        with self.task_cache_lock:
                            self.task_status_cache[task_id] = {"task_id": task_id, "status": "PENDING", "message": "Task submitted via UI"}
                    else:
                        flash(f'任務 "{task_id}" 上傳失敗。', 'error')
                    return redirect(url_for('index') + f"?user={username}")
                else:
                    flash('檔案格式無效，請上傳 .zip 檔案', 'error')
                    return redirect(request.url)
            return render_template('master_upload.html', username=username)

        @self.app.route('/api/poll_task/<task_id>')
        def api_poll_task(task_id):
            status_info = self.poll_task_status(task_id)
            return jsonify(status_info)

        @self.app.route('/api/task_status_and_retrieve/<task_id>')
        def api_task_status_and_retrieve(task_id):
            result = self.request_task_status_and_retrieve(task_id)
            return jsonify(result)

        @self.app.route('/api/stop_task/<task_id>', methods=['POST'])
        def api_stop_task(task_id):
            username = request.args.get('user')
            user = self.get_user(username)
            token = user['token'] if user else None
            
            if not username:
                logging.error(f"缺少用戶參數，無法停止任務 {task_id}")
                return jsonify({
                    "success": False,
                    "error": "缺少用戶參數",
                    "task_id": task_id
                }), 400
            
            if not user:
                logging.error(f"用戶 {username} 未登入，無法停止任務 {task_id}")
                return jsonify({
                    "success": False,
                    "error": "用戶未登入，請重新登入",
                    "task_id": task_id
                }), 401
            
            if not token:
                logging.error(f"用戶 {username} token為空，無法停止任務 {task_id}")
                return jsonify({
                    "success": False,
                    "error": "用戶token已過期，請重新登入",
                    "task_id": task_id
                }), 401
            
            if not self.master_stub:
                logging.error(f"gRPC stub 未初始化，無法停止任務 {task_id}")
                return jsonify({
                    "success": False,
                    "error": "服務未連接到節點池",
                    "task_id": task_id
                }), 500
            
            try:
                logging.info(f"用戶 {username} 請求停止任務 {task_id}")
                logging.info(f"使用用戶 {username} 的token停止任務 {task_id}: {token[:10]}...")
                
                # 創建停止任務請求，只使用 task_id 和 token
                req = nodepool_pb2.StopTaskRequest(
                    task_id=task_id,
                    token=token  # 使用請求用戶的token
                )
                
                logging.info(f"發送停止任務請求到節點池: task_id={task_id}, user={username}")
                
                # 發送 gRPC 請求到節點池
                response = self.master_stub.StopTask(req, timeout=30)
                
                logging.info(f"節點池回應停止任務請求: success={response.success}, message={response.message}")
                
                if response.success:
                    # 更新本地緩存
                    with self.task_cache_lock:
                        if task_id in self.task_status_cache:
                            self.task_status_cache[task_id].update({
                                "status": "STOPPED",
                                "last_polled": time.time()
                            })
                    
                    return jsonify({
                        "success": True,
                        "message": f"任務 {task_id} 已成功停止",
                        "task_id": task_id,
                    })
                else:
                    logging.warning(f"節點池拒絕停止任務請求: {response.message}")
                    return jsonify({
                        "success": False,
                        "error": f"節點池拒絕停止任務: {response.message}",
                        "message": f"停止任務 {task_id} 失敗: {response.message}",
                        "task_id": task_id
                    }), 400
                    
            except grpc.RpcError as e:
                logging.error(f"停止任務 {task_id} gRPC 錯誤: {e.code()} - {e.details()}")
                return jsonify({
                    "success": False,
                    "error": f"gRPC 錯誤: {e.details()}",
                    "message": f"停止任務時發生 gRPC 錯誤",
                    "task_id": task_id
                }), 500
            except Exception as e:
                logging.error(f"API 停止任務 {task_id} 失敗: {e}", exc_info=True)
                return jsonify({
                    "success": False,
                    "error": f"內部錯誤: {str(e)}",
                    "message": f"停止任務時發生內部錯誤",
                    "task_id": task_id
                }), 500

        @self.app.route('/api/task_logs/<task_id>')
        def api_task_logs(task_id):
            username = request.args.get('user')
            user = self.get_user(username)
            token = user['token'] if user else None
            if not token:
                return jsonify({"error": "用戶未登入或token已過期"}), 401
            if not self.master_stub:
                return jsonify({"error": "Not connected to gRPC server"}), 500
            try:
                req = nodepool_pb2.GetTaskLogsRequest(
                    task_id=task_id,
                    token=token
                )
                response = self.master_stub.GetTaskLogs(req, timeout=10)
                if response.success:
                    logs_text = response.logs
                    formatted_logs = []
                    if logs_text:
                        for line in logs_text.split('\n'):
                            if line.strip():
                                timestamp, content, level = self._parse_log_line(line)
                                formatted_logs.append({
                                    "timestamp": timestamp,
                                    "content": content,
                                    "level": level
                                })
                    status_request = nodepool_pb2.PollTaskStatusRequest(task_id=task_id)
                    status_response = self.master_stub.PollTaskStatus(
                        status_request, 
                        metadata=self._get_grpc_metadata(), 
                        timeout=10
                    )
                    return jsonify({
                        "task_id": task_id,
                        "status": status_response.status if status_response else "UNKNOWN",
                        "message": response.message,
                        "logs": formatted_logs,
                        "total_logs": len(formatted_logs)
                    })
                else:
                    return jsonify({
                        "error": response.message,
                        "task_id": task_id,
                        "logs": [],
                        "total_logs": 0
                    }), 404
            except grpc.RpcError as e:
                logging.error(f"API GetTaskLogs gRPC error: {e.details()}")
                return jsonify({"error": f"gRPC Error: {e.details()}"}), 500
            except Exception as e:
                logging.error(f"API GetTaskLogs unexpected error: {e}", exc_info=True)
                return jsonify({"error": f"Unexpected Error: {e}"}), 500

        @self.app.route('/api/download_result/<task_id>')
        def api_download_result(task_id):
            username = request.args.get('user')
            user = self.get_user(username)
            token = user['token'] if user else None
            if not token:
                return jsonify({"error": "用戶未登入或token已過期"}), 401
            if not self.master_stub:
                return jsonify({"error": "Not connected to gRPC server"}), 500
            try:
                req = nodepool_pb2.GetTaskResultRequest(
                    task_id=task_id,
                    token=token
                )
                response = self.master_stub.GetTaskResult(req, timeout=60)
                if response.success and response.result_zip:
                    from flask import Response
                    def generate():
                        yield response.result_zip
                    filename = f"{task_id}_result.zip"
                    return Response(
                        generate(),
                        mimetype='application/zip',
                        headers={
                            'Content-Disposition': f'attachment; filename="{filename}"',
                            'Content-Length': str(len(response.result_zip))
                        }
                    )
                else:
                    return jsonify({
                        "error": response.message or "Failed to get task result"
                    }), 404
            except grpc.RpcError as e:
                logging.error(f"Download result gRPC error: {e.details()}")
                return jsonify({"error": f"gRPC Error: {e.details()}"}), 500
            except Exception as e:
                logging.error(f"Download result unexpected error: {e}", exc_info=True)
                return jsonify({"error": f"Unexpected Error: {e}"}), 500

    def _detect_log_level(self, log_entry):
        """檢測日誌條目的級別"""
        if not isinstance(log_entry, str):
            return "INFO"
        
        log_upper = log_entry.upper()
        if "ERROR" in log_upper or "FAILED" in log_upper:
            return "ERROR"
        elif "WARNING" in log_upper or "WARN" in log_upper:
            return "WARNING"
        elif "DEBUG" in log_upper:
            return "DEBUG"
        else:
            return "INFO"

    def _parse_log_line(self, line):
        """解析日誌行，提取時間戳、內容和級別"""
        import re
        
        # 嘗試匹配格式: [2024-01-01 12:00:00] [節點ID] 內容
        # 或者: [2024-01-01 12:00:00] [級別] 內容
        timestamp_pattern = r'\[(\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2})\]'
        level_pattern = r'\[(ERROR|WARNING|INFO|DEBUG)\]'
        
        timestamp_match = re.search(timestamp_pattern, line)
        level_match = re.search(level_pattern, line)
        
        if timestamp_match:
            timestamp = timestamp_match.group(1)
            # 移除時間戳部分
            content = re.sub(timestamp_pattern, '', line, count=1).strip()
        else:
            timestamp = time.strftime('%Y-%m-%d %H:%M:%S')
            content = line
        
        if level_match:
            level = level_match.group(1).lower()
            # 移除級別部分
            content = re.sub(level_pattern, '', content, count=1).strip()
        else:
            level = self._detect_log_level(content)
        
        # 移除節點ID部分（如果存在）
        node_pattern = r'\[[a-zA-Z0-9_-]+\]'
        content = re.sub(node_pattern, '', content, count=1).strip()
        
        return timestamp, content, level

    def start_reward_distribution(self):
        if not self.reward_thread or not self.reward_thread.is_alive():
            self._stop_event.clear()
            self.reward_thread = threading.Thread(target=self.distribute_rewards, daemon=True)
            self.reward_thread.start()
            logging.info("Reward distribution thread started.")

    def start_auto_retrieval(self):
        if not self.auto_retrieve_thread or not self.auto_retrieve_thread.is_alive():
            self.auto_retrieve_thread = threading.Thread(target=self.auto_retrieve_completed_tasks, daemon=True)
            self.auto_retrieve_thread.start()
            logging.info("Auto retrieval thread started.")

    def stop_threads(self):
        self._stop_event.set()
        if self.reward_thread and self.reward_thread.is_alive():
            self.reward_thread.join(timeout=5)
        if self.task_poll_thread and self.task_poll_thread.is_alive():
            self.task_poll_thread.join(timeout=5)
        if self.auto_retrieve_thread and self.auto_retrieve_thread.is_alive():
            self.auto_retrieve_thread.join(timeout=5)

    def run(self):
        if not self._connect_grpc():
            logging.error("無法連接到節點池，退出")
            return

        if not self.login():
            logging.error("主控端登錄失敗，退出")
            return

        self.start_reward_distribution()
        self.start_auto_retrieval()
        
        try:
            # 確保主控端使用不同的端口，避免與工作端衝突
            actual_ui_port = UI_PORT
            logging.info(f"主控端 Master Node UI 啟動在 http://{UI_HOST}:{actual_ui_port}")
            logging.info(f"工作端應該使用端口 5000，主控端使用端口 {actual_ui_port}")
            self.app.run(host=UI_HOST, port=actual_ui_port, debug=False)
        except KeyboardInterrupt:
            logging.info("收到中斷信號，正在關閉...")
        finally:
            self.stop_threads()
            if self.channel:
                self.channel.close()

    def stop_task(self, task_id, reason=None):
        """停止指定的任務"""
        if not self.master_stub:
            logging.error("Cannot stop task: Not connected.")
            return False
        
        if not self.ensure_authenticated():
            logging.error("Authentication failed, cannot stop task")
            return False
        
        try:
            request = nodepool_pb2.StopTaskRequest(
                task_id=task_id,
                token=self.token  # 只用 self.token
            )
            response = self.master_stub.StopTask(request, timeout=10)
            
            if response.success:
                logging.info(f"任務 {task_id} 停止成功: {response.message}")
                
                # 更新本地緩存中的任務狀態
                with self.task_cache_lock:
                    if task_id in self.task_status_cache:
                        self.task_status_cache[task_id].update({
                            "status": "STOPPED",
                            "last_polled": time.time()
                        })
                
                return True
            else:
                logging.error(f"停止任務 {task_id} 失敗: {response.message}")
                return False
        except grpc.RpcError as e:
            logging.error(f"停止任務 {task_id} gRPC 錯誤: {e.details()}")
            return False
        except Exception as e:
            logging.error(f"停止任務 {task_id} 意外錯誤: {e}", exc_info=True)
            return False

class TaskStatusUpdater:
    def __init__(self, master_ui):
        self.master_ui = master_ui
        self.thread = None
        self._stop_event = threading.Event()

    def start(self):
        if not self.thread or not self.thread.is_alive():
            self._stop_event.clear()
            self.thread = threading.Thread(target=self._update_loop, daemon=True)
            self.thread.start()
            logging.info("Task status updater started.")

    def stop(self):
        self._stop_event.set()
        if self.thread and self.thread.is_alive():
            self.thread.join(timeout=5)

    def _update_loop(self):
        while not self._stop_event.is_set():
            try:
                # 清理過期緩存
                self.master_ui.clean_expired_task_cache()
                
                # 只更新活躍任務的狀態
                with self.master_ui.task_cache_lock:
                    active_tasks = {
                        task_id: info for task_id, info in self.master_ui.task_status_cache.items()
                        if isinstance(info, dict) and "task_id" in info
                        and info.get("status") not in ["COMPLETED", "FAILED", "ERROR", "STOPPED"]
                    }
                
                # 降低更新頻率
                if active_tasks:
                    logging.debug(f"更新 {len(active_tasks)} 個活躍任務狀態")
                    
                    for task_id in list(active_tasks.keys())[:3]:  # 一次只更新3個任務
                        if self._stop_event.is_set():
                            break
                        
                        try:
                            self.master_ui.poll_task_status(task_id)
                            time.sleep(5)  # 增加等待時間
                        except Exception as e:
                            logging.warning(f"更新任務 {task_id} 狀態失敗: {e}")
                
            except Exception as e:
                logging.error(f"任務狀態更新循環錯誤: {e}")
            
            # 等待更長時間再次更新
            self._stop_event.wait(120)  # 每2分鐘更新一次

if __name__ == "__main__":
    master_ui = MasterNodeUI(MASTER_USERNAME, MASTER_PASSWORD, GRPC_SERVER_ADDRESS)
    try:
        master_ui.run()
    except KeyboardInterrupt:
        logging.info("收到中斷信號，正在關閉...")
    except Exception as e:
        logging.error(f"主控端運行時發生錯誤: {e}", exc_info=True)
    finally:
        master_ui.stop_threads()
        if master_ui.channel:
            master_ui.channel.close()
        logging.info("主控端已關閉")
        if master_ui.channel:
            master_ui.channel.close()
        logging.info("主控端已關閉")
                

class TaskStatusUpdater:
    def __init__(self, master_ui):
        self.master_ui = master_ui
        self.thread = None
        self._stop_event = threading.Event()

    def start(self):
        if not self.thread or not self.thread.is_alive():
            self._stop_event.clear()
            self.thread = threading.Thread(target=self._update_loop, daemon=True)
            self.thread.start()
            logging.info("Task status updater started.")

    def stop(self):
        self._stop_event.set()
        if self.thread and self.thread.is_alive():
            self.thread.join(timeout=5)

    def _update_loop(self):
        while not self._stop_event.is_set():
            try:
                # 清理過期緩存
                self.master_ui.clean_expired_task_cache()
                
                # 只更新活躍任務的狀態
                with self.master_ui.task_cache_lock:
                    active_tasks = {
                        task_id: info for task_id, info in self.master_ui.task_status_cache.items()
                        if isinstance(info, dict) and "task_id" in info
                        and info.get("status") not in ["COMPLETED", "FAILED", "ERROR", "STOPPED"]
                    }
                
                # 降低更新頻率
                if active_tasks:
                    logging.debug(f"更新 {len(active_tasks)} 個活躍任務狀態")
                    
                    for task_id in list(active_tasks.keys())[:3]:  # 一次只更新3個任務
                        if self._stop_event.is_set():
                            break
                        
                        try:
                            self.master_ui.poll_task_status(task_id)
                            time.sleep(5)  # 增加等待時間
                        except Exception as e:
                            logging.warning(f"更新任務 {task_id} 狀態失敗: {e}")
                
            except Exception as e:
                logging.error(f"任務狀態更新循環錯誤: {e}")
            
            # 等待更長時間再次更新
            self._stop_event.wait(120)  # 每2分鐘更新一次

if __name__ == "__main__":
    master_ui = MasterNodeUI(MASTER_USERNAME, MASTER_PASSWORD, GRPC_SERVER_ADDRESS)
    try:
        master_ui.run()
    except KeyboardInterrupt:
        logging.info("收到中斷信號，正在關閉...")
    except Exception as e:
        logging.error(f"主控端運行時發生錯誤: {e}", exc_info=True)
    finally:
        master_ui.stop_threads()
        if master_ui.channel:
            master_ui.channel.close()
        logging.info("主控端已關閉")
        if master_ui.channel:
            master_ui.channel.close()
        logging.info("主控端已關閉")
