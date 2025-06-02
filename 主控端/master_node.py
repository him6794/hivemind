import grpc
import os
import time
import logging
import zipfile
from concurrent.futures import ThreadPoolExecutor, as_completed
import nodepool_pb2  # Updated proto with TransferRequest
import nodepool_pb2_grpc
import threading
from flask import Flask, jsonify, request, render_template, redirect, url_for, flash, session
import io
from functools import wraps

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

def login_required(f):
    @wraps(f)
    def decorated_function(*args, **kwargs):
        if 'username' not in session:
            flash('Please login to access this page.', 'error')
            return redirect(url_for('login'))
        return f(*args, **kwargs)
    return decorated_function

class MasterNodeUI:
    def __init__(self, username, password, grpc_address):
        self.username = username
        self.password = password
        self.grpc_address = grpc_address
        self.channel = None
        self.user_stub = None
        self.master_stub = None
        self.node_stub = None
        self.token = None
        self.reward_thread = None
        self.task_poll_thread = None
        self.auto_retrieve_thread = None
        self._stop_event = threading.Event()
        self.task_status_cache = {}
        self.task_cache_lock = threading.Lock()
        self.poll_interval = 5

        self.app = Flask(__name__, template_folder="templates_master", static_folder="static_master")
        self.setup_flask_routes()
        self.app.secret_key = FLASK_SECRET_KEY
        self.task_status_updater = TaskStatusUpdater(self)
        self.task_status_updater.start()

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

    def login(self):
        if not self.channel or not self.user_stub:
            logging.error("gRPC connection not established. Cannot login.")
            return False

        request = nodepool_pb2.LoginRequest(username=self.username, password=self.password)
        try:
            response = self.user_stub.Login(request, timeout=15)
            if response.success and response.token:
                self.token = response.token
                logging.info(f"MasterNodeUI logged in successfully. Token: {self.token[:10]}...")
                return True
            else:
                logging.error(f"MasterNodeUI login failed: {response.message}")
                self.token = None
                return False
        except grpc.RpcError as e:
            logging.error(f"MasterNodeUI login gRPC error: {e.code()} - {e.details()}")
            self.token = None
            return False
        except Exception as e:
            logging.error(f"MasterNodeUI login unexpected error: {e}", exc_info=True)
            self.token = None
            return False

    def _get_grpc_metadata(self):
        if self.token:
            logging.debug(f"附加 token 到 metadata: {self.token}")
            return [('authorization', f'Bearer {self.token}')]
        logging.warning("Attempting gRPC call without a valid token.")
        return None

    def ensure_authenticated(self):
        """確保主控端有有效的 token"""
        if not self.token:
            return self.login()
        
        try:
            balance_request = nodepool_pb2.GetBalanceRequest(token=self.token)
            self.user_stub.GetBalance(balance_request)
            return True
        except grpc.RpcError as e:
            if e.code() == grpc.StatusCode.UNAUTHENTICATED:
                logging.warning("Token 已失效，嘗試重新登錄")
                return self.login()
            else:
                logging.error(f"驗證 token 時發生錯誤: {e.details()}")
                return False

    def upload_task(self, task_id, task_zip_bytes, requirements):
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
                # 若 proto 有 cpt_cost 欄位，這裡加上 cpt_cost=cpt_cost
            )
            response = self.master_stub.UploadTask(request, timeout=30)
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

    def _get_user_id_from_token(self):
        """從 token 中獲取用戶ID"""
        try:
            # 使用 GetBalance 請求來獲取用戶ID
            request = nodepool_pb2.GetBalanceRequest(token=self.token)
            response = self.user_stub.GetBalance(request, timeout=10)
            if response.success:
                # 從消息中解析用戶ID
                message = response.message
                if "for user" in message:
                    user_id = message.split("for user")[-1].strip()
                    return user_id
            logging.error("無法從響應中獲取用戶ID")
            return None
        except Exception as e:
            logging.error(f"獲取用戶ID失敗: {e}")
            return None

    def poll_task_status(self, task_id):
        if not self.master_stub:
            logging.error("Cannot poll task status: Not connected.")
            return {"status": "ERROR", "message": "Not connected"}

        if not self.ensure_authenticated():
            logging.error("Authentication failed, cannot poll task status")
            return {"status": "ERROR", "message": "Authentication failed"}

        try:
            request = nodepool_pb2.PollTaskStatusRequest(task_id=task_id)
            response = self.master_stub.PollTaskStatus(request, metadata=self._get_grpc_metadata(), timeout=10)
            logging.debug(f"Polled task {task_id} status: {response.status}")
            with self.task_cache_lock:
                self.task_status_cache[task_id] = {
                    "task_id": task_id,  # 添加 task_id 字段
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
            request = nodepool_pb2.GetTaskResultRequest(task_id=task_id)
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
                        and info.get("status") not in ["COMPLETED", "FAILED", "ERROR"]
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

    def setup_flask_routes(self):
        @self.app.route('/login', methods=['GET', 'POST'])
        def login():
            if request.method == 'POST':
                username = request.form.get('username')
                password = request.form.get('password')
                # 呼叫節點池 gRPC 進行帳號密碼驗證
                if not self.channel or not self.user_stub:
                    flash('主控端未連接到節點池，請稍後再試。', 'error')
                    return render_template('login.html')
                try:
                    request_obj = nodepool_pb2.LoginRequest(username=username, password=password)
                    response = self.user_stub.Login(request_obj, timeout=10)
                    if response.success and response.token:
                        session['username'] = username
                        session['token'] = response.token
                        self.token = response.token  # 更新主控端 token
                        flash('Login successful!', 'success')
                        return redirect(url_for('index'))
                    else:
                        flash('Invalid username or password', 'error')
                except grpc.RpcError as e:
                    flash(f'gRPC 連線錯誤: {e.details()}', 'error')
                except Exception as e:
                    flash(f'登入時發生錯誤: {e}', 'error')
            return render_template('login.html')

        @self.app.route('/logout')
        def logout():
            session.pop('username', None)
            flash('You have been logged out.', 'success')
            return redirect(url_for('login'))

        @self.app.route('/')
        @login_required
        def index():
            return render_template('master_dashboard.html')

        @self.app.route('/api/nodes')
        @login_required
        def api_nodes():
            if not self.node_stub:
                return jsonify({"error": "Not connected to gRPC server"}), 500
            try:
                request = nodepool_pb2.GetNodeListRequest()
                response = self.node_stub.GetNodeList(request, timeout=10)
                if response.success:
                    nodes_list = []
                    for node in response.nodes:
                        nodes_list.append({
                            "node_id": node.node_id,
                            "hostname": node.hostname,
                            "cpu_cores": node.cpu_cores,
                            "memory_gb": node.memory_gb,
                            "status": node.status,
                            "last_heartbeat": time.strftime('%Y-%m-%d %H:%M:%S', time.localtime(int(node.last_heartbeat))) if node.last_heartbeat else 'N/A',
                            "cpu_score": node.cpu_score,
                            "gpu_score": node.gpu_score,
                            "gpu_memory_gb": node.gpu_memory_gb,
                            "gpu_name": node.gpu_name,
                            "location": node.location,
                            "port": node.port,
                        })
                    return jsonify({"nodes": nodes_list})
                else:
                    return jsonify({"error": f"Failed to get node list: {response.message}"}), 500
            except grpc.RpcError as e:
                logging.error(f"API GetNodeList gRPC error: {e.details()}")
                return jsonify({"error": f"gRPC Error: {e.details()}"}), 500
            except Exception as e:
                logging.error(f"API GetNodeList unexpected error: {e}", exc_info=True)
                return jsonify({"error": f"Unexpected Error: {e}"}), 500

        @self.app.route('/api/balance')
        @login_required
        def api_balance():
            if not self.user_stub or not self.token:
                return jsonify({"error": "Not connected to gRPC server"}), 500
            try:
                request = nodepool_pb2.GetBalanceRequest(token=self.token)
                response = self.user_stub.GetBalance(request, timeout=10)
                if response.success:
                    return jsonify({"cpt_balance": response.balance})
                else:
                    return jsonify({"error": f"Failed to get balance: {response.message}"}), 500
            except grpc.RpcError as e:
                logging.error(f"API GetBalance gRPC error: {e.details()}")
                return jsonify({"error": f"gRPC Error: {e.details()}"}), 500
            except Exception as e:
                logging.error(f"API GetBalance unexpected error: {e}", exc_info=True)
                return jsonify({"error": f"Unexpected Error: {e}"}), 500

        @self.app.route('/api/tasks')
        @login_required 
        def api_tasks():
            task_list = []
            try:
                meta = self._get_grpc_metadata()
                if not meta:
                    return jsonify({"error": "Not authenticated"}), 401
                    
                # 從緩存獲取任務狀態
                with self.task_cache_lock:
                    for task_id, status_info in self.task_status_cache.items():
                        # 確保這是一個任務記錄而不是節點狀態
                        if isinstance(status_info, dict) and "task_id" in status_info:
                            task_list.append({
                                "task_id": task_id,
                                "status": status_info.get("status", "UNKNOWN"),
                                "progress": status_info.get("progress", "0%"),
                                "message": status_info.get("message", ""),
                                "output_tail": status_info.get("output_tail", []),
                                "last_update": time.strftime('%Y-%m-%d %H:%M:%S', 
                                    time.localtime(int(status_info.get("last_polled")))) if status_info.get("last_polled") else "-"
                            })
                        
                # 如果緩存为空，主動從節點池獲取所有任務
                if not task_list:
                    logging.info("緩存中無任務，嘗試從節點池獲取任務列表")
                    try:
                        # 從master_node_service獲取活動任務
                        request = nodepool_pb2.GetAllTasksRequest(token=self.token)
                        response = self.master_stub.GetAllTasks(request, metadata=meta, timeout=15)
                        
                        if response.success and response.tasks:
                            logging.info(f"從節點池獲取到 {len(response.tasks)} 個任務")
                            
                            # 更新任務緩存
                            for task in response.tasks:
                                task_status = self.poll_task_status(task.task_id)
                                
                                task_list.append({
                                    "task_id": task.task_id,
                                    "status": task_status.get("status", "UNKNOWN"),
                                    "progress": task_status.get("progress", "0%"),
                                    "message": task_status.get("message", ""),
                                    "output_tail": task_status.get("output_tail", []),
                                    "last_update": time.strftime('%Y-%m-%d %H:%M:%S', time.time())
                                })
                        else:
                            logging.info("節點池未返回任務或請求失敗")
                            
                        # 直接查詢工作節點的運行中任務
                        try:
                            # 先獲取所有節點
                            nodes_req = nodepool_pb2.GetNodeListRequest()
                            nodes_resp = self.node_stub.GetNodeList(nodes_req, metadata=meta, timeout=10)
                            
                            if nodes_resp.success and nodes_resp.nodes:
                                for node in nodes_resp.nodes:
                                    if node.status == "Running" and node.port > 0:
                                        # 連接到工作節點查詢運行狀態
                                        worker_address = f'127.0.0.1:{node.port}'
                                        worker_channel = grpc.insecure_channel(worker_address)
                                        try:
                                            grpc.channel_ready_future(worker_channel).result(timeout=2)
                                            worker_stub = nodepool_pb2_grpc.WorkerNodeServiceStub(worker_channel)
                                            
                                            status_req = nodepool_pb2.RunningStatusRequest(node_id=node.node_id, task_id="")
                                            status_resp = worker_stub.ReportRunningStatus(status_req, timeout=3)
                                            
                                            if status_resp.success and status_resp.running_task_id:
                                                # 添加到任務列表
                                                running_task = {
                                                    "task_id": status_resp.running_task_id,
                                                    "status": "RUNNING",
                                                    "progress": status_resp.progress or "0%",
                                                    "message": f"Running on {node.node_id}",
                                                    "output_tail": [],
                                                    "last_update": time.strftime('%Y-%m-%d %H:%M:%S', time.time())
                                                }
                                                
                                                # 更新緩存
                                                with self.task_cache_lock:
                                                    self.task_status_cache[status_resp.running_task_id] = {
                                                        "task_id": status_resp.running_task_id,
                                                        "status": "RUNNING",
                                                        "message": f"Running on {node.node_id}",
                                                        "last_polled": time.time()
                                                    }
                                                
                                                # 避免重複添加
                                                if not any(t["task_id"] == status_resp.running_task_id for t in task_list):
                                                    task_list.append(running_task)
                                                    logging.info(f"從工作節點 {node.node_id} 發現運行中任務: {status_resp.running_task_id}")
                                        except Exception as worker_err:
                                            logging.warning(f"訪問工作節點 {node.node_id} 時出錯: {worker_err}")
                                        finally:
                                            worker_channel.close()
                        except Exception as node_err:
                            logging.error(f"查詢節點任務時出錯: {node_err}")
                    except Exception as e:
                        logging.error(f"從節點池獲取任務列表失敗: {e}")
                        
                return jsonify({"tasks": task_list})
                        
            except Exception as e:
                logging.error(f"處理任務列表請求時出錯: {e}")
                return jsonify({"error": str(e), "tasks": []}), 500

        @self.app.route('/upload', methods=['GET', 'POST'])
        @login_required
        def upload_task_ui():
            if request.method == 'POST':
                if 'task_zip' not in request.files:
                    flash('No task_zip file part', 'error')
                    return redirect(request.url)
                file = request.files['task_zip']
                if file.filename == '':
                    flash('No selected file', 'error')
                    return redirect(request.url)

                if file and file.filename.endswith('.zip'):
                    task_id = request.form.get('task_id', f"task_{int(time.time())}")
                    requirements = {
                        "memory_gb": request.form.get('memory_gb', 0),
                        "cpu_score": request.form.get('cpu_score', 0),
                        "gpu_score": request.form.get('gpu_score', 0),
                        "gpu_memory_gb": request.form.get('gpu_memory_gb', 0),
                        "location": request.form.get('location', 'Any'),
                        "gpu_name": request.form.get('gpu_name', '')
                    }
                    task_zip_bytes = file.read()
                    _, success = self.upload_task(task_id, task_zip_bytes, requirements)
                    if success:
                        flash(f'Task "{task_id}" uploaded successfully!', 'success')
                        with self.task_cache_lock:
                            self.task_status_cache[task_id] = {"status": "PENDING", "message": "Task submitted via UI"}
                    else:
                        flash(f'Task "{task_id}" upload failed.', 'error')
                    return redirect(url_for('index'))
                else:
                    flash('Invalid file type, please upload a .zip file', 'error')
                    return redirect(request.url)
            return render_template('master_upload.html')

        @self.app.route('/api/poll_task/<task_id>')
        @login_required
        def api_poll_task(task_id):
            status_info = self.poll_task_status(task_id)
            return jsonify(status_info)

        @self.app.route('/api/task_status_and_retrieve/<task_id>')
        @login_required
        def api_task_status_and_retrieve(task_id):
            result = self.request_task_status_and_retrieve(task_id)
            return jsonify(result)

    def start_flask_app(self):
        def run_flask():
            log = logging.getLogger('werkzeug')
            log.setLevel(logging.ERROR)
            try:
                logging.info(f"Starting Master UI Flask server on {UI_HOST}:{UI_PORT}...")
                self.app.run(host=UI_HOST, port=UI_PORT, debug=False, use_reloader=False)
            except Exception as e:
                logging.error(f"Flask server failed: {e}", exc_info=True)
                self._stop_event.set()

        flask_thread = threading.Thread(target=run_flask, name="FlaskThread", daemon=True)
        flask_thread.start()
        logging.info("Flask server thread started.")

    def poll_all_tasks(self):
        logging.info("Task status polling thread started.")
        while not self._stop_event.is_set():
            if not self.master_stub or not self.token:
                logging.warning("Task polling skipped: Not connected or not logged in.")
                self._stop_event.wait(self.poll_interval)
                continue

            try:
                self.clean_expired_task_cache()
                
                request = nodepool_pb2.GetNodeListRequest()
                meta = self._get_grpc_metadata()
                if not meta:
                    logging.warning("Missing token for task status polling")
                    self._stop_event.wait(self.poll_interval)
                    continue

                try:
                    # 查詢所有節點
                    node_response = self.node_stub.GetNodeList(request, metadata=meta, timeout=10)
                    if node_response.success:
                        for node in node_response.nodes:
                            if self._stop_event.is_set():
                                break
                            
                            # 更新節點狀態緩存
                            with self.task_cache_lock:
                                self.task_status_cache[node.node_id] = {
                                    "type": "node",
                                    "status": node.status,
                                    "last_polled": time.time()
                                }
                                
                            # 只查詢狀態為"Running"的節點上的任務
                            if node.status == "Running" and node.port > 0:
                                worker_channel = None
                                try:
                                    worker_address = f'127.0.0.1:{node.port}'
                                    worker_channel = grpc.insecure_channel(worker_address)
                                    grpc.channel_ready_future(worker_channel).result(timeout=2)
                                    worker_stub = nodepool_pb2_grpc.WorkerNodeServiceStub(worker_channel)
                                    
                                    status_req = nodepool_pb2.RunningStatusRequest(node_id=node.node_id, task_id="")
                                    status_response = worker_stub.ReportRunningStatus(status_req, timeout=3)
                                    
                                    if status_response.success:
                                        # 更新節點狀態
                                        with self.task_cache_lock:
                                            self.task_status_cache[node.node_id]["message"] = status_response.message
                                        
                                        # 如果節點正在運行任務，獲取任務信息並更新緩存
                                        if status_response.running_task_id:
                                            running_task_id = status_response.running_task_id
                                            progress = status_response.progress or "0%"
                                            
                                            # 更新任務狀態
                                            with self.task_cache_lock:
                                                self.task_status_cache[running_task_id] = {
                                                    "task_id": running_task_id,
                                                    "type": "task",
                                                    "status": "RUNNING",
                                                    "progress": progress,
                                                    "message": f"Running on {node.node_id}",
                                                    "assigned_node": node.node_id,
                                                    "last_polled": time.time()
                                                }
                                                
                                            logging.debug(f"節點 {node.node_id} 正在運行任務 {running_task_id}，進度: {progress}")
                                        else:
                                            logging.debug(f"節點 {node.node_id} 狀態: {node.status}，但當前無任務")
                                
                                except grpc.FutureTimeoutError:
                                    logging.warning(f"連接工作節點 {node.node_id} 超時")
                                except Exception as e:
                                    logging.error(f"查詢工作節點 {node.node_id} 運行狀態時出錯: {e}")
                                finally:
                                    if worker_channel:
                                        worker_channel.close()
                    
                    # 直接查詢節點池中所有任務的狀態
                    try:
                        task_list_req = nodepool_pb2.GetAllTasksRequest(token=self.token)
                        task_list_resp = self.master_stub.GetAllTasks(task_list_req, metadata=meta, timeout=10)
                        
                        if task_list_resp.success and task_list_resp.tasks:
                            for task in task_list_resp.tasks:
                                # 為每個任務查詢詳細狀態
                                task_status = self.poll_task_status(task.task_id)
                                logging.debug(f"節點池中任務 {task.task_id} 狀態: {task_status.get('status', 'UNKNOWN')}")
                    except Exception as task_err:
                        logging.error(f"獲取所有任務列表時出錯: {task_err}")
                    
                except grpc.RpcError as e:
                    logging.error(f"獲取節點列表時出錯: {e.details()}")
                except Exception as e:
                    logging.error(f"節點輪詢過程中發生未知錯誤: {e}")
                
            except Exception as e:
                logging.error(f"任務輪詢循環中發生錯誤: {e}")
            
            self._stop_event.wait(self.poll_interval)

        logging.info("Task status polling thread stopped.")

    def start_task_polling(self):
        if not self.task_poll_thread or not self.task_poll_thread.is_alive():
            self.task_poll_thread = threading.Thread(
                target=self.poll_all_tasks,
                name="TaskPollThread",
                daemon=True
            )
            self.task_poll_thread.start()
            logging.info("Task status polling thread started.")

    def stop_task_polling(self):
        if self.task_poll_thread and self.task_poll_thread.is_alive():
            logging.info("Requesting task polling thread stop...")
            self._stop_event.set()
            self.task_poll_thread.join(timeout=5)
            self.task_poll_thread = None

    def start(self):
        logging.info("Starting MasterNodeUI...")
        if not self._connect_grpc():
            logging.error("Failed to connect to gRPC server. Exiting.")
            return
        if not self.login():
            logging.error("MasterNodeUI login failed. UI and reward distribution might not work correctly.")
        self.start_flask_app()
        self.reward_thread = threading.Thread(target=self.distribute_rewards, name="RewardThread", daemon=True)
        self.reward_thread.start()
        self.start_task_polling()
        
        self.auto_retrieve_thread = threading.Thread(
            target=self.auto_retrieve_completed_tasks, 
            name="AutoRetrieveThread", 
            daemon=True
        )
        self.auto_retrieve_thread.start()
        
        logging.info("MasterNodeUI started. Running Flask UI and background tasks. Press Ctrl+C to stop.")
        try:
            while not self._stop_event.is_set():
                time.sleep(5)
        except KeyboardInterrupt:
            logging.info("Ctrl+C detected. Stopping MasterNodeUI...")
        finally:
            self.stop()

    def stop(self):
        logging.info("Stopping MasterNodeUI services...")
        self._stop_event.set()
        if self.reward_thread and self.reward_thread.is_alive():
            logging.info("Waiting for reward thread to stop...")
            self.reward_thread.join(timeout=5)
        if self.task_poll_thread and self.task_poll_thread.is_alive():
            logging.info("Waiting for task polling thread to stop...")
            self.task_poll_thread.join(timeout=5)
        if hasattr(self, 'auto_retrieve_thread') and self.auto_retrieve_thread.is_alive():
            logging.info("Waiting for auto retrieve thread to stop...")
            self.auto_retrieve_thread.join(timeout=5)
        if self.channel:
            logging.info("Closing gRPC channel.")
            self.channel.close()
        logging.info("MasterNodeUI stopped.")

    def run_batch_from_dir(self, task_dir):
        if not self.token:
            if not self.login():
                logging.error("Login failed, cannot run batch processing.")
                return
        tasks = []
        logging.info(f"Scanning task directory: {task_dir}")
        if not os.path.isdir(task_dir):
            logging.error(f"Task directory not found: {task_dir}")
            return
        for item_name in os.listdir(task_dir):
            if item_name.endswith(".zip") and not item_name.endswith("_result.zip"):
                task_id = item_name.replace(".zip", "")
                task_zip_path = os.path.join(task_dir, item_name)
                requirements_path = os.path.join(task_dir, f"{task_id}_requirements.txt")
                if not os.path.exists(requirements_path):
                    logging.warning(f"Skipping task {task_id}: Requirements file '{requirements_path}' not found.")
                    continue
                requirements = {}
                try:
                    with open(requirements_path, "r") as f:
                        for line in f:
                            if '=' in line:
                                key, value = line.strip().split("=", 1)
                                key = key.strip()
                                value = value.strip()
                                if key in ["memory_gb", "cpu_score", "gpu_score", "gpu_memory_gb"]:
                                    try:
                                        requirements[key] = int(value)
                                    except ValueError:
                                        logging.warning(f"Invalid integer value for '{key}' in {requirements_path}: '{value}'. Using 0.")
                                        requirements[key] = 0
                                elif key in ["location", "gpu_name"]:
                                    requirements[key] = value
                                else:
                                    logging.warning(f"Unknown key '{key}' in {requirements_path}. Ignoring.")
                    requirements.setdefault("memory_gb", 0)
                    requirements.setdefault("cpu_score", 0)
                    requirements.setdefault("gpu_score", 0)
                    requirements.setdefault("gpu_memory_gb", 0)
                    requirements.setdefault("location", "Any")
                    requirements.setdefault("gpu_name", "")
                    with open(task_zip_path, "rb") as f_zip:
                        task_zip_bytes = f_zip.read()
                    tasks.append((task_id, task_zip_bytes, requirements))
                except FileNotFoundError:
                    logging.error(f"Error reading file: {task_zip_path} or {requirements_path}")
                    continue
                except Exception as e:
                    logging.error(f"Error parsing requirements for task {task_id}: {e}")
                    continue
        if not tasks:
            logging.info("No valid tasks found in the directory.")
            return
        logging.info(f"Found {len(tasks)} tasks to process.")
        uploaded_tasks = []
        with ThreadPoolExecutor(max_workers=10, thread_name_prefix="Upload") as executor:
            future_to_task = {
                executor.submit(self.upload_task, task_id, zip_bytes, reqs): task_id
                for task_id, zip_bytes, reqs in tasks
            }
            for future in as_completed(future_to_task):
                task_id, success = future.result()
                if success:
                    uploaded_tasks.append(task_id)
                else:
                    logging.error(f"Batch: Task {task_id} upload failed.")
        if not uploaded_tasks:
            logging.error("Batch: All task uploads failed.")
            return
        logging.info(f"Batch: Successfully initiated upload for {len(uploaded_tasks)} tasks.")
        logging.info("Batch: Polling task statuses (simple check)...")
        all_done = False
        max_poll_time = 300
        start_poll_time = time.time()
        while not all_done and (time.time() - start_poll_time) < max_poll_time:
            all_done = True
            current_statuses = {}
            for task_id in uploaded_tasks:
                status_info = self.poll_task_status(task_id)
                current_statuses[task_id] = status_info["status"]
                if status_info["status"] not in ["COMPLETED", "FAILED", "ERROR"]:
                    all_done = False
            logging.info(f"Batch Status Check: {current_statuses}")
            if not all_done:
                time.sleep(15)
        if not all_done:
            logging.warning("Batch: Max polling time reached, some tasks might not be finished.")
        logging.info("Batch: Attempting to get results for completed/failed tasks...")
        results_dir = os.path.join(task_dir, "results")
        os.makedirs(results_dir, exist_ok=True)
        for task_id in uploaded_tasks:
            result_path = os.path.join(results_dir, f"{task_id}_result.zip")
            self.get_task_result(task_id, result_path)
        logging.info("Batch processing finished.")

class TaskStatusUpdater:
    def __init__(self, master_node_service):
        self.master_node_service = master_node_service
        self.running = False
        self.update_thread = None
        self.update_interval = 10  # 10秒更新一次

    def start(self):
        """啟動狀態更新線程"""
        if self.running:
            return
        self.running = True
        self.update_thread = threading.Thread(target=self._update_loop)
        self.update_thread.daemon = True
        self.update_thread.start()

    def stop(self):
        """停止狀態更新線程"""
        self.running = False
        if self.update_thread:
            self.update_thread.join()

    def _update_loop(self):
        """狀態更新循環"""
        while self.running:
            try:
                # 獲取所有任務的狀態
                if not self.master_node_service.master_stub or not self.master_node_service.token:
                    logging.warning("Task status update skipped: Not connected or not logged in.")
                    time.sleep(self.update_interval)
                    continue

                # 從任務緩存中獲取所有任務ID
                with self.master_node_service.task_cache_lock:
                    task_ids = [
                        task_id for task_id, info in self.master_node_service.task_status_cache.items()
                        if isinstance(info, dict) and "task_id" in info
                    ]

                for task_id in task_ids:
                    try:
                        # 使用現有的 PollTaskStatus 服務獲取任務狀態
                        request = nodepool_pb2.PollTaskStatusRequest(task_id=task_id)
                        response = self.master_node_service.master_stub.PollTaskStatus(
                            request, 
                            metadata=self.master_node_service._get_grpc_metadata(),
                            timeout=10
                        )
                        
                        # 更新任務緩存
                        with self.master_node_service.task_cache_lock:
                            self.master_node_service.task_status_cache[task_id] = {
                                "task_id": task_id,
                                "status": response.status,
                                "message": response.message,
                                "output_tail": response.output[-5:] if response.output else [],
                                "last_polled": time.time()
                            }
                            
                    except Exception as e:
                        logging.error(f"更新任務 {task_id} 狀態時發生錯誤: {e}")

            except Exception as e:
                logging.error(f"任務狀態更新循環發生錯誤: {e}", exc_info=True)
            
            # 確保時間格式正確
            created_at = ""
            updated_at = ""
            
            try:
                # 如果時間戳是數字，轉化為字符串格式
                if task_info.get("created_at"):
                    created_timestamp = float(task_info.get("created_at", 0))
                    created_at = time.strftime("%Y-%m-%d %H:%M:%S", time.localtime(created_timestamp))
                
                if task_info.get("updated_at"):
                    updated_timestamp = float(task_info.get("updated_at", 0))
                    updated_at = time.strftime("%Y-%m-%d %H:%M:%S", time.localtime(updated_timestamp))
            except (ValueError, TypeError) as e:
                logging.warning(f"任務 {task_id} 時間戳格式錯誤: {e}")
                # 使用當前時間作為備用
                current_time = time.strftime("%Y-%m-%d %H:%M:%S", time.localtime())
                created_at = current_time
                updated_at = current_time
            
            # 創建TaskStatus對象
            task_status = nodepool_pb2.TaskStatus(
                task_id=task_id,
                status=task_info.get("status", "UNKNOWN"),
                created_at=created_at,
                updated_at=updated_at,
                assigned_node=task_info.get("assigned_node", "")
            )
            all_tasks.append(task_status)
            
            # 等待下一次更新
            time.sleep(self.update_interval)

    def add_user_task(self, user_id, task_id):
        """添加用戶任務到緩存"""
        try:
            with self.master_node_service.task_cache_lock:
                self.master_node_service.task_status_cache[task_id] = {
                    "task_id": task_id,
                    "status": "PENDING",
                    "message": "Task submitted",
                    "last_polled": time.time()
                }
            return True
        except Exception as e:
            logging.error(f"添加用戶任務失敗: {e}", exc_info=True)
            return False

    def get_user_tasks(self, user_id):
        """獲取用戶的所有任務狀態"""
        try:
            with self.master_node_service.task_cache_lock:
                return [
                    info for info in self.master_node_service.task_status_cache.values()
                    if isinstance(info, dict) and "task_id" in info
                ]
        except Exception as e:
            logging.error(f"獲取用戶任務失敗: {e}", exc_info=True)
            return []

    def get_task_status(self, task_id):
        """獲取特定任務的狀態"""
        try:
            with self.master_node_service.task_cache_lock:
                return self.master_node_service.task_status_cache.get(task_id)
        except Exception as e:
            logging.error(f"獲取任務狀態失敗: {e}", exc_info=True)
            return None

class MasterNode:
    def __init__(self):
        self.task_status_updater = TaskStatusUpdater(self)
        self.task_status_updater.start()

    def __del__(self):
        self.task_status_updater.stop()

    def get_user_tasks(self, user_id):
        """获取用户的所有任务状态"""
        return self.task_status_updater.get_user_tasks(user_id)

    def get_task_status(self, task_id):
        """获取特定任务的状态"""
        return self.task_status_updater.get_task_status(task_id)

    def add_user_task(self, user_id, task_id):
        """添加用户任务"""
        return self.task_status_updater.add_user_task(user_id, task_id)

if __name__ == "__main__":
    master_ui = MasterNodeUI(
        username=MASTER_USERNAME,
        password=MASTER_PASSWORD,
        grpc_address=GRPC_SERVER_ADDRESS
    )
    master_ui.start()