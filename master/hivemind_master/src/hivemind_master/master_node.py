import grpc
import os
import time
import logging
import zipfile
import datetime
from concurrent.futures import ThreadPoolExecutor, as_completed
import sys
sys.path.insert(0, os.path.abspath(os.path.dirname(__file__)))
import nodepool_pb2
import nodepool_pb2_grpc
from grpc_auth_client import add_token_to_metadata, get_metadata_list
import threading
from flask import Flask, jsonify, request, render_template, redirect, url_for, flash
import io
from functools import wraps
import uuid
import requests  # 新增


# --- Configuration ---
GRPC_SERVER_ADDRESS = os.environ.get('GRPC_SERVER_ADDRESS', '127.0.0.1:50051')
FLASK_SECRET_KEY = os.environ.get('FLASK_SECRET_KEY', 'a-default-master-secret-key')
# 移除預設的用戶名和密碼
MASTER_USERNAME = os.environ.get('MASTER_USERNAME')  # 不設默認值
MASTER_PASSWORD = os.environ.get('MASTER_PASSWORD')  # 不設默認值
UI_HOST = '0.0.0.0'
UI_PORT = 5002
logging.basicConfig(level=logging.INFO, format='%(asctime)s - %(levelname)s - [%(threadName)s] - %(message)s')

