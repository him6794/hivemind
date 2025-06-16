import docker
import grpc
import threading
import logging
from flask import Flask, jsonify, request, render_template, session, redirect, url_for
import nodepool_pb2
import nodepool_pb2_grpc
from concurrent import futures
import os
import psutil
import time
import zipfile
import io
import sys
import tempfile
import subprocess
import platform
import datetime
import secrets
import shutil
import socket
import uuid

# 配置
logging.basicConfig(level=logging.INFO, format='%(asctime)s - %(levelname)s - [%(threadName)s] - %(message)s')

NODE_PORT = int(os.environ.get("NODE_PORT", 50053))
FLASK_PORT = int(os.environ.get("FLASK_PORT", 5000))
MASTER_ADDRESS = os.environ.get("MASTER_ADDRESS", "127.0.0.1:50051")
NODE_ID = os.environ.get("NODE_ID", f"worker-{platform.node().split('.')[0]}-{NODE_PORT}")

class WorkerNode:
    def __init__(self):
        self.node_id = NODE_ID
        self.port = NODE_PORT
        self.master_address = MASTER_ADDRESS
        self.flask_port = FLASK_PORT
        
        # 狀態管理
        self.status = "Initializing"
        self.current_task_id = None
        self.username = None
        self.token = None
        self.is_registered = False
        self.login_time = None
        self.cpt_balance = 0
        
        # 線程控制
        self.status_thread = None
        self._stop_event = threading.Event()
        self.logs = []
        self.log_lock = threading.Lock()
        
        # 硬體信息
        self._init_hardware()
        
        # Docker 初始化
        self._init_docker()
        
        # gRPC 連接
        self._init_grpc()
        
        # Flask 應用
        self._init_flask()
        
        self.status = "Waiting for Login"

        # 用戶會話管理 - 存在後端陣列，不存在瀏覽器
        self.user_sessions = {}  # session_id -> user_data
        self.session_lock = threading.Lock()

    def _init_hardware(self):
        """初始化硬體信息"""
        try:
            self.hostname = platform.node()
            self.cpu_cores = psutil.cpu_count(logical=True)
            self.memory_gb = round(psutil.virtual_memory().total / (1024**3), 2)
            self.location = "Unknown"
            
            # 獲取本機 IP
            self.local_ip = self._get_local_ip()
            
            # 簡化的效能計算
            self.cpu_score = self._benchmark_cpu()
            self.gpu_score, self.gpu_name, self.gpu_memory_gb = self._detect_gpu()
            
            self._log(f"Hardware: CPU={self.cpu_cores} cores, RAM={self.memory_gb:.1f}GB")
            self._log(f"Performance: CPU={self.cpu_score}, GPU={self.gpu_score}")
            self._log(f"Local IP: {self.local_ip}")
        except Exception as e:
            self._log(f"Hardware detection failed: {e}", logging.ERROR)
            # 設置預設值
            self.hostname = "unknown"
            self.cpu_cores = 1
            self.memory_gb = 1.0
            self.location = "Unknown"
            self.local_ip = "127.0.0.1"
            self.cpu_score = 0
            self.gpu_score = 0
            self.gpu_name = "Not Detected"
            self.gpu_memory_gb = 0.0

    def _get_local_ip(self):
        """獲取本機 IP 地址"""
        try:
            # 連接到一個不存在的地址來獲取本機 IP
            s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
            s.connect(("8.8.8.8", 80))
            ip = s.getsockname()[0]
            s.close()
            return ip
        except:
            return "127.0.0.1"

    def _benchmark_cpu(self):
        """簡化的 CPU 基準測試"""
        try:
            start_time = time.time()
            result = 0
            for i in range(10_000_000):
                result = (result + i * i) % 987654321
            duration = time.time() - start_time
            return int((10_000_000 / duration) / 1000) if duration > 0.01 else 10000
        except:
            return 1000

    def _detect_gpu(self):
        """簡化的 GPU 檢測"""
        try:
            if platform.system() == "Windows":
                cmd = 'wmic path Win32_VideoController get Name, AdapterRAM /VALUE'
                result = subprocess.run(cmd, capture_output=True, text=True, timeout=10, 
                                      creationflags=subprocess.CREATE_NO_WINDOW)
                output = result.stdout
                
                # 解析輸出
                lines = [line.strip() for line in output.split('\n') if '=' in line]
                data = {}
                for line in lines:
                    if '=' in line:
                        key, value = line.split('=', 1)
                        data[key.strip()] = value.strip()
                
                gpu_name = data.get('Name', 'Unknown')
                ram = data.get('AdapterRAM')
                gpu_memory_gb = round(int(ram) / (1024**3), 2) if ram and ram.isdigit() else 1.0
                
                if 'Microsoft Basic' not in gpu_name:
                    gpu_score = 500 + int(gpu_memory_gb * 200)
                    return gpu_score, gpu_name, gpu_memory_gb
            
            return 0, "Not Detected", 0.0
        except:
            return 0, "Detection Failed", 0.0

    def _init_docker(self):
        """初始化 Docker"""
        try:
            self.docker_client = docker.from_env(timeout=10)
            self.docker_client.ping()
            self.docker_available = True
            
            # 檢查鏡像
            try:
                self.docker_client.images.get("hivemind-worker:latest")
                self._log("Docker image found")
            except docker.errors.ImageNotFound:
                self._log("Docker image not found", logging.WARNING)
                
        except Exception as e:
            self._log(f"Docker initialization failed: {e}", logging.WARNING)
            self.docker_available = False
            self.docker_client = None

    def _init_grpc(self):
        """初始化 gRPC 連接"""
        try:
            self.channel = grpc.insecure_channel(self.master_address)
            grpc.channel_ready_future(self.channel).result(timeout=10)
            
            self.user_stub = nodepool_pb2_grpc.UserServiceStub(self.channel)
            self.node_stub = nodepool_pb2_grpc.NodeManagerServiceStub(self.channel)
            self.master_stub = nodepool_pb2_grpc.MasterNodeServiceStub(self.channel)
            
            self._log(f"Connected to master at {self.master_address}")
        except Exception as e:
            self._log(f"gRPC connection failed: {e}", logging.ERROR)
            sys.exit(1)

    def _init_flask(self):
        """初始化 Flask 應用"""
        self.app = Flask(__name__, template_folder="templates", static_folder="static")
        self.app.secret_key = secrets.token_hex(32)
        
        # 配置會話持久性，使用不同的cookie名稱避免與主控端衝突
        self.app.config.update(
            SESSION_COOKIE_NAME='worker_session',  # 與主控端不同的cookie名稱
            SESSION_COOKIE_SECURE=False,  # 如果使用HTTPS則設為True
            SESSION_COOKIE_HTTPONLY=True,
            SESSION_COOKIE_SAMESITE='Lax',
            SESSION_COOKIE_PATH='/',
            SESSION_COOKIE_DOMAIN=None,
            PERMANENT_SESSION_LIFETIME=datetime.timedelta(hours=24),  # 24小時會話
            SESSION_REFRESH_EACH_REQUEST=True  # 每次請求刷新會話
        )
        
        self._setup_routes()
        self._start_flask()

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

    def _setup_routes(self):
        """設置 Flask 路由"""
        @self.app.route('/')
        def index():
            return render_template('login.html', node_id=self.node_id, current_status=self.status)

        @self.app.route('/monitor')
        def monitor():
            session_id = session.get('session_id')
            user_data = self._get_user_session(session_id) if session_id else None
            
            if not user_data:
                return redirect(url_for('index'))
            
            return render_template('monitor.html', 
                                 username=user_data['username'],
                                 node_id=self.node_id, 
                                 initial_status=self.status)

        @self.app.route('/login', methods=['GET', 'POST'])
        def login_route():
            if request.method == 'GET':
                session_id = session.get('session_id')
                user_data = self._get_user_session(session_id) if session_id else None
                
                if user_data and user_data['username'] == self.username:
                    return redirect(url_for('monitor'))
                return render_template('login.html', node_id=self.node_id, current_status=self.status)

            # POST 登入
            username = request.form.get('username')
            password = request.form.get('password')
            
            if not username or not password:
                return render_template('login.html', error="請輸入用戶名和密碼", 
                                     node_id=self.node_id, current_status=self.status)

            if self._login(username, password) and self._register():
                # 創建會話並只在 session 中存儲會話ID
                session_id = self._create_user_session(username, self.token)
                session['session_id'] = session_id
                session.permanent = True
                
                self._log(f"User '{username}' logged in successfully, session created: {session_id[:8]}...")
                return redirect(url_for('monitor'))
            else:
                return render_template('login.html', error=f"登入失敗: {self.status}", 
                                     node_id=self.node_id, current_status=self.status)

        @self.app.route('/api/status')
        def api_status():
            session_id = session.get('session_id')
            user_data = self._get_user_session(session_id)
            
            if not user_data or user_data['username'] != self.username:
                return jsonify({'error': 'Unauthorized'}), 401
            
            try:
                cpu_percent = psutil.cpu_percent(interval=0.1)
                mem = psutil.virtual_memory()
            except:
                cpu_percent, mem = 0, None

            return jsonify({
                'node_id': self.node_id,
                'status': self.status,
                'current_task_id': self.current_task_id or "None",
                'is_registered': self.is_registered,
                'docker_available': self.docker_available,
                'cpu_percent': round(cpu_percent, 1),
                'cpu_cores': self.cpu_cores,
                'memory_percent': round(mem.percent, 1) if mem else 0,
                'memory_used_gb': round(mem.used/(1024**3), 2) if mem else 0,
                'memory_total_gb': self.memory_gb,
                'cpu_score': self.cpu_score,
                'gpu_score': self.gpu_score,
                'gpu_name': self.gpu_name,
                'gpu_memory_gb': self.gpu_memory_gb,
                'cpt_balance': user_data['cpt_balance'],
                'login_time': user_data['login_time'].isoformat(),
                'ip': getattr(self, 'local_ip', '127.0.0.1')
            })

        @self.app.route('/api/logs')
        def api_logs():
            session_id = session.get('session_id')
            user_data = self._get_user_session(session_id)
            
            if not user_data or user_data['username'] != self.username:
                return jsonify({'error': 'Unauthorized'}), 401
                
            with self.log_lock:
                return jsonify({'logs': list(self.logs)})

        @self.app.route('/logout')
        def logout():
            session_id = session.get('session_id')
            if session_id:
                self._clear_user_session(session_id)
            
            session.clear()
            self._logout()
            return redirect(url_for('index'))

    def _start_flask(self):
        """啟動 Flask 服務"""
        def run_flask():
            try:
                self.app.run(host='0.0.0.0', port=self.flask_port, debug=False, 
                           use_reloader=False, threaded=True)
            except Exception as e:
                self._log(f"Flask failed to start: {e}", logging.ERROR)
                os._exit(1)
        
        threading.Thread(target=run_flask, daemon=True).start()
        self._log(f"Flask started on port {self.flask_port}")

    def _login(self, username, password):
        """登入到節點池"""
        try:
            response = self.user_stub.Login(
                nodepool_pb2.LoginRequest(username=username, password=password), 
                timeout=15
            )
            if response.success and response.token:
                self.username = username
                self.token = response.token
                self.login_time = datetime.datetime.now()
                self.status = "Logged In"
                self._log(f"User {username} logged in successfully")
                return True
            else:
                self.status = "Login Failed"
                self._log(f"Login failed for user {username}")
                return False
        except Exception as e:
            self._log(f"Login error: {e}", logging.ERROR)
            self.status = "Login Failed"
            return False

    def _register(self):
        """註冊節點"""
        if not self.token:
            return False

        try:
            request = nodepool_pb2.RegisterWorkerNodeRequest(
                node_id=self.username,
                hostname="127.0.0.1",
                cpu_cores=self.cpu_cores,
                memory_gb=int(self.memory_gb),
                cpu_score=self.cpu_score,
                gpu_score=self.gpu_score,
                gpu_name=self.gpu_name,
                gpu_memory_gb=int(self.gpu_memory_gb),
                location=self.location,
                port=self.port
            )
            
            response = self.node_stub.RegisterWorkerNode(
                request, 
                metadata=[('authorization', f'Bearer {self.token}')], 
                timeout=15
            )
            
            if response.success:
                self.node_id = self.username
                self.is_registered = True
                self.status = "Idle"
                self._start_status_reporting()
                return True
            else:
                self.status = f"Registration Failed: {response.message}"
                return False
                
        except Exception as e:
            self._log(f"Registration error: {e}", logging.ERROR)
            self.status = "Registration Failed"
            return False

    def _logout(self):
        """登出並清理狀態"""
        old_username = self.username
        self.token = None
        self.username = None
        self.is_registered = False
        self.status = "Waiting for Login"
        self.current_task_id = None
        self.login_time = None
        self.cpt_balance = 0
        self._stop_status_reporting()
        
        if old_username:
            self._log(f"User {old_username} logged out")

    def _start_status_reporting(self):
        """開始狀態報告"""
        if self.status_thread and self.status_thread.is_alive():
            return
        
        self._stop_event.clear()
        self.status_thread = threading.Thread(target=self._status_reporter, daemon=True)
        self.status_thread.start()

    def _stop_status_reporting(self):
        """停止狀態報告"""
        self._stop_event.set()
        if self.status_thread and self.status_thread.is_alive():
            self.status_thread.join(timeout=5)

    def _status_reporter(self):
        """狀態報告線程"""
        while not self._stop_event.is_set():
            if self.is_registered and self.token:
                try:
                    status_msg = f"Executing Task: {self.current_task_id}" if self.current_task_id else self.status
                    
                    # 無論是否在執行任務都要發送心跳
                    self.node_stub.ReportStatus(
                        nodepool_pb2.ReportStatusRequest(
                            node_id=self.node_id,
                            status_message=status_msg
                        ),
                        metadata=[('authorization', f'Bearer {self.token}')],
                        timeout=10
                    )
                    
                    # 更新餘額
                    self._update_balance()
                    
                    # 調試日誌
                    if self.current_task_id:
                        self._log(f"Heartbeat sent while executing task {self.current_task_id}")
                    
                except Exception as e:
                    self._log(f"Status report failed: {e}", logging.WARNING)
            
            # 縮短心跳間隔以確保連接穩定
            self._stop_event.wait(1) 

    def _update_balance(self):
        """更新 CPT 餘額"""
        try:
            if not self.username or not self.token:
                return
                
            response = self.user_stub.GetBalance(
                nodepool_pb2.GetBalanceRequest(username=self.username, token=self.token),
                metadata=[('authorization', f'Bearer {self.token}')],
                timeout=10
            )
            if response.success:
                # 更新所有該用戶的會話餘額
                with self.session_lock:
                    for session_id, session_data in self.user_sessions.items():
                        if session_data['username'] == self.username:
                            session_data['cpt_balance'] = response.balance
        except:
            pass

    def _send_task_logs(self, task_id, logs_content):
        """發送任務日誌到節點池"""
        if not self.master_stub or not self.token or not task_id:
            return False
            
        try:
            import time
            current_timestamp = int(time.time())  # 使用秒級時間戳而不是毫秒
            
            request = nodepool_pb2.StoreLogsRequest(
                node_id=self.node_id,
                task_id=task_id,
                logs=logs_content,
                timestamp=current_timestamp
            )
            
            response = self.master_stub.StoreLogs(
                request,
                metadata=[('authorization', f'Bearer {self.token}')],
                timeout=10
            )
            
            if response.success:
                self._log(f"Successfully sent logs for task {task_id}")
                return True
            else:
                self._log(f"Failed to send logs for task {task_id}: {response.message}")
                return False
                
        except Exception as e:
            self._log(f"Error sending logs for task {task_id}: {e}", logging.WARNING)
            return False

    def _execute_task(self, task_id, task_zip_bytes):
        """執行任務"""
        self.current_task_id = task_id
        self.status = f"Executing: {task_id}"
        
        temp_dir = None
        container = None
        success = False
        task_logs = []  # 收集任務日誌
        
        try:
            if not self.docker_available:
                raise RuntimeError("Docker not available")

            # 創建臨時目錄
            temp_dir = tempfile.mkdtemp(prefix=f"task_{task_id}_")
            workspace = os.path.join(temp_dir, "workspace")
            os.makedirs(workspace)

            # 解壓任務文件
            with zipfile.ZipFile(io.BytesIO(task_zip_bytes), 'r') as zip_ref:
                zip_ref.extractall(workspace)

            self._log(f"Task {task_id} files extracted to {workspace}")

            # 確保 run_task.sh 存在於 workspace
            script_src = os.path.join(os.path.dirname(__file__), "run_task.sh")
            script_dst = os.path.join(workspace, "run_task.sh")
            shutil.copy2(script_src, script_dst)
            os.chmod(script_dst, 0o755)

            # 執行容器 - 使用 workspace 內的腳本
            container_name = f"task-{task_id}-{secrets.token_hex(4)}"
            container = self.docker_client.containers.run(
                "hivemind-worker:latest",
                command=["bash", "/app/task/run_task.sh"],
                detach=True,
                name=container_name,
                volumes={workspace: {'bind': '/app/task', 'mode': 'rw'}},
                working_dir="/app/task",
                environment={"TASK_ID": task_id, "PYTHONUNBUFFERED": "1"},
                remove=False
            )
            
            self._log(f"Task {task_id} container started, collecting logs...")
            
            # 發送初始日誌
            initial_log = f"任務 {task_id} 開始執行\n容器名稱: {container_name}\n工作目錄: {workspace}"
            task_logs.append(initial_log)
            self._send_task_logs(task_id, initial_log)
            
            # 收集日誌 - 使用非阻塞方式並定期發送到節點池
            log_buffer = []
            log_send_counter = 0
            
            for log_line in container.logs(stream=True, follow=True):
                line = log_line.decode('utf-8', errors='replace').strip()
                if line:
                    # 本地日誌
                    self._log(f"[Task {task_id}]: {line}")
                    
                    # 收集到緩衝區
                    log_buffer.append(line)
                    task_logs.append(line)
                    log_send_counter += 1
                    
                    # 每20行或每30秒發送一次日誌到節點池
                    if log_send_counter >= 20:
                        logs_to_send = "\n".join(log_buffer)
                        self._send_task_logs(task_id, logs_to_send)
                        log_buffer.clear()
                        log_send_counter = 0
                
                # 檢查容器是否還在運行
                try:
                    container.reload()
                    if container.status != 'running':
                        break
                except:
                    break
            
            # 發送剩餘的日誌
            if log_buffer:
                logs_to_send = "\n".join(log_buffer)
                self._send_task_logs(task_id, logs_to_send)
            
            # 等待完成
            result = container.wait(timeout=3600)
            success = result.get('StatusCode', -1) == 0
            
            completion_log = f"任務 {task_id} 執行完成，退出碼: {result.get('StatusCode', -1)}"
            self._log(completion_log)
            task_logs.append(completion_log)
            self._send_task_logs(task_id, completion_log)
            
            self._log(f"Task {task_id} completed with status: {success}")
            
            # 打包結果
            result_zip = self._create_result_zip(task_id, workspace, success)
            
            # 發送結果
            if result_zip:
                self.master_stub.ReturnTaskResult(
                    nodepool_pb2.ReturnTaskResultRequest(
                        task_id=task_id,
                        result_zip=result_zip
                    ),
                    metadata=[('authorization', f'Bearer {self.token}')],
                    timeout=30
                )
                
                result_log = f"任務 {task_id} 結果已發送到節點池"
                self._log(result_log)
                task_logs.append(result_log)
                self._send_task_logs(task_id, result_log)
            
        except Exception as e:
            error_log = f"任務 {task_id} 執行失敗: {str(e)}"
            self._log(error_log, logging.ERROR)
            task_logs.append(error_log)
            self._send_task_logs(task_id, error_log)
            success = False
        
        finally:
            # 清理
            if container:
                try:
                    container.stop(timeout=10)
                    container.remove(force=True)
                    cleanup_log = f"任務 {task_id} 容器已清理"
                    self._log(cleanup_log)
                    task_logs.append(cleanup_log)
                    self._send_task_logs(task_id, cleanup_log)
                except Exception as e:
                    cleanup_error = f"清理容器失敗: {str(e)}"
                    self._log(cleanup_error, logging.WARNING)
                    task_logs.append(cleanup_error)
                    self._send_task_logs(task_id, cleanup_error)
            
            if temp_dir and os.path.exists(temp_dir):
                try:
                    shutil.rmtree(temp_dir)
                    self._log(f"Temporary directory {temp_dir} cleaned up")
                except Exception as e:
                    self._log(f"Failed to clean up temp dir: {e}", logging.WARNING)
            
            # 發送最終的完整日誌
            try:
                final_logs = "\n".join(task_logs)
                self._send_task_logs(task_id, f"=== 任務完整日誌 ===\n{final_logs}\n=== 日誌結束 ===")
            except Exception as e:
                self._log(f"Failed to send final logs: {e}", logging.WARNING)
            
            # 通知完成
            try:
                self.master_stub.TaskCompleted(
                    nodepool_pb2.TaskCompletedRequest(
                        task_id=task_id,
                        node_id=self.node_id,
                        success=success
                    ),
                    metadata=[('authorization', f'Bearer {self.token}')],
                    timeout=10
                )
            except Exception as e:
                self._log(f"Failed to notify task completion: {e}", logging.WARNING)
            
            # 重置狀態
            self.current_task_id = None
            self.status = "Idle"
            self._log(f"Task {task_id} cleanup completed, status reset to Idle")

    def _create_result_zip(self, task_id, workspace, success):
        """創建結果 ZIP"""
        try:
            # 創建執行日誌
            log_file = os.path.join(workspace, "execution_log.txt")
            with open(log_file, 'w', encoding='utf-8') as f:
                f.write(f"Task ID: {task_id}\n")
                f.write(f"Status: {'Success' if success else 'Failed'}\n")
                f.write(f"Time: {datetime.datetime.now()}\n")
            
            # 打包整個工作目錄
            zip_buffer = io.BytesIO()
            with zipfile.ZipFile(zip_buffer, 'w', zipfile.ZIP_DEFLATED) as zip_file:
                for root, dirs, files in os.walk(workspace):
                    for file in files:
                        file_path = os.path.join(root, file)
                        arcname = os.path.relpath(file_path, workspace)
                        zip_file.write(file_path, arcname)
            
            return zip_buffer.getvalue()
        except Exception as e:
            self._log(f"Failed to create result zip: {e}", logging.ERROR)
            return None

    def _log(self, message, level=logging.INFO):
        """記錄日誌"""
        if level == logging.INFO:
            logging.info(message)
        elif level == logging.WARNING:
            logging.warning(message)
        elif level == logging.ERROR:
            logging.error(message)
        
        with self.log_lock:
            timestamp = datetime.datetime.now().strftime('%H:%M:%S')
            level_name = logging.getLevelName(level)
            self.logs.append(f"{timestamp} - {level_name} - {message}")
            if len(self.logs) > 500:  # 限制日誌數量
                self.logs.pop(0)

