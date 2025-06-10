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
            
            # 檢查用戶餘額是否足夠
            try:
                balance_request = nodepool_pb2.GetBalanceRequest(token=self.token)
                balance_response = self.user_stub.GetBalance(balance_request, timeout=10)
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
                            last_polled = status_info.get("last_polled")
                            if last_polled:
                                try:
                                    if isinstance(last_polled, (int, float)):
                                        last_update_str = time.strftime('%Y-%m-%d %H:%M:%S', time.localtime(last_polled))
                                    else:
                                        last_update_str = str(last_polled)
                                except (ValueError, OSError):
                                    last_update_str = "-"
                            else:
                                last_update_str = "-"
                            
                            task_list.append({
                                "task_id": task_id,
                                "status": status_info.get("status", "UNKNOWN"),
                                "progress": status_info.get("progress", "0%"),
                                "message": status_info.get("message", ""),
                                "output_tail": status_info.get("output_tail", []),
                                "last_update": last_update_str
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
                                try:
                                    task_status = self.poll_task_status(task.task_id)
                                    
                                    task_list.append({
                                        "task_id": task.task_id,
                                        "status": task_status.get("status", "UNKNOWN"),
                                        "progress": task_status.get("progress", "0%"),
                                        "message": task_status.get("message", ""),
                                        "output_tail": task_status.get("output_tail", []),
                                        "last_update": time.strftime('%Y-%m-%d %H:%M:%S')
                                    })
                                except Exception as task_err:
                                    logging.error(f"處理任務 {task.task_id} 時出錯: {task_err}")
                                    # 添加一個基本的任務條目
                                    task_list.append({
                                        "task_id": task.task_id,
                                        "status": "UNKNOWN",
                                        "progress": "0%",
                                        "message": "Failed to get task status",
                                        "output_tail": [],
                                        "last_update": time.strftime('%Y-%m-%d %H:%M:%S')
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
                                        # 使用節點的 hostname
                                        node_address = f'{node.hostname}:{node.port}'
                                        worker_channel = grpc.insecure_channel(node_address)
                                        try:
                                            grpc.channel_ready_future(worker_channel).result(timeout=2)
                                            worker_stub = nodepool_pb2_grpc.WorkerNodeServiceStub(worker_channel)
                                            
                                            status_req = nodepool_pb2.RunningStatusRequest(node_id=node.node_id, task_id="")
                                            status_resp = worker_stub.ReportRunningStatus(status_req, timeout=3)
                                            
                                            if status_resp.success and hasattr(status_resp, 'message') and "Running Task:" in status_resp.message:
                                                # 從消息中提取任務ID
                                                try:
                                                    running_task_id = status_resp.message.split("Running Task:")[-1].strip()
                                                    if running_task_id:
                                                        # 添加到任務列表
                                                        running_task = {
                                                            "task_id": running_task_id,
                                                            "progress": "運行中",
                                                            "message": f"Running on {node.node_id}",
                                                            "output_tail": [],
                                                            "last_update": time.strftime('%Y-%m-%d %H:%M:%S')
                                                        }
                                                        
                                                        # 更新緩存
                                                        with self.task_cache_lock:
                                                            self.task_status_cache[running_task_id] = {
                                                                "task_id": running_task_id,
                                                                "status": "RUNNING",
                                                                "message": f"Running on {node.node_id}",
                                                                "last_polled": time.time()
                                                            }
                                                        
                                                        # 避免重複添加
                                                        if not any(t["task_id"] == running_task_id for t in task_list):
                                                            task_list.append(running_task)
                                                            logging.info(f"從工作節點 {node.node_id} 發現運行中任務: {running_task_id}")
                                                except Exception as parse_err:
                                                    logging.warning(f"解析運行中任務ID失敗: {parse_err}")
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
                    # 自動生成任務ID
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
                    _, success = self.upload_task(task_id, task_zip_bytes, requirements)
                    if success:
                        flash(f'任務 "{task_id}" 上傳成功！', 'success')
                        with self.task_cache_lock:
                            self.task_status_cache[task_id] = {"task_id": task_id, "status": "PENDING", "message": "Task submitted via UI"}
                    else:
                        flash(f'任務 "{task_id}" 上傳失敗。', 'error')
                    return redirect(url_for('index'))
                else:
                    flash('檔案格式無效，請上傳 .zip 檔案', 'error')
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

        @self.app.route('/api/stop_task/<task_id>', methods=['POST'])
        @login_required
        def api_stop_task(task_id):
            """停止指定的任務"""
            try:
                # 從請求中獲取停止原因（可選）
                data = request.get_json() or {}
                reason = data.get('reason', '用戶手動停止')
                
                # 調用停止任務方法
                success = self.stop_task(task_id, reason)
                
                if success:
                    return jsonify({
                        "success": True,
                        "message": f"任務 {task_id} 已成功停止",
                        "task_id": task_id,
                        "reason": reason
                    })
                else:
                    return jsonify({
                        "success": False,
                        "error": f"停止任務 {task_id} 失敗",
                        "task_id": task_id
                    }), 400
                    
            except Exception as e:
                logging.error(f"API 停止任務 {task_id} 失敗: {e}", exc_info=True)
                return jsonify({
                    "success": False,
                    "error": f"內部錯誤: {str(e)}",
                    "task_id": task_id
                }), 500

        @self.app.route('/api/task_logs/<task_id>')
        @login_required
        def api_task_logs(task_id):
            """從節點池獲取特定任務的詳細日誌"""
            if not self.master_stub or not self.token:
                return jsonify({"error": "Not connected to gRPC server"}), 500
            
            try:
                # 使用新的 GetTaskLogs RPC，包含 token
                request = nodepool_pb2.GetTaskLogsRequest(
                    task_id=task_id,
                    token=self.token
                )
                
                response = self.master_stub.GetTaskLogs(request, timeout=10)
                
                if response.success:
                    # 解析日誌
                    logs_text = response.logs
                    formatted_logs = []
                    
                    if logs_text:
                        for line in logs_text.split('\n'):
                            if line.strip():
                                # 嘗試解析時間戳和級別
                                timestamp, content, level = self._parse_log_line(line)
                                formatted_logs.append({
                                    "timestamp": timestamp,
                                    "content": content,
                                    "level": level
                                })
                    
                    # 同時獲取任務狀態
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

        @self.app.route('/api/task_live_logs/<task_id>')
        @login_required
        def api_task_live_logs(task_id):
            """獲取任務的實時日誌更新"""
            try:
                # 從緩存中獲取任務信息
                with self.task_cache_lock:
                    task_info = self.task_status_cache.get(task_id)
                
                if not task_info:
                    return jsonify({"error": f"Task {task_id} not found in cache"}), 404
                
                # 如果任務正在運行，嘗試從工作節點獲取實時日誌
                if task_info.get("status") == "RUNNING" and task_info.get("assigned_node"):
                    node_id = task_info.get("assigned_node")
                    
                    # 查找節點的端口
                    try:
                        nodes_req = nodepool_pb2.GetNodeListRequest()
                        nodes_resp = self.node_stub.GetNodeList(nodes_req, timeout=5)
                        
                        target_node = None
                        for node in nodes_resp.nodes:
                            if node.node_id == node_id:
                                target_node = node
                                break
                        
                        if target_node and target_node.port > 0:
                            # 使用節點的實際hostname而不是127.0.0.1
                            worker_address = f'{target_node.hostname}:{target_node.port}'
                            worker_channel = grpc.insecure_channel(worker_address)
                            try:
                                grpc.channel_ready_future(worker_channel).result(timeout=2)
                                worker_stub = nodepool_pb2_grpc.WorkerNodeServiceStub(worker_channel)
                                
                                status_req = nodepool_pb2.RunningStatusRequest(node_id=node_id, task_id=task_id)
                                status_resp = worker_stub.ReportRunningStatus(status_req, timeout=3)
                                
                                if status_resp.success:
                                    return jsonify({
                                        "task_id": task_id,
                                        "status": "RUNNING",
                                        "live_message": status_resp.message,
                                        "timestamp": time.strftime('%Y-%m-%d %H:%M:%S')
                                    })
                            except Exception as e:
                                logging.warning(f"無法連接到工作節點 {node_id}: {e}")
                            finally:
                                worker_channel.close()
                    
                    except Exception as e:
                        logging.error(f"查找節點信息失敗: {e}")
                
                # 返回緩存中的任務信息
                return jsonify({
                    "task_id": task_id,
                    "status": task_info.get("status", "UNKNOWN"),
                    "message": task_info.get("message", ""),
                    "output_tail": task_info.get("output_tail", []),
                    "timestamp": time.strftime('%Y-%m-%d %H:%M:%S')
                })
                
            except Exception as e:
                logging.error(f"獲取實時日誌失敗: {e}")
                return jsonify({"error": f"Error: {str(e)}"}), 500

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
            logging.info(f"Master Node UI 啟動在 http://{UI_HOST}:{UI_PORT}")
            self.app.run(host=UI_HOST, port=UI_PORT, debug=False)
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
                token=self.token
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
            logging.error(f"停止任務 {task_id} 發生未知錯誤: {e}", exc_info=True)
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
                
                # 更新任務狀態
                with self.master_ui.task_cache_lock:
                    task_ids = list(self.master_ui.task_status_cache.keys())
                
                for task_id in task_ids:
                    if self._stop_event.is_set():
                        break
                    
                    try:
                        # 只更新任務，跳過節點狀態
                        with self.master_ui.task_cache_lock:
                            task_info = self.master_ui.task_status_cache.get(task_id)
                        
                        if task_info and isinstance(task_info, dict) and "task_id" in task_info:
                            current_status = task_info.get("status")
                            if current_status not in ["COMPLETED", "FAILED", "ERROR"]:
                                # 更新任務狀態
                                self.master_ui.poll_task_status(task_id)
                        
                    except Exception as e:
                        logging.warning(f"更新任務 {task_id} 狀態失敗: {e}")
                    
                    time.sleep(1)  # 避免過於頻繁的請求
                
            except Exception as e:
                logging.error(f"任務狀態更新循環錯誤: {e}")
            
            # 等待下次更新
            self._stop_event.wait(30)

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