class MasterNodeUI:
    def __init__(self, grpc_address):  # 移除預設用戶名密碼參數
        self.grpc_address = grpc_address
        self.channel = None
        self.user_stub = None
        self.master_stub = None
        self.node_stub = None
        self.token = None
        self._stop_event = threading.Event()
        self.task_status_cache = {}
        self.task_cache_lock = threading.Lock()

        self.app = Flask(__name__, template_folder="templates_master", static_folder="static_master")
        self.app.secret_key = FLASK_SECRET_KEY
        # Log where Flask will load templates/static from to avoid confusion
        try:
            root_path = getattr(self.app, 'root_path', os.getcwd())
            tpl_folder = getattr(self.app, 'template_folder', 'templates') or 'templates'
            static_folder = getattr(self.app, 'static_folder', 'static') or 'static'
            logging.info(f"Flask root_path = {root_path}")
            logging.info(f"Flask templates = {os.path.abspath(os.path.join(root_path, tpl_folder))}")
            logging.info(f"Flask static = {os.path.abspath(os.path.join(root_path, static_folder))}")
        except Exception as _e:
            logging.debug(f"Unable to log Flask paths: {_e}")
        self.setup_flask_routes()
        
        # 用戶會話管理
        self.user_list = []
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

    # --- Token 相關輔助方法 ---
    def _refresh_token(self, username):
        """嘗試刷新指定用戶的 token。成功則更新並返回 True，失敗返回 False。"""
        user = self.get_user(username)
        if not user:
            logging.warning(f"Refresh token skipped, user {username} not found")
            return False
        old_token = user.get('token')
        if not old_token:
            logging.warning(f"Refresh token skipped, user {username} has no token")
            return False
        try:
            req = nodepool_pb2.RefreshTokenRequest(old_token=old_token)
            metadata = get_metadata_list(old_token)
            resp = self.user_stub.RefreshToken(req, metadata=metadata, timeout=15)
            if resp.success and resp.new_token:
                self.add_or_update_user(username, resp.new_token)
                logging.info(f"User {username} token refreshed successfully")
                return True
            else:
                logging.warning(f"User {username} token refresh failed: {resp.message}")
                return False
        except Exception as e:
            logging.error(f"User {username} token refresh error: {e}")
            return False

    def _maybe_refresh_token(self, username, force=False):
        """根據 token 使用時間決定是否刷新，或在 force=True 時強制刷新。"""
        user = self.get_user(username)
        if not user:
            return False
        # token 有效期 60 分鐘，超過 50 分鐘主動刷新
        login_time = user.get('login_time')
        if force:
            return self._refresh_token(username)
        if not login_time:
            return False
        age_minutes = (datetime.datetime.now() - login_time).total_seconds() / 60.0
        if age_minutes >= 50:  # 主動刷新
            logging.info(f"User {username} token age {age_minutes:.1f}m exceeds threshold, refreshing...")
            return self._refresh_token(username)
        return False

    def _connect_grpc(self):
        try:
            # 設定大檔案傳輸的 gRPC channel 選項
            channel_options = [
                ('grpc.max_receive_message_length', 1024 * 1024 * 1024),  # 1000MB
                ('grpc.max_send_message_length', 1024 * 1024 * 1024),     # 1000MB
            ]
            self.channel = grpc.insecure_channel(self.grpc_address, options=channel_options)
            grpc.channel_ready_future(self.channel).result(timeout=10)
            self.user_stub = nodepool_pb2_grpc.UserServiceStub(self.channel)
            self.master_stub = nodepool_pb2_grpc.MasterNodeServiceStub(self.channel)
            self.node_stub = nodepool_pb2_grpc.NodeManagerServiceStub(self.channel)
            logging.info(f"Successfully connected to gRPC server at {self.grpc_address}")
            return True
        except Exception as e:
            logging.error(f"Failed to connect to gRPC server: {e}")
            return False

    def login(self, username, password):  # 移除默認參數
        if not self.channel or not self.user_stub:
            logging.error("gRPC connection not established. Cannot login.")
            return False

        request = nodepool_pb2.LoginRequest(username=username, password=password)
        try:
            response = self.user_stub.Login(request, timeout=15)
            if response.success and response.token:
                self.add_or_update_user(username, response.token)
                logging.info(f"User {username} logged in successfully")
                return True
            else:
                return False
        except Exception as e:
            logging.error(f"Login error: {e}")
            return False

    def get_balance(self, username):
        user = self.get_user(username)
        if not user:
            return 0
        try:
            # 可能需要提前刷新 token
            self._maybe_refresh_token(username)
            req = nodepool_pb2.GetBalanceRequest(username=username, token=user['token'])
            metadata = get_metadata_list(user['token'])
            resp = self.user_stub.GetBalance(req, metadata=metadata, timeout=30)
            if resp.success:
                user['cpt_balance'] = resp.balance
                return resp.balance
            else:
                # 根據錯誤類型處理 TOKEN 過期或無效
                if resp.message in ("TOKEN_EXPIRED", "INVALID_TOKEN"):
                    logging.warning(f"GetBalance failed for {username}: {resp.message}, attempting refresh...")
                    if self._refresh_token(username):
                        # 重試一次
                        user = self.get_user(username)
                        retry_req = nodepool_pb2.GetBalanceRequest(username=username, token=user['token'])
                        metadata = get_metadata_list(user['token'])
                        retry_resp = self.user_stub.GetBalance(retry_req, metadata=metadata, timeout=30)
                        if retry_resp.success:
                            user['cpt_balance'] = retry_resp.balance
                            return retry_resp.balance
                        else:
                            logging.error(f"GetBalance retry failed for {username}: {retry_resp.message}")
                    else:
                        logging.error(f"Token refresh failed for {username}, balance unavailable")
                return 0
        except Exception as e:
            logging.error(f"GetBalance exception for {username}: {e}")
            return 0

    def get_tasks(self, username):
        user = self.get_user(username)
        if not user:
            return []
        try:
            # 主動刷新檢查
            self._maybe_refresh_token(username)
            req = nodepool_pb2.GetAllUserTasksRequest(token=user['token'])
            metadata = get_metadata_list(user['token'])
            resp = self.master_stub.GetAllUserTasks(req, metadata=metadata, timeout=30)
            if resp.tasks:
                return resp.tasks
            else:
                # 如果沒有任務返回，懷疑 token 過期再嘗試刷新一次
                if self._refresh_token(username):
                    user = self.get_user(username)
                    retry_req = nodepool_pb2.GetAllUserTasksRequest(token=user['token'])
                    metadata = get_metadata_list(user['token'])
                    retry_resp = self.master_stub.GetAllUserTasks(retry_req, metadata=metadata, timeout=30)
                    return list(retry_resp.tasks) if retry_resp.tasks else []
                return []
        except Exception as e:
            logging.error(f"get_tasks error for {username}: {e}")
            return []

    def upload_task_with_user(self, username, task_id, task_zip_bytes, requirements):
        user = self.get_user(username)
        if not user:
            logging.error(f"User {username} not found, cannot upload task")
            return task_id, False
        token = user['token']

        try:
            # 先檢查 token 年齡並必要時刷新
            self._maybe_refresh_token(username)
            token = self.get_user(username)['token']

            # Check user balance
            balance_request = nodepool_pb2.GetBalanceRequest(username=username, token=token)
            metadata = get_metadata_list(token)
            balance_response = self.user_stub.GetBalance(balance_request, metadata=metadata, timeout=30)
            if balance_response.success:
                user['cpt_balance'] = balance_response.balance

                # Enhanced cost calculation - support task priority
                memory_gb_val = float(requirements.get("memory_gb", 0))
                cpu_score_val = float(requirements.get("cpu_score", 0))
                gpu_score_val = float(requirements.get("gpu_score", 0))
                gpu_memory_gb_val = float(requirements.get("gpu_memory_gb", 0))
                base_cost = max(1, int(memory_gb_val + cpu_score_val / 100 + gpu_score_val / 100 + gpu_memory_gb_val))

                # Apply priority multiplier
                priority = requirements.get("task_priority", "normal")
                priority_multiplier = {"normal": 1.0, "high": 1.2, "urgent": 1.5}.get(priority, 1.0)
                cpt_cost = int(base_cost * priority_multiplier)

                if balance_response.balance < cpt_cost:
                    logging.error(f"User {username} insufficient balance: needs {cpt_cost} CPT (base: {base_cost}, priority: {priority}), but only has {balance_response.balance} CPT")
                    return task_id, False

                logging.info(f"Task {task_id} cost calculation: base {base_cost} CPT, priority {priority} (x{priority_multiplier}), total {cpt_cost} CPT")
            else:
                logging.error(f"Cannot get balance for user {username} (message={balance_response.message})")
                if balance_response.message in ("TOKEN_EXPIRED", "INVALID_TOKEN"):
                    if self._refresh_token(username):
                        return self.upload_task_with_user(username, task_id, task_zip_bytes, requirements)
                return task_id, False

            # Add host_count to the request
            host_count = int(requirements.get("host_count", 0))

            request = nodepool_pb2.UploadTaskRequest(
                task_id=task_id,
                task_zip=task_zip_bytes,
                memory_gb=int(requirements.get("memory_gb", 0)),
                cpu_score=int(requirements.get("cpu_score", 0)),
                gpu_score=int(requirements.get("gpu_score", 0)),
                gpu_memory_gb=int(requirements.get("gpu_memory_gb", 0)),
                location=requirements.get("location", "Any"),
                gpu_name=requirements.get("gpu_name", ""),
                token=token,
                host_count=host_count
            )
            metadata = [("authorization", f"Bearer {token}")]
            response = self.master_stub.UploadTask(request, metadata=metadata, timeout=60)

            if response.success:
                logging.info(f"Task {task_id} uploaded successfully with priority {priority}")
                with self.task_cache_lock:
                    self.task_status_cache[task_id] = {
                        "task_id": task_id,
                        "status": "PENDING",
                        "message": f"Task submitted (Priority: {priority})",
                        "last_polled": time.time(),
                        "priority": priority,
                        "estimated_cost": cpt_cost
                    }
                return task_id, True, None
            else:
                error_msg = f"NodePool rejected: {response.message}"
                logging.error(f"Task {task_id} upload failed: {error_msg}")
                return task_id, False, error_msg
        except grpc.RpcError as e:
            error_msg = f"gRPC error: {e.code()} - {e.details()}"
            logging.error(f"Task {task_id} upload RPC error: {error_msg}", exc_info=True)
            return task_id, False, error_msg
        except Exception as e:
            error_msg = f"Upload error: {str(e)}"
            logging.error(f"Task {task_id} upload error: {error_msg}", exc_info=True)
            return task_id, False, error_msg

    def setup_flask_routes(self):
        @self.app.route('/login', methods=['GET', 'POST'])
        def login():
            if request.method == 'POST':
                username = request.form.get('username')
                password = request.form.get('password')
                if not self.channel or not self.user_stub:
                    flash('Master not connected to node pool, please try again later.', 'error')
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
                return jsonify({"error": "Please login first", "cpt_balance": 0}), 401
            balance = self.get_balance(username)
            return jsonify({"cpt_balance": balance})

        @self.app.route('/api/tasks')
        def api_tasks():
            username = request.args.get('user')
            if not username or not self.get_user(username):
                return jsonify({"error": "Please login first", "tasks": []}), 401
            tasks = self.get_tasks(username)
            task_list = []
            for task in tasks:
                created_time = ""
                if getattr(task, "created_at", None):
                    try:
                        created_timestamp = float(task.created_at)
                        created_time = time.strftime('%H:%M:%S', time.localtime(created_timestamp))
                    except:
                        created_time = "Unknown"
                
                # Get task resource information
                resource_info = ""
                # Prefer worker_ip (present in TaskInfo) if assigned_node is missing
                assigned_node = getattr(task, 'assigned_node', '') or getattr(task, 'worker_ip', '')
                if assigned_node:
                    resource_info = f"Node: {assigned_node}"
                
                # Get additional info from cache
                cache_info = self.task_status_cache.get(task.task_id, {})
                priority = cache_info.get('priority', 'normal')
                estimated_cost = cache_info.get('estimated_cost', 0)
                
                priority_icons = {"normal": "🔵", "high": "🟡", "urgent": "🔴"}
                priority_text = f"{priority_icons.get(priority, '🔵')} {priority.title()}"
                
                # Usage metrics as float percentages (clamped 0-100 in node_pool)
                def to_float(v):
                    try:
                        return float(v)
                    except Exception:
                        return 0.0
                cpu_usage = to_float(getattr(task, 'cpu_usage', 0))
                memory_usage = to_float(getattr(task, 'memory_usage', 0))
                gpu_usage = to_float(getattr(task, 'gpu_usage', 0))
                gpu_memory_usage = to_float(getattr(task, 'gpu_memory_usage', 0))

                # Compute numeric values when limits are known
                try:
                    mem_limit_mb = int(float(getattr(task, 'memory_gb', 0) or 0) * 1024)
                except Exception:
                    mem_limit_mb = 0
                try:
                    gpu_mem_limit_mb = int(float(getattr(task, 'gpu_memory_gb', 0) or 0) * 1024)
                except Exception:
                    gpu_mem_limit_mb = 0

                mem_used_mb = int(round(memory_usage * mem_limit_mb / 100)) if mem_limit_mb else 0
                gpu_mem_used_mb = int(round(gpu_memory_usage * gpu_mem_limit_mb / 100)) if gpu_mem_limit_mb else 0

                task_list.append({
                    "task_id": task.task_id,
                    "status": task.status,
                    "progress": "100%" if task.status == "COMPLETED" else "75%" if task.status == "RUNNING" else "25%" if task.status == "PENDING" else "0%",
                    "message": f"Status: {task.status} | Priority: {priority_text}",
                    "last_update": created_time,
                    "assigned_node": assigned_node or 'Waiting for assignment',
                    "resource_info": resource_info,
                    "priority": priority,
                    "estimated_cost": estimated_cost,
                    "cpu_usage": round(cpu_usage, 2),
                    "memory_usage": round(memory_usage, 2),
                    "gpu_usage": round(gpu_usage, 2),
                    "gpu_memory_usage": round(gpu_memory_usage, 2),
                    # Numeric fields for UI that wants numbers instead of percentage
                    "mem_limit_mb": mem_limit_mb,
                    "mem_used_mb": mem_used_mb,
                    "gpu_mem_limit_mb": gpu_mem_limit_mb,
                    "gpu_mem_used_mb": gpu_mem_used_mb
                })
            return jsonify({"tasks": task_list})

        @self.app.route('/api/nodes')
        def api_nodes():
            username = request.args.get('user')
            if not username or not self.get_user(username):
                return jsonify({"error": "Please login first", "nodes": []}), 401
            if not self.node_stub:
                return jsonify({"error": "Not connected to gRPC server", "nodes": []}), 200
            try:
                # TODO: 實現節點列表功能 - 目前 proto 文件中沒有定義 GetNodeList 方法
                # 暫時返回空節點列表
                return jsonify({"nodes": []})
            except Exception as e:
                logging.error(f"API GetNodeList error: {e}")
                return jsonify({"error": "Failed to get node list", "nodes": []}), 200

        @self.app.route('/upload', methods=['GET', 'POST'])
        def upload_task_ui():
            username = request.args.get('user')
            if not username:
                flash('User parameter required', 'error')
                return redirect(url_for('login'))
            
            user = self.get_user(username)
            if not user:
                flash('User not logged in, please login again', 'error')
                return redirect(url_for('login'))
            
            if request.method == 'POST':
                logging.info(f"Received file upload request from user {username}")
                logging.info(f"Request files keys: {list(request.files.keys())}")
                logging.info(f"Request form keys: {list(request.form.keys())}")
                
                # 支援多檔案上傳
                files = request.files.getlist('task_zip')
                
                if not files or len(files) == 0:
                    logging.warning("No task_zip file field in request")
                    flash('Please select at least one ZIP file', 'error')
                    return render_template('master_upload.html', username=username)
                
                # 過濾掉空檔案
                valid_files = [f for f in files if f.filename and f.filename != '']
                
                if len(valid_files) == 0:
                    logging.warning("No valid files selected")
                    flash('No file selected, please choose ZIP file(s)', 'error')
                    return render_template('master_upload.html', username=username)
                
                logging.info(f"Received {len(valid_files)} file(s)")
                
                # Get repeat count
                try:
                    repeat_count = int(request.form.get('repeat_count', 1))
                    if repeat_count < 1 or repeat_count > 100:
                        flash('Repeat count must be between 1 and 100', 'error')
                        return render_template('master_upload.html', username=username)
                except ValueError:
                    flash('Invalid repeat count, please enter a number', 'error')
                    return render_template('master_upload.html', username=username)
                
                # 取得配置
                requirements = {
                    "memory_gb": request.form.get('memory_gb', 0),
                    "cpu_score": request.form.get('cpu_score', 0),
                    "gpu_score": request.form.get('gpu_score', 0),
                    "gpu_memory_gb": request.form.get('gpu_memory_gb', 0),
                    "location": request.form.get('location', 'Any'),
                    "gpu_name": request.form.get('gpu_name', ''),
                    "task_priority": request.form.get('task_priority', 'normal')
                }
                
                total_success = 0
                total_tasks = 0
                all_task_ids = []
                failed_files = []
                timestamp = datetime.datetime.now().strftime("%Y%m%d_%H%M%S")
                
                # 處理每個檔案
                for file_idx, file in enumerate(valid_files):
                    try:
                        logging.info(f"Processing file {file_idx + 1}/{len(valid_files)}: {file.filename}")
                        
                        # 驗證檔案格式
                        if not file.filename.lower().endswith('.zip'):
                            logging.warning(f"Invalid file format: {file.filename}")
                            failed_files.append(f"{file.filename} (invalid format)")
                            continue
                        
                        # 讀取檔案內容
                        file_content = file.read()
                        logging.info(f"Read file content, size: {len(file_content)} bytes")
                        
                        if len(file_content) == 0:
                            logging.warning(f"File content is empty: {file.filename}")
                            failed_files.append(f"{file.filename} (empty file)")
                            continue
                        
                        max_size = 1 * 1024 * 1024 * 1024  # 1000MB per file
                        if len(file_content) > max_size:
                            logging.warning(f"File too large: {file.filename} ({len(file_content)} bytes)")
                            failed_files.append(f"{file.filename} (exceeds 1000MB)")
                            continue
                        
                        # 驗證 ZIP 格式
                        try:
                            zip_buffer = io.BytesIO(file_content)
                            with zipfile.ZipFile(zip_buffer, 'r') as zip_file:
                                zip_file.testzip()
                                file_list = zip_file.namelist()
                                logging.info(f"ZIP validation OK: {file.filename}, {len(file_list)} files inside")
                        except zipfile.BadZipFile:
                            logging.warning(f"Invalid ZIP file: {file.filename}")
                            failed_files.append(f"{file.filename} (corrupted ZIP)")
                            continue
                        except Exception as e:
                            logging.error(f"ZIP validation error for {file.filename}: {e}")
                            failed_files.append(f"{file.filename} (validation error)")
                            continue
                        
                        # 取得檔案基礎名稱（去除 .zip）
                        base_name = os.path.splitext(file.filename)[0]
                        
                        # 對每個檔案執行 repeat_count 次上傳
                        for i in range(repeat_count):
                            total_tasks += 1
                            task_uuid = str(uuid.uuid4())[:8]
                            task_id = f"task_{timestamp}_{base_name}_{task_uuid}_{i+1}"
                            logging.info(f"Uploading task {task_id}")
                            
                            task_id, success, error_msg = self.upload_task_with_user(username, task_id, file_content, requirements)
                            if success:
                                total_success += 1
                                all_task_ids.append(task_id)
                            else:
                                error_detail = error_msg if error_msg else "Unknown error"
                                logging.error(f"Task {task_id} upload failed: {error_detail}")
                                failed_files.append(f"{file.filename} (task {i+1}: {error_detail})")
                                
                    except Exception as e:
                        logging.error(f"Error processing file {file.filename}: {e}", exc_info=True)
                        failed_files.append(f"{file.filename} (processing error)")
                        continue
                
                # 產生結果訊息
                if total_tasks == 0:
                    flash('No valid files to upload', 'error')
                elif total_success == total_tasks:
                    if len(all_task_ids) <= 5:
                        flash(f'Successfully uploaded {total_success} task(s): {", ".join(all_task_ids)}', 'success')
                    else:
                        flash(f'Successfully uploaded {total_success} task(s) from {len(valid_files)} file(s)', 'success')
                    logging.info(f"Successfully uploaded {total_success}/{total_tasks} tasks")
                elif total_success > 0:
                    flash(f'Partially uploaded: {total_success}/{total_tasks} tasks succeeded', 'warning')
                    if failed_files:
                        flash(f'Failed uploads: {"; ".join(failed_files[:3])}', 'error')
                else:
                    flash(f'All uploads failed ({total_tasks} tasks)', 'error')
                    if failed_files:
                        flash(f'Reasons: {"; ".join(failed_files[:5])}', 'error')
                
                return redirect(url_for('index') + f"?user={username}")
            
            return render_template('master_upload.html', username=username)

        @self.app.route('/api/stop_task/<task_id>', methods=['POST'])
        def api_stop_task(task_id):
            username = request.args.get('user')
            user = self.get_user(username)
            if not user:
                return jsonify({"success": False, "error": "User not logged in"}), 401
            
            try:
                logging.info(f"User {username} requested to stop task {task_id}")
                
                req = nodepool_pb2.StopTaskRequest(task_id=task_id, token=user['token'])
                metadata = get_metadata_list(user['token'])
                response = self.master_stub.StopTask(req, metadata=metadata, timeout=60)
                
                if response.success:
                    with self.task_cache_lock:
                        if task_id in self.task_status_cache:
                            self.task_status_cache[task_id].update({
                                "status": "STOPPED",
                                "last_polled": time.time()
                            })
                    
                    logging.info(f"Task {task_id} stopped successfully")
                    return jsonify({
                        "success": True,
                        "message": f"Task {task_id} stopped successfully, worker node is packaging partial results",
                        "note": "Stopped tasks will still package partial results for download"
                    })
                else:
                    logging.warning(f"Node pool refused to stop task {task_id}: {response.message}")
                    return jsonify({
                        "success": False,
                        "error": f"Failed to stop task: {response.message}"
                    }), 400
                    
            except grpc.RpcError as e:
                logging.error(f"gRPC error stopping task {task_id}: {e.code()} - {e.details()}")
                return jsonify({
                    "success": False,
                    "error": f"Communication error: {e.details()}"
                }), 500
            except Exception as e:
                logging.error(f"Failed to stop task {task_id}: {e}")
                return jsonify({"success": False, "error": f"Internal error: {str(e)}"}), 500

        @self.app.route('/api/task_logs/<task_id>')
        def api_task_logs(task_id):
            username = request.args.get('user')
            user = self.get_user(username)
            if not user:
                return jsonify({"error": "User not logged in"}), 401
            
            try:
                # 呼叫節點池 gRPC 取得日誌
                req = nodepool_pb2.TasklogRequest(task_id=task_id, token=user['token'])
                metadata = get_metadata_list(user['token'])
                resp = self.master_stub.GetTasklog(req, metadata=metadata, timeout=30)

                logs_text = getattr(resp, 'log', '') or ''
                raw_lines = [line for line in logs_text.split('\n') if line.strip()]

                # 將純文字行轉成 {timestamp, content, level} 物件，避免前端出現 [undefined]
                lines = []
                for line in raw_lines:
                    ts, content, level = self._parse_log_line(line)
                    lines.append({
                        "timestamp": ts,
                        "content": content,
                        "level": level
                    })
                return jsonify({
                    "task_id": task_id,
                    "status": "OK" if getattr(resp, 'success', False) else "ERROR",
                    "message": "",
                    "logs": lines,
                    "total_logs": len(lines)
                })
            except Exception as e:
                logging.error(f"Failed to get task logs: {e}")
                return jsonify({"error": f"Failed to get logs: {str(e)}"}), 500

        @self.app.route('/api/download_result/<task_id>')
        def api_download_result(task_id):
            username = request.args.get('user')
            if not username:
                return jsonify({"error": "Missing user parameter"}), 400
                
            user = self.get_user(username)
            if not user:
                return jsonify({"error": "User not logged in or session expired"}), 401
            
            try:
                logging.info(f"User {username} requested to download results for task {task_id}")
                
                req = nodepool_pb2.GetTaskResultRequest(task_id=task_id, token=user['token'])
                metadata = get_metadata_list(user['token'])
                response = self.master_stub.GetTaskResult(req, metadata=metadata, timeout=60)
                
                if response.success and response.result_zip:
                    from flask import Response
                    
                    # Check if result is empty
                    if len(response.result_zip) == 0:
                        return jsonify({"error": "Task result is empty"}), 404
                    
                    def generate():
                        yield response.result_zip
                    
                    filename = f"{task_id}_result.zip"
                    logging.info(f"Starting download of task {task_id} result, file size: {len(response.result_zip)} bytes")
                    
                    return Response(
                        generate(),
                        mimetype='application/zip',
                        headers={
                            'Content-Disposition': f'attachment; filename="{filename}"',
                            'Content-Length': str(len(response.result_zip)),
                            'Cache-Control': 'no-cache'
                        }
                    )
                else:
                    error_msg = response.message if hasattr(response, 'message') else "Cannot get task result"
                    logging.warning(f"Failed to download task {task_id}: {error_msg}")
                    return jsonify({"error": error_msg}), 404
                    
            except grpc.RpcError as e:
                logging.error(f"gRPC error downloading task result: {e.code()} - {e.details()}")
                return jsonify({"error": f"Server communication error: {e.details()}"}), 500
            except Exception as e:
                logging.error(f"Failed to download task result: {e}", exc_info=True)
                return jsonify({"error": f"Download failed: {str(e)}"}), 500

    def _parse_log_line(self, line):
        """Simplified log parsing"""
        import re
        timestamp_pattern = r'\[(\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2})\]'
        timestamp_match = re.search(timestamp_pattern, line)
        
        if timestamp_match:
            timestamp = timestamp_match.group(1)
            content = re.sub(timestamp_pattern, '', line, count=1).strip()
        else:
            timestamp = time.strftime('%Y-%m-%d %H:%M:%S')
            content = line
        
        # Simplified level detection
        level = "info"
        content_upper = content.upper()
        if "ERROR" in content_upper or "FAILED" in content_upper:
            level = "error"
        elif "WARNING" in content_upper or "WARN" in content_upper:
            level = "warning"
        
        return timestamp, content, level

    def auto_join_vpn(self):
        """
        Master node automatically requests /api/vpn/join to get WireGuard config and attempts to connect VPN.
        If auto-connection fails, prompts user for manual connection.
        """
        try:
            api_url = "https://hivemind.justin0711.com/api/vpn/join"
            nodename = os.environ.get("COMPUTERNAME", "master")
            client_name = f"master-{nodename}-{os.getpid()}"
            resp = requests.post(api_url, json={"client_name": client_name}, timeout=15, verify=True)
            try:
                resp_json = resp.json()
            except Exception:
                resp_json = {}
            if resp.status_code == 200 and resp_json.get("success"):
                config_content = resp_json.get("config")
                config_path = os.path.join(os.path.dirname(os.path.abspath(__file__)), "wg0.conf")
                try:
                    with open(config_path, "w") as f:
                        f.write(config_content)
                    logging.info(f"Auto-obtained WireGuard config and wrote to {config_path}")
                except Exception as e:
                    logging.warning(f"Failed to write WireGuard config: {e}")
                    return
                # Try to start VPN automatically
                result = os.system(f"wg-quick down {config_path} 2>/dev/null; wg-quick up {config_path}")
                if result == 0:
                    logging.info("WireGuard VPN started successfully")
                else:
                    logging.warning("WireGuard VPN startup failed, please check permissions and configuration")
                    self.prompt_manual_vpn(config_path)
            else:
                error_msg = resp_json.get("error") if resp_json else resp.text
                logging.warning(f"Auto-obtaining WireGuard config failed: {error_msg}")
                if error_msg and "VPN service not available" in error_msg:
                    logging.warning("Please ensure master Flask has properly initialized WireGuardServer on startup and /api/vpn/join is available")
                self.prompt_manual_vpn()
        except Exception as e:
            logging.warning(f"Auto-requesting /api/vpn/join failed: {e}")
            self.prompt_manual_vpn()

    def prompt_manual_vpn(self, config_path=None):
        """Prompt user to manually connect WireGuard"""
        msg = (
            "\n[Notice] Master auto-connection to WireGuard failed, please manually connect VPN:\n"
            "1. Please find your config file (wg0.conf).\n"
            "2. Manually open WireGuard client and import configuration\n"
            "3. If you encounter permission issues, run as administrator/root.\n"
        )
        print(msg)
        print('If you have already connected, please press y')
        a = input()
        if a == 'y':
            logging.info("User confirmed manual WireGuard connection for master")

    def run(self):
        # Optional: auto-connect VPN first (disabled by default)
        try:
            if os.environ.get("MASTER_AUTO_VPN", "0") == "1":
                self.auto_join_vpn()
            else:
                logging.info("Skipping VPN auto-join (set MASTER_AUTO_VPN=1 to enable)")
        except Exception as e:
            logging.warning(f"Auto VPN join skipped due to error: {e}")

        # Try to connect gRPC; even if it fails, still bring up the web UI
        if not self._connect_grpc():
            logging.warning("Cannot connect to node pool gRPC. Web UI will still start; login/actions may be limited until connection is available.")

        # Remove auto-login logic, require manual login
        try:
            logging.info(f"Master starting on http://{UI_HOST}:{UI_PORT}")
            logging.info("Web UI will open shortly. Please login through the browser UI.")

            # Open browser after a short delay
            try:
                import threading, webbrowser, time as _time
                def _open():
                    _time.sleep(1.5)
                    try:
                        webbrowser.open(f"http://127.0.0.1:{UI_PORT}/login")
                    except Exception:
                        pass
                threading.Thread(target=_open, daemon=True).start()
            except Exception:
                pass

            self.app.run(host=UI_HOST, port=UI_PORT, debug=False)
        except KeyboardInterrupt:
            logging.info("Received interrupt signal, shutting down...")
        finally:
            if self.channel:
                self.channel.close()
def run_master_node():
    # Remove default username/password passing
    master_ui = MasterNodeUI(GRPC_SERVER_ADDRESS)
    try:
        master_ui.run()
    except KeyboardInterrupt:
        logging.info("Received interrupt signal, shutting down...")
    except Exception as e:
        logging.error(f"Error occurred while running master: {e}")
    finally:
        if master_ui.channel:
            master_ui.channel.close()
        logging.info("Master node has been shut down")


if __name__ == "__main__":
    run_master_node()