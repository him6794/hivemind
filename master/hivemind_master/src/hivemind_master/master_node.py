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
import threading
from flask import Flask, jsonify, request, render_template, redirect, url_for, flash
import io
from functools import wraps
import uuid
import requests  # 新增


# --- Configuration ---
GRPC_SERVER_ADDRESS = os.environ.get('GRPC_SERVER_ADDRESS', '10.0.0.1:50051')
FLASK_SECRET_KEY = os.environ.get('FLASK_SECRET_KEY', 'a-default-master-secret-key')
# 移除預設的用戶名和密碼
MASTER_USERNAME = os.environ.get('MASTER_USERNAME')  # 不設默認值
MASTER_PASSWORD = os.environ.get('MASTER_PASSWORD')  # 不設默認值
UI_HOST = '0.0.0.0'
UI_PORT = 5001
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

    def _connect_grpc(self):
        try:
            self.channel = grpc.insecure_channel(self.grpc_address)
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

    def upload_task_with_user(self, username, task_id, task_zip_bytes, requirements):
        user = self.get_user(username)
        if not user:
            logging.error(f"User {username} not found, cannot upload task")
            return task_id, False
        token = user['token']

        try:
            # Check user balance
            balance_request = nodepool_pb2.GetBalanceRequest(username=username, token=token)
            balance_response = self.user_stub.GetBalance(balance_request, timeout=30)
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
                logging.error(f"Cannot get balance for user {username}")
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
                return task_id, True
            else:
                logging.error(f"Task {task_id} upload failed: {response.message}")
                return task_id, False
        except Exception as e:
            logging.error(f"Task {task_id} upload error: {e}")
            return task_id, False

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
                if hasattr(task, 'assigned_node') and task.assigned_node:
                    resource_info = f"Node: {task.assigned_node}"
                
                # Get additional info from cache
                cache_info = self.task_status_cache.get(task.task_id, {})
                priority = cache_info.get('priority', 'normal')
                estimated_cost = cache_info.get('estimated_cost', 0)
                
                priority_icons = {"normal": "🔵", "high": "🟡", "urgent": "🔴"}
                priority_text = f"{priority_icons.get(priority, '🔵')} {priority.title()}"
                
                task_list.append({
                    "task_id": task.task_id,
                    "status": task.status,
                    "progress": "100%" if task.status == "COMPLETED" else "75%" if task.status == "RUNNING" else "25%" if task.status == "PENDING" else "0%",
                    "message": f"Status: {task.status} | Priority: {priority_text}",
                    "last_update": created_time,
                    "assigned_node": getattr(task, 'assigned_node', 'Waiting for assignment'),
                    "resource_info": resource_info,
                    "priority": priority,
                    "estimated_cost": estimated_cost
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
                
                if 'task_zip' not in request.files:
                    logging.warning("No task_zip file field in request")
                    logging.warning(f"Available file fields: {list(request.files.keys())}")
                    flash('Please select a ZIP file', 'error')
                    return render_template('master_upload.html', username=username)
                    
                file = request.files['task_zip']
                logging.info(f"Received file object: {file}")
                logging.info(f"File name: {file.filename}")
                logging.info(f"File content type: {file.content_type}")
                
                if not file.filename or file.filename == '':
                    logging.warning("Empty file name")
                    flash('No file selected, please choose a ZIP file', 'error')
                    return render_template('master_upload.html', username=username)
                
                if not file.filename.lower().endswith('.zip'):
                    logging.warning(f"Invalid file format: {file.filename}")
                    flash('Invalid file format, please upload a .zip file', 'error')
                    return render_template('master_upload.html', username=username)
                
                try:
                    file_content = file.read()
                    logging.info(f"Successfully read file content, size: {len(file_content)} bytes")
                    
                    if len(file_content) == 0:
                        logging.warning("File content is empty")
                        flash('Uploaded file is empty, please select a valid ZIP file', 'error')
                        return render_template('master_upload.html', username=username)
                    
                    max_size = 50 * 1024 * 1024
                    if len(file_content) > max_size:
                        logging.warning(f"File too large: {len(file_content)} bytes")
                        flash('File size exceeds 50MB limit', 'error')
                        return render_template('master_upload.html', username=username)
                    
                    try:
                        zip_buffer = io.BytesIO(file_content)
                        with zipfile.ZipFile(zip_buffer, 'r') as zip_file:
                            zip_file.testzip()
                            file_list = zip_file.namelist()
                            logging.info(f"ZIP file validation successful, contains {len(file_list)} files")
                            if len(file_list) > 0:
                                logging.info(f"ZIP content sample: {file_list[:5]}")
                    except zipfile.BadZipFile:
                        logging.warning("Invalid ZIP file")
                        flash('Invalid ZIP file, please ensure the file is not corrupted', 'error')
                        return render_template('master_upload.html', username=username)
                    except Exception as e:
                        logging.error(f"ZIP file validation error: {e}")
                        flash('File validation failed, please try uploading again', 'error')
                        return render_template('master_upload.html', username=username)
                    
                    logging.info(f"File validation passed: {file.filename}, size: {len(file_content)} bytes")
                    
                    # Get repeat count
                    try:
                        repeat_count = int(request.form.get('repeat_count', 1))
                        if repeat_count < 1 or repeat_count > 100:
                            flash('Repeat count must be between 1 and 100', 'error')
                            return render_template('master_upload.html', username=username)
                    except ValueError:
                        flash('Invalid repeat count, please enter a number', 'error')
                        return render_template('master_upload.html', username=username)
                    
                    requirements = {
                        "memory_gb": request.form.get('memory_gb', 0),
                        "cpu_score": request.form.get('cpu_score', 0),
                        "gpu_score": request.form.get('gpu_score', 0),
                        "gpu_memory_gb": request.form.get('gpu_memory_gb', 0),
                        "location": request.form.get('location', 'Any'),
                        "gpu_name": request.form.get('gpu_name', ''),
                        "task_priority": request.form.get('task_priority', 'normal')
                    }
                    
                    success_count = 0
                    task_ids = []
                    timestamp = datetime.datetime.now().strftime("%Y%m%d_%H%M%S")
                    
                    for i in range(repeat_count):
                        task_uuid = str(uuid.uuid4())[:8]
                        task_id = f"task_{timestamp}_{task_uuid}_{i+1}"
                        logging.info(f"Preparing to upload task {task_id}, requirements: {requirements}")
                        task_id, success = self.upload_task_with_user(username, task_id, file_content, requirements)
                        if success:
                            success_count += 1
                            task_ids.append(task_id)
                        else:
                            logging.error(f"Task {task_id} upload failed")
                    
                    if success_count == repeat_count:
                        flash(f'Successfully uploaded {success_count}/{repeat_count} tasks: {", ".join(task_ids)}', 'success')
                        logging.info(f"Successfully uploaded {success_count}/{repeat_count} tasks")
                    else:
                        flash(f'Only {success_count}/{repeat_count} tasks uploaded successfully: {", ".join(task_ids)}', 'warning')
                        logging.warning(f"Only {success_count}/{repeat_count} tasks uploaded successfully")
                    
                    return redirect(url_for('index') + f"?user={username}")
                        
                except Exception as e:
                    logging.error(f"Error processing uploaded file: {e}", exc_info=True)
                    flash('Error processing file, please try again later', 'error')
                    return render_template('master_upload.html', username=username)
            
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
                response = self.master_stub.StopTask(req, timeout=60)
                
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
                req = nodepool_pb2.GetTaskLogsRequest(task_id=task_id, token=user['token'])
                response = self.master_stub.GetTaskLogs(req, timeout=10)
                
                if response.success:
                    formatted_logs = []
                    if response.logs:
                        for line in response.logs.split('\n'):
                            if line.strip():
                                timestamp, content, level = self._parse_log_line(line)
                                formatted_logs.append({
                                    "timestamp": timestamp,
                                    "content": content,
                                    "level": level
                                })
                    
                    # Get task status
                    status_request = nodepool_pb2.PollTaskStatusRequest(task_id=task_id)
                    status_response = self.master_stub.PollTaskStatus(status_request, timeout=10)
                    
                    return jsonify({
                        "task_id": task_id,
                        "status": status_response.status if status_response else "UNKNOWN",
                        "message": response.message,
                        "logs": formatted_logs,
                        "total_logs": len(formatted_logs)
                    })
                else:
                    return jsonify({"error": response.message, "logs": []}), 404
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
                response = self.master_stub.GetTaskResult(req, timeout=60)
                
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
        # Auto-connect VPN first
        self.auto_join_vpn()
        if not self._connect_grpc():
            logging.error("Cannot connect to node pool, exiting")
            return

        # Remove auto-login logic, require manual login
        try:
            logging.info(f"Master started at http://{UI_HOST}:{UI_PORT}")
            logging.info("Please login through web interface to use console functions")
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