# gRPC 服務實現
class WorkerNodeServicer(nodepool_pb2_grpc.WorkerNodeServiceServicer):
    def __init__(self, worker_node):
        self.worker_node = worker_node

    def ExecuteTask(self, request, context):
        """執行任務 RPC"""
        task_id = request.task_id
        task_zip = request.task_zip
        
        if self.worker_node.current_task_id:
            return nodepool_pb2.ExecuteTaskResponse(
                success=False, 
                message=f"Node busy with task {self.worker_node.current_task_id}"
            )
        
        if not self.worker_node.docker_available:
            return nodepool_pb2.ExecuteTaskResponse(
                success=False, 
                message="Docker not available"
            )
        
        # 啟動執行線程
        threading.Thread(
            target=self.worker_node._execute_task,
            args=(task_id, task_zip),
            daemon=True
        ).start()
        
        return nodepool_pb2.ExecuteTaskResponse(
            success=True, 
            message=f"Task {task_id} started"
        )

    def ReportRunningStatus(self, request, context):
        """報告運行狀態"""
        task_id = request.task_id
        if self.worker_node.current_task_id == task_id:
            return nodepool_pb2.RunningStatusResponse(
                success=True,
                message=f"Task {task_id} running"
            )
        else:
            return nodepool_pb2.RunningStatusResponse(
                success=False,
                message=f"Not running task {task_id}"
            )

    def StopTaskExecution(self, request, context):
        """停止任務執行"""
        task_id = request.task_id
        if self.worker_node.current_task_id == task_id:
            self.worker_node.current_task_id = None
            self.worker_node.status = "Idle"
            return nodepool_pb2.StopTaskExecutionResponse(
                success=True,
                message=f"Task {task_id} stopped"
            )
        else:
            return nodepool_pb2.StopTaskExecutionResponse(
                success=False,
                message=f"Task {task_id} not running"
            )

if __name__ == "__main__":
    try:
        # 創建工作節點
        worker = WorkerNode()
        
        # 啟動 gRPC 服務
        server = grpc.server(futures.ThreadPoolExecutor(max_workers=5))
        nodepool_pb2_grpc.add_WorkerNodeServiceServicer_to_server(
            WorkerNodeServicer(worker), server
        )
        
        server.add_insecure_port(f'[::]:{NODE_PORT}')
        server.start()
        
        worker._log(f"Worker Node started on port {NODE_PORT}")
        worker._log(f"Flask UI: http://localhost:{FLASK_PORT}")
        
        # 保持運行
        try:
            while True:
                time.sleep(60)
        except KeyboardInterrupt:
            worker._log("Shutting down...")
            worker._stop_status_reporting()
            server.stop(grace=5)
            
    except Exception as e:
        logging.critical(f"Failed to start worker: {e}")
        sys.exit(1)
