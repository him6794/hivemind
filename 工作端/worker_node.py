# --- START OF UPDATED FILE worker_node.py ---
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
import tarfile
import io
import sys
import tempfile
import subprocess
import platform
import re
import datetime
import secrets
import jwt
import shutil

# --- 配置日誌 ---
logging.basicConfig(
    level=logging.INFO,
    format='%(asctime)s - %(levelname)s - [%(threadName)s] - %(message)s',
    handlers=[logging.StreamHandler(sys.stdout)]
)

# --- 從環境變數或預設值讀取設定 ---
FLASK_SECRET_KEY = os.environ.get("FLASK_SECRET_KEY", secrets.token_hex(32))
MASTER_ADDRESS = os.environ.get("MASTER_ADDRESS", "127.0.0.1:50051")
NODE_PORT = int(os.environ.get("NODE_PORT", 50053))
FLASK_PORT = int(os.environ.get("FLASK_PORT", 5000))
NODE_ID = os.environ.get("NODE_ID", f"worker-{platform.node().split('.')[0]}-{NODE_PORT}")
DEFAULT_LOCATION = os.environ.get("NODE_LOCATION", "Unknown")
DOCKER_BASE_IMAGE = "hivemind-worker"
KEEP_CONTAINERS = os.environ.get("KEEP_CONTAINERS", "false").lower() == "true"
STATUS_REPORT_INTERVAL = int(os.environ.get("STATUS_REPORT_INTERVAL", 30))
MAX_LOG_ENTRIES = int(os.environ.get("MAX_LOG_ENTRIES", 500))
MAX_TASK_ZIP_SIZE = int(os.environ.get("MAX_TASK_ZIP_SIZE", 50 * 1024 * 1024))

def start_docker_service():
    """嘗試啟動 Docker 服務"""
    try:
        if platform.system() == 'Windows':
            logging.info("Attempting to start Docker service on Windows...")
            subprocess.run(['net', 'start', 'com.docker.service'], check=True, timeout=30)
            time.sleep(15)
            logging.info("Docker service start command executed.")
            return True
        elif platform.system() == 'Linux':
            logging.info("Attempting to start Docker service on Linux...")
            subprocess.run(['sudo', 'systemctl', 'start', 'docker'], check=True, timeout=30)
            time.sleep(5)
            logging.info("Docker service start command executed.")
            return True
        else:
            logging.warning("Unsupported OS for automatic Docker start.")
            return False
    except FileNotFoundError:
        logging.error("Docker start command not found (e.g., 'net' or 'systemctl'). Is the OS supported and tools installed?")
        return False
    except subprocess.CalledProcessError as e:
        logging.error(f"Starting Docker service failed: {e}")
        return False
    except subprocess.TimeoutExpired:
        logging.error("Starting Docker service timed out.")
        return False
    except Exception as e:
        logging.error(f"Unknown error starting Docker service: {e}")
        return False

# --- WorkerNode 類 ---
class WorkerNode:
    def __init__(self, node_id=NODE_ID, port=NODE_PORT, master_address=MASTER_ADDRESS):
        self.node_id = node_id
        self.port = port
        self.master_address = master_address
        self.current_task_id = None
        self.status = "Initializing"
        self.username = None
        self.token = None
        self.logs = []
        self.is_registered = False
        self.status_thread = None
        self._stop_event = threading.Event()
        self.log_lock = threading.Lock()
        self.flask_port = FLASK_PORT
        self.login_time = None
        self.cpt_balance = 0  # Changed to int to match proto int64
        self.docker_available = False
        self.docker_client = None
        self.docker_client_low = None
        self.local_ip = self._get_local_ip()

        self._log_and_append(f"Initializing Worker Node: {self.node_id} (gRPC Port: {self.port}, Flask Port: {self.flask_port})")
        try:
            self.hostname = platform.node()
            self.cpu_cores = psutil.cpu_count(logical=True)
            self.memory_gb = round(psutil.virtual_memory().total / (1024**3), 2) # Keep float for internal use/display
            self.location = DEFAULT_LOCATION
            self.cpu_score, self.gpu_score, self.gpu_name, self.gpu_memory_gb = self.calculate_performance_score() # Keep float for internal use/display
            self._log_and_append(f"Hardware: CPU={self.cpu_cores} cores, RAM={self.memory_gb:.1f}GB, Location={self.location}")
            self._log_and_append(f"Performance Scores: CPU={self.cpu_score}, GPU={self.gpu_score}")
            self._log_and_append(f"Detected GPU: Name='{self.gpu_name}', VRAM={self.gpu_memory_gb:.2f}GB")
        except Exception as hw_err:
            logging.error(f"Hardware detection failed: {hw_err}", exc_info=True)
            # Keep default initializations
            self.hostname = "unknown_host"
            self.cpu_cores = 1
            self.memory_gb = 1.0
            self.location = "Unknown"
            self.cpu_score, self.gpu_score, self.gpu_name, self.gpu_memory_gb = 0, 0, "Detection Failed", 0.0
            self.status = "Error - HW Detect Failed"
            self._log_and_append(f"Error: Hardware detection failed: {hw_err}", level=logging.ERROR)


        try:
            self._init_docker_client()
        except Exception as e:
            logging.warning(f"Docker client initialization failed: {e}", exc_info=True)
            self._log_and_append(f"Warning: Docker not available, task execution will fail.", level=logging.WARNING)
            self.docker_available = False # Ensure it's False if init fails

        # Connect to Master
        try:
            self.channel = grpc.insecure_channel(self.master_address)
            grpc.channel_ready_future(self.channel).result(timeout=10)
            self._log_and_append(f"Successfully connected to Master Node at {self.master_address}")
        except grpc.FutureTimeoutError:
            logging.critical(f"FATAL: Timed out connecting to Master at {self.master_address}")
            self._log_and_append(f"Error: Timeout connecting to Master", level=logging.ERROR)
            sys.exit(1)
        except Exception as e:
            logging.critical(f"FATAL: Failed to create gRPC channel to Master: {e}", exc_info=True)
            self._log_and_append(f"Error: Failed to create gRPC channel", level=logging.ERROR)
            sys.exit(1)

        self.user_stub = nodepool_pb2_grpc.UserServiceStub(self.channel)
        self.node_stub = nodepool_pb2_grpc.NodeManagerServiceStub(self.channel)
        self.master_stub = nodepool_pb2_grpc.MasterNodeServiceStub(self.channel)

        # Setup Flask App
        self.app = Flask(__name__, template_folder="templates", static_folder="static")
        self.app.secret_key = FLASK_SECRET_KEY
        self.app.config['SESSION_COOKIE_NAME'] = 'worker_session'
        self.app.config['PERMANENT_SESSION_LIFETIME'] = datetime.timedelta(days=7)
        self.app.config['SESSION_COOKIE_SAMESITE'] = 'Lax'
        self.app.config['SESSION_COOKIE_SECURE'] = False # Change to True if using HTTPS
        self.app.config['SESSION_TYPE'] = 'filesystem'
        self.app.config['SESSION_FOLDER'] = 'flask_session' # Explicitly set session folder
        self.app.config['WORKER_NODE_INSTANCE'] = self
        self.app.config['MAX_LOG_ENTRIES'] = MAX_LOG_ENTRIES
        self.setup_flask_routes()
        self.start_flask_service()

        self.status = "Waiting for Login"

    def _get_local_ip(self):
        """取得本機對外 IP（僅顯示用，不傳給 master）"""
        import socket
        s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        try:
            s.connect(("8.8.8.8", 80))
            ip = s.getsockname()[0]
        except Exception:
            ip = "127.0.0.1"
        finally:
            s.close()
        return ip

    def _init_docker_client(self):
        """Initialize Docker client and build the base image if it doesn't exist."""
        docker_available_flag = False
        for attempt in range(2):
            try:
                self._log_and_append(f"Attempting Docker connection (Attempt {attempt+1})...")
                base_url = 'npipe:////./pipe/docker_engine' if platform.system() == 'Windows' else 'unix://var/run/docker.sock'
                self.docker_client_low = docker.APIClient(base_url=base_url, timeout=10)
                self.docker_client_low.ping()
                self.docker_client = docker.from_env(timeout=10)
                self.docker_client.ping()
                docker_available_flag = True
                self._log_and_append("Docker client connected successfully.")
                break
            except Exception as e:
                self._log_and_append(f"Docker connection failed (Attempt {attempt+1}): {e}", level=logging.WARNING)
                if attempt == 0:
                    self._log_and_append("Attempting to start Docker service...")
                    if not start_docker_service():
                         self._log_and_append("Failed to start Docker service. Docker unavailable.", level=logging.ERROR)
                         break
                    else:
                        self._log_and_append("Docker service start initiated, retrying connection...")
                        time.sleep(5)
                else:
                    self._log_and_append("Docker connection failed after attempting service start.", level=logging.ERROR)

        if not docker_available_flag:
            self.docker_available = False
            self.docker_client = None
            self.docker_client_low = None
            raise ConnectionError("Docker service is not running or accessible.")

        self.docker_available = True
        # Check/Build image
        try:
            self.docker_client.images.get(f"{DOCKER_BASE_IMAGE}:latest")
            self._log_and_append(f"Docker image '{DOCKER_BASE_IMAGE}:latest' found.")
        except docker.errors.ImageNotFound:
            self._log_and_append(f"Docker image '{DOCKER_BASE_IMAGE}:latest' not found. Attempting to build...", level=logging.WARNING)
            # (Keep image build logic - assumes Dockerfile and run_task.sh are present)
            dockerfile_path = os.path.join(os.path.dirname(__file__), "Dockerfile")
            run_script_path = os.path.join(os.path.dirname(__file__), "run_task.sh")
            build_context_path = os.path.dirname(__file__)

            if not os.path.exists(dockerfile_path) or not os.path.exists(run_script_path):
                 missing = "Dockerfile" if not os.path.exists(dockerfile_path) else "run_task.sh"
                 self._log_and_append(f"{missing} not found in {build_context_path}. Cannot build image.", level=logging.ERROR)
                 self.docker_available = False
                 raise FileNotFoundError(f"{missing} not found in {build_context_path}")

            try:
                 self._log_and_append(f"Building image '{DOCKER_BASE_IMAGE}:latest' from context '{build_context_path}'...")
                 if platform.system() != "Windows":
                     try: os.chmod(run_script_path, 0o755)
                     except OSError as chmod_err: logging.warning(f"Could not set execute permission on run_task.sh: {chmod_err}")

                 image, build_log = self.docker_client.images.build(
                     path=build_context_path, dockerfile="Dockerfile", tag=f"{DOCKER_BASE_IMAGE}:latest", rm=True, forcerm=True
                 )
                 self._log_and_append(f"Successfully built image '{image.tags[0]}'.")
            except docker.errors.BuildError as e:
                 self._log_and_append(f"Failed to build Docker image: {e}", level=logging.ERROR)
                 for line in e.build_log:
                     if 'stream' in line: logging.error(f"Build Error Log: {line['stream'].strip()}")
                     elif 'errorDetail' in line: logging.error(f"Build Error Detail: {line['errorDetail']}")
                 self.docker_available = False
                 raise e
            except Exception as e:
                 self._log_and_append(f"An unexpected error occurred during image build: {e}", level=logging.ERROR)
                 self.docker_available = False
                 raise e
        except Exception as e:
             self._log_and_append(f"Error during Docker image check/build: {e}", level=logging.CRITICAL)
             self.docker_available = False
             raise e

    def calculate_performance_score(self):
        # (Keep the existing calculate_performance_score method as it is)
        # It calculates scores internally, which are then rounded to int for proto.
        cpu_score = 0
        gpu_score = 0
        gpu_name = "N/A"
        gpu_memory_gb = 0.0
        try:
            start_time = time.time()
            result = 0
            for i in range(20_000_000): # Reduced iterations for faster startup
                result = (result + i * i) % 987654321
            duration = time.time() - start_time
            # Adjusted scaling factor based on reduced iterations
            cpu_score = int((20_000_000 / duration) / 1000) if duration > 0.01 else 99999
            logging.info(f"CPU benchmark: {duration:.3f}s, Score: {cpu_score}")
        except Exception as e:
            logging.error(f"CPU benchmark failed: {e}", exc_info=True)

        logging.info(f"Detecting GPU (Platform: {sys.platform})...")
        detected_gpu = False
        is_basic_driver = False
        try:
            if sys.platform.startswith("linux"):
                try:
                    cmd = "lspci -nnk | grep -i -E 'VGA compatible|3D controller' -A 3"
                    res = subprocess.run(cmd, shell=True, capture_output=True, text=True, check=True, timeout=5)
                    out = res.stdout.strip()
                    if out:
                        m = re.search(r':\s*(.*?)\s*\[', out.splitlines()[0]) or re.search(r':\s*(.*)', out.splitlines()[0])
                        if m:
                            gpu_name = m.group(1).strip()
                            detected_gpu = True
                            logging.info(f"Linux GPU: {gpu_name}")
                        # Attempt to get VRAM using nvidia-smi if available
                        try:
                            res_vram = subprocess.run(['nvidia-smi', '--query-gpu=memory.total', '--format=csv,noheader,nounits'], capture_output=True, text=True, check=True, timeout=5)
                            gpu_memory_gb = round(int(res_vram.stdout.strip()) / 1024, 2)
                            logging.info(f"VRAM (nvidia-smi): {gpu_memory_gb:.2f} GB")
                        except (FileNotFoundError, subprocess.CalledProcessError, ValueError):
                            logging.info("nvidia-smi not found or failed, VRAM detection skipped on Linux.")
                            gpu_memory_gb = 0.0 # Reset if nvidia-smi fails
                except Exception as e:
                    logging.warning(f"lspci failed: {e}")
            elif sys.platform == "win32":
                try:
                    cmd = 'wmic path Win32_VideoController get Name, AdapterRAM /VALUE'
                    res = subprocess.run(cmd, capture_output=True, text=True, check=True, timeout=10, creationflags=subprocess.CREATE_NO_WINDOW)
                    d = {k.strip(): v.strip() for k, v in (line.split('=', 1) for line in res.stdout.splitlines() if '=' in line)}
                    gpu_name = d.get("Name", "N/A")
                    ram_str = d.get("AdapterRAM")
                    if gpu_name != "N/A":
                        if "Microsoft Basic" in gpu_name:
                            is_basic_driver = True
                            logging.info("Basic display adapter found.")
                        else:
                            detected_gpu = True
                            logging.info(f"Windows GPU: {gpu_name}")
                        if ram_str:
                            try:
                                gpu_memory_gb = round(int(ram_str) / (1024**3), 2)
                                logging.info(f"VRAM: {gpu_memory_gb:.2f} GB")
                            except ValueError:
                                logging.warning(f"Parsing AdapterRAM failed: {ram_str}")
                except Exception as e:
                    logging.warning(f"wmic failed: {e}")
            elif sys.platform == "darwin":
                try:
                    cmd = "/usr/sbin/system_profiler SPDisplaysDataType"
                    res = subprocess.run(cmd, shell=True, capture_output=True, text=True, check=True, timeout=10)
                    out = res.stdout
                    cm = re.search(r'Chipset Model:\s*(.*)', out)
                    vm = re.search(r'VRAM.*:\s*(\d+)\s*([GMK]B)', out)
                    if cm:
                        gpu_name = cm.group(1).strip()
                        detected_gpu = True
                        logging.info(f"macOS GPU: {gpu_name}")
                    if vm:
                        v, u = int(vm.group(1)), vm.group(2).upper()
                        gpu_memory_gb = float(v) if u == 'GB' else round(float(v) / (1024 if u == 'MB' else 1024**2), 2)
                        logging.info(f"VRAM: {gpu_memory_gb:.2f} GB")
                    elif detected_gpu: # Integrated GPUs might not report VRAM explicitly
                        try:
                             mem_total_bytes = psutil.virtual_memory().total
                             gpu_memory_gb = min(2.0, round(mem_total_bytes / (8 * 1024**3), 2))
                             logging.info(f"VRAM not explicit, estimating shared memory: ~{gpu_memory_gb:.2f} GB")
                        except Exception:
                             gpu_memory_gb = 0.5 # Fallback
                             logging.info("VRAM not explicit, assuming shared (fallback 0.5GB).")
                except Exception as e:
                    logging.warning(f"system_profiler failed: {e}")
        except Exception as e:
                logging.error(f"GPU detection error: {e}", exc_info=True)

        if detected_gpu and not is_basic_driver:
            gpu_score = 500 + int(gpu_memory_gb * 200)
            logging.info(f"GPU Score calculated: {gpu_score}")
        else:
            gpu_name = gpu_name if gpu_name != "N/A" else "Not Detected"
            logging.info("No specific GPU or basic driver found. GPU score set to 0.")
            gpu_score = 0

        # Return raw calculated values; conversion to int happens when sending via proto
        return int(cpu_score), int(gpu_score), str(gpu_name), float(gpu_memory_gb)


    def _get_grpc_metadata(self):
        if self.token:
            return [('authorization', f'Bearer {self.token}')]
        else:
            self._log_and_append("Token missing for gRPC call. Cannot proceed.", logging.ERROR)
            return None

    def _log_and_append(self, message, level=logging.INFO):
        log_func = {
            logging.CRITICAL: logging.critical, logging.ERROR: logging.error,
            logging.WARNING: logging.warning, logging.INFO: logging.info,
            logging.DEBUG: logging.debug
        }.get(level, logging.info)
        log_func(message)
        with self.log_lock:
            timestamp = time.strftime('%Y-%m-%d %H:%M:%S')
            self.logs.append(f"{timestamp} - {logging.getLevelName(level)} - {message}")
            if len(self.logs) > MAX_LOG_ENTRIES:
                self.logs.pop(0)

    def login(self, username, password):
        self._log_and_append(f"Attempting login for '{username}'...")
        try:
            grpc.channel_ready_future(self.channel).result(timeout=10)
            resp = self.user_stub.Login(nodepool_pb2.LoginRequest(username=username, password=password), timeout=15)
            if resp.success and resp.token:
                self.token = resp.token
                self.username = username
                self.status = "Logged In, Registering..."
                self.login_time = datetime.datetime.now()
                self._log_and_append(f"User '{username}' logged in successfully.")
                self.query_cpt_balance() # Query balance after login
                return True
            else:
                self._log_and_append(f"Login failed for '{username}': {resp.message}", logging.WARNING)
                self.status = "Login Failed"
                return False
        except grpc.RpcError as e:
            self._log_and_append(f"Login gRPC failed: {e.code()} - {e.details()}", logging.ERROR)
            self.status = f"Login Failed ({e.code()})"
            return False
        except Exception as e:
            self._log_and_append(f"Login error: {e}", logging.ERROR)
            logging.error("Login exception", exc_info=True)
            self.status = "Login Failed (Error)"
            return False

    def register(self):
        if not self.token:
            self._log_and_append("Cannot register: No token. Please login first.", logging.ERROR)
            self.status = "Reg Failed (No Token)"
            return False
        if self.is_registered:
            self._log_and_append("Already registered.")
            return True

        self._log_and_append(f"Registering node {self.node_id}...")
        meta = self._get_grpc_metadata()
        if not meta:
            self._log_and_append("Token missing for registration.", logging.ERROR)
            self.status = "Reg Failed (Token Error)"
            return False

        # --- 修正: 統一使用用戶名作為 node_id ---
        mem_gb_int = int(round(self.memory_gb))
        gpu_mem_gb_int = int(round(self.gpu_memory_gb))

        req = nodepool_pb2.RegisterWorkerNodeRequest(
            node_id=self.username,            # 使用用戶名作為 node_id
            hostname=self.local_ip,           # 使用本機 IP 作為 hostname
            cpu_cores=self.cpu_cores,
            memory_gb=mem_gb_int,
            cpu_score=self.cpu_score,
            gpu_score=self.gpu_score,
            gpu_name=self.gpu_name,
            gpu_memory_gb=gpu_mem_gb_int,
            location=self.location,
            port=self.port
        )
        # ----------------------------------

        try:
            grpc.channel_ready_future(self.channel).result(timeout=10)
            resp = self.node_stub.RegisterWorkerNode(req, metadata=meta, timeout=15)
            if resp.success:
                self._log_and_append("Node registered successfully.")
                self.is_registered = True
                # 修正: 更新 node_id 為用戶名，確保狀態報告一致
                self.node_id = self.username
                self.status = "Idle"
                self.start_status_reporting()
                return True
            else:
                self._log_and_append(f"Registration failed: {resp.message}", logging.ERROR)
                self.status = f"Reg Failed: {resp.message}"
                self.is_registered = False
                self.stop_status_reporting()
                return False
        except grpc.RpcError as e:
            self._log_and_append(f"Registration gRPC failed: {e.code()} - {e.details()}", logging.ERROR)
            # (Keep existing RPC error handling)
            if e.code() == grpc.StatusCode.UNAUTHENTICATED:
                self.status = "Reg Failed (Auth Error)"
                self.token = None; self.username = None; self.login_time = None; self.is_registered = False
            elif e.code() in [grpc.StatusCode.UNAVAILABLE, grpc.StatusCode.DEADLINE_EXCEEDED]:
                self.status = "Reg Failed (Master Unavailable)"
                self.is_registered = False
            else:
                self.status = f"Reg Failed (RPC Error: {e.code()})"
                self.is_registered = False
            self.stop_status_reporting()
            return False
        except Exception as e:
            self._log_and_append(f"Registration error: {e}", logging.ERROR)
            logging.error("Reg exception", exc_info=True)
            self.status = "Reg Failed (Unknown Error)"
            self.is_registered = False
            self.stop_status_reporting()
            return False

    def query_cpt_balance(self):
        """Query CPT balance from the backend server."""
        if not self.is_registered or not self.token or not self.username:
             # Silently return if not ready to query balance
            return False
        meta = self._get_grpc_metadata()
        if not meta:
            self._log_and_append("Token missing for CPT balance query.", logging.WARNING)
            return False

        try:
            grpc.channel_ready_future(self.channel).result(timeout=5)

            # --- Adhere to proto definition ---
            request = nodepool_pb2.GetBalanceRequest(
                username=self.username,  # Field required by proto
                token=self.token         # Field required by proto
            )
            # ----------------------------------

            # Call GetBalance RPC using user_stub, still use metadata for auth layer
            response = self.user_stub.GetBalance(request, metadata=meta, timeout=10)

            if response.success:
                # Proto returns int64, store as int
                self.cpt_balance = int(response.balance)
                self._log_and_append(f"CPT balance updated: {self.cpt_balance} CPT") # Display as int
                return True
            else:
                self._log_and_append(f"Failed to query CPT balance: {response.message}", logging.WARNING)
                return False
        except grpc.RpcError as e:
            self._log_and_append(f"CPT balance query gRPC error: {e.code()} - {e.details()}", logging.ERROR)
            if e.code() == grpc.StatusCode.UNAUTHENTICATED:
                self.token = None; self.is_registered = False; self.status = "Error-Auth Failed"
            return False
        except Exception as e:
            self._log_and_append(f"CPT balance query error: {e}", logging.ERROR)
            logging.error("CPT query exception", exc_info=True)
            return False

    def setup_flask_routes(self):
        # (Keep existing Flask routes: index, monitor, login_route, logout)
        # Ensure login_route calls self.login() then self.register()

        @self.app.route('/')
        def index():
            return render_template('login.html',
                                   node_id=self.node_id,
                                   current_status=self.status)

        @self.app.route('/monitor')
        def monitor():
            if not session.get('username'):
                return redirect(url_for('index'))
            # self._log_and_append(f"Monitor page accessed by '{session.get('username')}'") # Reduce noise
            return render_template('monitor.html',
                                   username=session.get('username'),
                                   node_id=self.node_id,
                                   initial_status=self.status,
                                   max_log_entries=MAX_LOG_ENTRIES)

        @self.app.route('/login', methods=['GET', 'POST'])
        def login_route():
            if request.method == 'GET':
                if session.get('username'): return redirect(url_for('monitor'))
                return render_template('login.html', node_id=self.node_id, current_status=self.status)

            # POST
            username = request.form.get('username')
            password = request.form.get('password')
            if not username or not password:
                return render_template('login.html', error="請輸入用戶名和密碼", node_id=self.node_id, current_status=self.status)

            if self.login(username, password):
                if self.register():
                    session.permanent = True
                    session['username'] = username
                    self._log_and_append(f"User '{username}' session started.")
                    return redirect(url_for('monitor'))
                else:
                    login_error = f"登入成功但節點註冊失敗: {self.status}"
                    self._log_and_append(login_error, level=logging.ERROR)
                    if "Auth" in self.status: self.token = None
                    return render_template('login.html', error=login_error, node_id=self.node_id, current_status=self.status)
            else:
                login_error = f"登入失敗: {self.status}"
                self._log_and_append(login_error, level=logging.WARNING)
                return render_template('login.html', error=login_error, node_id=self.node_id, current_status=self.status)

        @self.app.route('/logout')
        def logout():
            logged_out_user = session.pop('username', None)
            session.clear()
            self.token = None; self.username = None; self.is_registered = False
            self.status = "Waiting for Login"; self.current_task_id = None
            self.login_time = None; self.cpt_balance = 0
            self.stop_status_reporting()
            if logged_out_user: self._log_and_append(f"User '{logged_out_user}' logged out.")
            else: self._log_and_append("Logout called, no active session found.")
            return redirect(url_for('index'))

        # Update /api/status to use correct types and format cpt_balance as int
        @self.app.route('/api/status')
        def get_status():
            if 'username' not in session:
                return jsonify({'error': 'Unauthorized', 'status': self.status}), 401

            try:
                cpu_percent = psutil.cpu_percent(interval=0.1)
                mem_info = psutil.virtual_memory()
            except Exception as ps_err:
                logging.warning(f"psutil query failed: {ps_err}")
                cpu_percent, mem_info = -1.0, None

            status_data = {
                'node_id': self.node_id,
                'status': self.status,
                'current_task_id': self.current_task_id or "None",
                'is_registered': self.is_registered,
                'docker_available': self.docker_available,
                'cpu_percent': round(cpu_percent, 1),
                'cpu_cores': self.cpu_cores,
                'memory_percent': round(mem_info.percent, 1) if mem_info else -1.0,
                'memory_used_gb': round(mem_info.used/(1024**3), 2) if mem_info else -1.0,
                'memory_total_gb': self.memory_gb,
                'cpu_score': self.cpu_score,
                'gpu_score': self.gpu_score,
                'gpu_name': self.gpu_name,
                'gpu_memory_gb': round(self.gpu_memory_gb, 2),
                'location': self.location,
                'grpc_port': self.port,
                'flask_port': self.flask_port,
                'master_address': self.master_address,
                'logged_in_user': session.get('username', 'N/A'),
                'login_time': self.login_time.isoformat() if self.login_time else None,
                'cpt_balance': self.cpt_balance,
                'ip': self.local_ip  # 新增本機 IP 顯示
            }
            return jsonify(status_data)

        @self.app.route('/api/logs')
        def get_logs():
            if 'username' not in session:
                return jsonify({'error': 'Unauthorized'}), 401
            with self.log_lock:
                logs_copy = list(self.logs)
            return jsonify({'logs': logs_copy})

    def start_flask_service(self):
        def run_flask():
            if logging.getLogger().getEffectiveLevel() > logging.DEBUG:
                 logging.getLogger('werkzeug').setLevel(logging.ERROR)
            try:
                self.app.run(host='0.0.0.0', port=self.flask_port, debug=False, use_reloader=False, threaded=True)
            except OSError as e:
                logging.critical(f"Flask start failed (Port {self.flask_port}): {e}", exc_info=True)
                self._log_and_append(f"CRITICAL: Flask Port {self.flask_port} Busy/Permission Error. Exiting.", logging.CRITICAL)
                os._exit(1)
            except Exception as e:
                logging.critical(f"Flask start failed with unexpected error: {e}", exc_info=True)
                self._log_and_append(f"CRITICAL: Flask Failed to Start. Exiting.", logging.CRITICAL)
                os._exit(1)
        threading.Thread(target=run_flask, name="FlaskThread", daemon=True).start()
        self._log_and_append(f"Flask server starting on http://0.0.0.0:{self.flask_port}")


    def report_status(self):
        # (Keep report_status loop logic)
        # Ensures ReportStatusRequest matches proto (node_id, status_message)
        self._log_and_append(f"Status reporting thread started.")
        balance_update_counter = 0
        report_interval = STATUS_REPORT_INTERVAL

        while not self._stop_event.is_set():
            next_report_time = time.monotonic() + report_interval
            if self.is_registered and self.token:
                meta = self._get_grpc_metadata()
                if not meta:
                    self._log_and_append("Token missing, stopping status report.", logging.WARNING)
                    self.is_registered = False
                    self.status = "Error - Token Lost"
                    self.stop_status_reporting()
                    continue

                # --- Report Status ---
                try:
                    grpc.channel_ready_future(self.channel).result(timeout=5)
                    current_status_msg = self.status
                    if self.current_task_id:
                         current_status_msg = f"Executing Task: {self.current_task_id}"
                    elif not self.docker_available:
                         current_status_msg = "Error - Docker Unavailable"

                    # Adheres to proto: ReportStatusRequest(node_id, status_message)
                    req = nodepool_pb2.ReportStatusRequest(
                        node_id=self.node_id,
                        status_message=current_status_msg
                        )
                    resp = self.node_stub.ReportStatus(req, metadata=meta, timeout=10)
                    if not resp.success:
                        self._log_and_append(f"Master status report rejected: {resp.message}", logging.WARNING)

                except grpc.RpcError as e:
                    logging.error(f"Status report RPC failed: {e.code()} - {e.details()}", exc_info=False)
                    if e.code() == grpc.StatusCode.UNAUTHENTICATED:
                        self._log_and_append("Authentication failed reporting status. Stopping.", logging.ERROR)
                        self.is_registered = False; self.token = None; self.username = None
                        self.status = "Error - Auth Failed"
                        self.stop_status_reporting()
                    elif e.code() in [grpc.StatusCode.UNAVAILABLE, grpc.StatusCode.DEADLINE_EXCEEDED]:
                        self._log_and_append(f"Master unavailable for status report ({e.code()}). Retrying later.", logging.WARNING)
                    else:
                         self._log_and_append(f"Unhandled RPC error during status report: {e.code()}", logging.ERROR)
                except Exception as e:
                    self._log_and_append(f"Status report unexpected error: {e}", logging.ERROR)
                    logging.error("Status report exception", exc_info=True)

                # --- Update CPT Balance Periodically ---
                balance_update_counter += 1
                if balance_update_counter >= 5:
                    # self._log_and_append("Attempting periodic CPT balance update...") # Reduce noise
                    if self.query_cpt_balance():
                        # self._log_and_append(f"Periodic CPT balance update successful: {self.cpt_balance} CPT") # Reduce noise
                        pass
                    else:
                        self._log_and_append("Periodic CPT balance update failed.", logging.WARNING)
                    balance_update_counter = 0

            else:
                 # logging.debug("Status reporting paused (not registered or no token).") # Reduce noise
                 report_interval = STATUS_REPORT_INTERVAL * 2

            wait_time = max(0, next_report_time - time.monotonic())
            if self._stop_event.wait(wait_time):
                 break

        self._log_and_append(f"Status reporting thread stopped.")

    def start_status_reporting(self):
        if not self.status_thread or not self.status_thread.is_alive():
            self._stop_event.clear()
            self.status_thread = threading.Thread(target=self.report_status, name="StatusReportThread", daemon=True)
            self.status_thread.start()
            self._log_and_append("Status reporting thread started.")
        else:
             self._log_and_append("Status reporting thread already running.")

    def stop_status_reporting(self):
        if self.status_thread and self.status_thread.is_alive():
            self._log_and_append("Requesting status reporting thread stop...")
            self._stop_event.set()
            self.status_thread = None
            self._log_and_append("Stop signal sent to status reporting thread.")
        # else: # Reduce noise
            # self._log_and_append("Status reporting thread not running or already stopped.")


    def _send_intermediate_output(self, task_id, output):
        """Sends log output or intermediate results to the master."""
        # Adheres to proto: StoreOutputRequest(task_id, output)
        if not self.is_registered or not self.token or not self.master_stub: return
        meta = self._get_grpc_metadata()
        if not meta: return

        max_output_len = 4000
        if len(output) > max_output_len: output = output[:max_output_len] + "... (truncated)"

        try:
            req = nodepool_pb2.StoreOutputRequest(task_id=task_id, output=output)
            self.master_stub.StoreOutput(req, metadata=meta, timeout=5)
        except grpc.RpcError as e:
            level = logging.ERROR if e.code() == grpc.StatusCode.UNAUTHENTICATED else logging.WARNING
            self._log_and_append(f"Failed to send output for task {task_id}: RPC Error {e.code()} - {e.details()}", level)
            if e.code() == grpc.StatusCode.UNAUTHENTICATED:
                 self.token = None; self.is_registered = False; self.status = "Error - Auth Failed"
        except Exception as e:
            self._log_and_append(f"Failed to send output for task {task_id}: {e}", logging.WARNING)


    def _notify_master_task_completion(self, task_id, success, error_message=""):
        """Notifies the master node about task completion status."""
        # --- Adhere to proto definition ---
        # TaskCompletedRequest(task_id, node_id, success)
        # The error_message is NOT part of the proto request. Log it locally only.
        if not self.master_stub:
            logging.error(f"Cannot notify master for task {task_id}: Master stub unavailable.")
            return
        if not self.is_registered or not self.token:
             logging.error(f"Cannot notify master for task {task_id}: Not registered or no token.")
             return

        meta = self._get_grpc_metadata()
        if not meta:
            logging.error(f"Cannot notify master for task {task_id}: Token missing.")
            return

        # Log the outcome locally, including error if any
        log_level = logging.INFO if success else logging.ERROR
        log_message = f"Task {task_id} {'succeeded' if success else 'failed'}"
        if not success and error_message:
             # Truncate local log message if needed
             max_local_error_len = 1024
             if len(error_message) > max_local_error_len:
                 error_message_log = error_message[:max_local_error_len] + "... (truncated)"
             else:
                 error_message_log = error_message
             log_message += f": {error_message_log}"
        self._log_and_append(log_message, level=log_level)

        # Create request according to proto (no error_message field)
        req = nodepool_pb2.TaskCompletedRequest(
            task_id=task_id,
            node_id=self.node_id,
            success=success
            # error_message field removed
        )
        # ----------------------------------

        try:
            grpc.channel_ready_future(self.channel).result(timeout=5)
            response = self.master_stub.TaskCompleted(req, metadata=meta, timeout=15)
            if response.success:
                logging.info(f"Master acknowledged task {task_id} completion.")
            else:
                logging.error(f"Master rejected task {task_id} completion notification: {response.message}")
        except grpc.RpcError as e:
            logging.error(f"Notify Master RPC failed for task {task_id}: {e.code()} - {e.details()}")
            if e.code() == grpc.StatusCode.UNAUTHENTICATED:
                self.token = None; self.is_registered = False; self.status = "Error - Auth Failed"
        except Exception as e:
            logging.error(f"Notify Master failed unexpectedly for task {task_id}: {str(e)}", exc_info=True)


    def _execute_task_thread(self, task_id, task_zip_bytes):
        """Executes the task in a Docker container within a separate thread."""
        self.current_task_id = task_id
        self.status = f"Starting Task: {task_id}"
        self._log_and_append(f"Task {task_id}: Execution thread started.")

        host_temp_dir = None
        container = None
        success = False
        error_message = "" # For local logging, not for TaskCompletedRequest

        try:
            if not self.docker_available or not self.docker_client:
                raise RuntimeError("Docker is not available on this node.")

            host_temp_dir = tempfile.mkdtemp(prefix=f"hivemind_{task_id}_")
            host_zip_path = os.path.join(host_temp_dir, "task_archive.zip")
            self._log_and_append(f"Task {task_id}: Host temp dir: {host_temp_dir}")
            try:
                with open(host_zip_path, "wb") as f: f.write(task_zip_bytes)
                self._log_and_append(f"Task {task_id}: Saved task archive.")
            except Exception as e: raise IOError(f"Failed to save task zip: {e}")

            container_name = f"hivemind-task-{task_id}-{secrets.token_hex(4)}"
            self.status = f"Running Task: {task_id} (Container: {container_name[:12]}...)"
            self._log_and_append(f"Task {task_id}: Starting container '{container_name}' image '{DOCKER_BASE_IMAGE}:latest'")

            try:
                container = self.docker_client.containers.run(
                    image=f"{DOCKER_BASE_IMAGE}:latest",
                    # --- THIS IS THE CORRECTED LINE ---
                    command=["/usr/local/bin/run_task.sh"], # Override the Dockerfile's CMD to run the task script
                    # ------------------------------------
                    detach=True,
                    name=container_name,
                    volumes={host_zip_path: {'bind': '/tmp/task_archive.zip', 'mode': 'ro'}},
                    working_dir="/app", # Should match run_task.sh and Dockerfile WORKDIR
                    remove=False,      # We remove it manually in the finally block after getting logs/results
                    security_opt=["no-new-privileges"], # Good security practice
                    user="appuser",    # As defined in Dockerfile
                    environment={"TASK_ID": task_id, "PYTHONUNBUFFERED": "1"} # Pass task ID and ensure unbuffered output
                    # Add resource limits if needed, e.g.:
                    # mem_limit="1g",
                    # cpu_period=100000, cpu_quota=50000, # 50% of one CPU
                )
                self._log_and_append(f"Task {task_id}: Container {container.id} started, executing /usr/local/bin/run_task.sh.") # Updated log
            except docker.errors.APIError as e:
                 if "No such image" in str(e): raise docker.errors.ImageNotFound(f"Image {DOCKER_BASE_IMAGE}:latest not found") from e
                 # Add more specific error handling if needed (e.g., port conflicts, though not applicable here)
                 else: raise RuntimeError(f"Docker API error starting container for task {task_id}: {e}") from e

            # Log streaming
            self._log_and_append(f"Task {task_id}: Streaming logs from container {container.id}...")
            log_stream = container.logs(stream=True, follow=True, stdout=True, stderr=True)
            for raw_line in log_stream:
                try:
                    line = raw_line.decode('utf-8', errors='replace').strip()
                    if line:
                        # Log to worker's console/file
                        logging.info(f"[Task {task_id} Log]: {line}")
                        # Send to master
                        self._send_intermediate_output(task_id, line)
                except Exception as log_err:
                     logging.warning(f"Task {task_id}: Error processing log stream line: {log_err}")

            # Wait for container completion
            self._log_and_append(f"Task {task_id}: Waiting for container {container.id} to finish...")
            # It's good to have a timeout on container.wait() to prevent indefinite blocking
            # The timeout should be configurable or sufficiently long for most tasks.
            wait_timeout_seconds = 3600 # Example: 1 hour, adjust as needed
            result = container.wait(timeout=wait_timeout_seconds)
            exit_code = result.get('StatusCode', -1) # Docker API returns StatusCode, -1 if not found
            self._log_and_append(f"Task {task_id}: Container {container.id} finished with exit code {exit_code}.")

            # Process results
            if exit_code == 0:
                success = True
                self._log_and_append(f"Task {task_id}: Execution successful (exit code 0).")
                results_archive_path = '/app/results.zip' # Path *inside* the container
                try:
                    self._log_and_append(f"Task {task_id}: Attempting to retrieve '{results_archive_path}' from container.")
                    bits, stat = container.get_archive(results_archive_path) # This gets a tar stream

                    zip_content = None
                    # The archive is a tar stream containing the file. We need to extract it.
                    with io.BytesIO(b"".join(bits)) as tar_stream_bytes_io: # Join all byte chunks
                        with tarfile.open(fileobj=tar_stream_bytes_io, mode='r') as tar:
                            # Find the actual 'results.zip' member in the tar archive
                            # (it might be nested if the path inside container was complex, but /app/results.zip should be direct)
                            zip_member_info = None
                            for member in tar.getmembers():
                                # Name might be e.g. 'results.zip' or './results.zip' if /app was WORKDIR
                                # Or 'app/results.zip' if path was absolute from root of tarred dir
                                # For simplicity, assume it's directly named results.zip or similar
                                if member.isfile() and 'results.zip' in member.name.lower(): # more robust check
                                    zip_member_info = member
                                    break

                            if zip_member_info:
                                extracted_file = tar.extractfile(zip_member_info)
                                if extracted_file:
                                    zip_content = extracted_file.read()
                                    self._log_and_append(f"Task {task_id}: Successfully extracted {len(zip_content)} bytes for results.zip.")
                                else:
                                    self._log_and_append(f"Task {task_id}: Could not extract file object for results.zip from tar.", level=logging.WARNING)
                            else:
                                self._log_and_append(f"Task {task_id}: 'results.zip' not found within the retrieved tar archive.", level=logging.WARNING)

                    if zip_content:
                        meta_res = self._get_grpc_metadata()
                        if meta_res:
                            self._log_and_append(f"Task {task_id}: Storing results ({len(zip_content)} bytes)...")
                            self.master_stub.StoreResult(
                                nodepool_pb2.StoreResultRequest(task_id=task_id, result_zip=zip_content),
                                metadata=meta_res, timeout=30 # Timeout for potentially large results
                            )
                            self._log_and_append(f"Task {task_id}: Results stored successfully.")
                        else:
                            self._log_and_append(f"Task {task_id}: Cannot store results, token missing.", level=logging.WARNING)

                except docker.errors.NotFound:
                    self._log_and_append(f"Task {task_id}: No '{results_archive_path}' found in container. Task considered successful without results file.")
                except tarfile.ReadError as tar_err:
                    success = False # If results are expected but corrupt, task might be considered failed
                    error_message = f"Failed to read results archive from container: {tar_err}"
                    self._log_and_append(f"Task {task_id}: {error_message}", level=logging.ERROR)
                except grpc.RpcError as grpc_err:
                     success = False # If storing result fails, this is an issue
                     error_message = f"Failed to store results via gRPC: {grpc_err.code()} - {grpc_err.details()}"
                     self._log_and_append(f"Task {task_id}: {error_message}", level=logging.ERROR)
                except Exception as res_err:
                    success = False # Any other error in result processing
                    error_message = f"Error processing/storing results: {res_err}"
                    self._log_and_append(f"Task {task_id}: {error_message}", level=logging.ERROR)
                    logging.error(f"Task {task_id} Result Exception", exc_info=True)

            else: # exit_code != 0
                success = False
                error_message = f"Task execution failed inside container with exit code {exit_code}."
                self._log_and_append(error_message, level=logging.ERROR)
                # Attempt to get last few log lines for more context on failure
                try:
                    # Get final logs after container stop, might have more info
                    final_logs = container.logs(tail=20).decode('utf-8', errors='replace')
                    error_message += f"\n--- Last 20 lines of container log ---\n{final_logs}\n------------------------------------"
                    # Also log these to worker's main log for debugging
                    logging.error(f"Task {task_id} Failed Container Logs (Final):\n{final_logs}")
                except Exception as log_fetch_err:
                    logging.warning(f"Task {task_id}: Could not fetch final logs after failure: {log_fetch_err}")


        # Handle specific exceptions from the try block
        except docker.errors.ImageNotFound as e:
             error_msg = f"Docker image '{DOCKER_BASE_IMAGE}:latest' not found. Build required. {e}"
             logging.critical(error_msg)
             self.status = "Error - Docker Image Missing" # Update worker status
             success = False; error_message = error_msg
        except docker.errors.APIError as e: # Catch other Docker API errors
             error_msg = f"Docker API error during task {task_id}: {e}"
             logging.error(error_msg, exc_info=True)
             self.status = "Error - Docker API"
             success = False; error_message = error_msg
        except RuntimeError as e: # Catch runtime errors like Docker unavailable or container start failure
             error_msg = f"Runtime error during task {task_id}: {e}"
             logging.error(error_msg, exc_info=True)
             self.status = f"Error - Runtime ({type(e).__name__})"
             success = False; error_message = error_msg
        except IOError as e: # Catch file saving errors
             error_msg = f"I/O error during task {task_id}: {e}"
             logging.error(error_msg, exc_info=True)
             self.status = "Error - Task I/O"
             success = False; error_message = error_msg
        except Exception as e:
             # Catch-all for unexpected errors during execution
             error_msg = f"Unexpected error during task {task_id} execution: {e}"
             logging.error(error_msg, exc_info=True)
             self.status = "Error - Unexpected Task Failure"
             success = False; error_message = error_msg

        finally:
            # Cleanup: Remove the container
            if container:
                try:
                    self._log_and_append(f"Task {task_id}: Attempting to stop and remove container {container.id}...")
                    container.stop(timeout=10) # Give it a chance to stop gracefully
                    container.remove(force=True) # force=True if stop fails or to ensure removal
                    self._log_and_append(f"Task {task_id}: Container {container.id} removed.")
                except docker.errors.NotFound:
                     self._log_and_append(f"Task {task_id}: Container {container.id} already removed or not found.")
                except docker.errors.APIError as cleanup_err: # Catch Docker API errors during cleanup
                    logging.warning(f"Task {task_id}: API error removing container {container.id}: {cleanup_err}")
                except Exception as cleanup_err: # Catch other unexpected errors
                    logging.warning(f"Task {task_id}: Unexpected error removing container {container.id}: {cleanup_err}")

            # Cleanup: Remove the host temporary directory
            if host_temp_dir and os.path.exists(host_temp_dir):
                try:
                    self._log_and_append(f"Task {task_id}: Removing host temp directory {host_temp_dir}...")
                    shutil.rmtree(host_temp_dir)
                    self._log_and_append(f"Task {task_id}: Host temp directory {host_temp_dir} removed.")
                except OSError as cleanup_err: # Catch errors like permission denied or dir not empty
                    logging.warning(f"Task {task_id}: Error removing host temp directory {host_temp_dir}: {cleanup_err}")

            # Notify Master of final completion status
            # error_message here is for *local logging only* within _notify_master_task_completion
            self._notify_master_task_completion(task_id, success, error_message)

            # Reset Worker Status
            if self.current_task_id == task_id: # Ensure this is still the task we think it is
                self.current_task_id = None
                # Set status based on outcome, unless a critical error occurred earlier
                # that already set a more specific error status for the worker itself.
                if not self.status.startswith("Error -"): # Avoid overwriting more specific worker errors
                     self.status = "Idle" if success else f"Error - Task Failed ({task_id})"
                self._log_and_append(f"Task {task_id}: Worker status updated to '{self.status}'.")

    def run_grpc_server(self):
        server = grpc.server(futures.ThreadPoolExecutor(max_workers=10, thread_name_prefix="gRPCWorker"))
        nodepool_pb2_grpc.add_WorkerNodeServiceServicer_to_server(WorkerNodeServiceServicer(self), server)
        listen_addr = f'0.0.0.0:{self.port}'
        try:
            server.add_insecure_port(listen_addr)
            server.start()
            self._log_and_append(f"gRPC server started, listening on {listen_addr}")
            server.wait_for_termination()
        except OSError as e:
            logging.critical(f"gRPC bind failed on {listen_addr}: {e}", exc_info=True)
            self._log_and_append(f"CRITICAL: gRPC Port {self.port} Busy/Permission Error. Exiting.", logging.CRITICAL)
            os._exit(1)
        except Exception as e:
            logging.critical(f"gRPC server failed unexpectedly: {e}", exc_info=True)
            self._log_and_append(f"CRITICAL: gRPC Failed to Start. Exiting.", logging.CRITICAL)
            os._exit(1)
        finally:
             server.stop(grace=5)
             self._log_and_append("gRPC server shut down.")


    def run(self):
        """Main execution loop for the worker node."""
        self._log_and_append(f"Worker Node {self.node_id} starting run cycle.")
        try:
            self.run_grpc_server()
        except Exception as e:
            logging.critical(f"Worker main run loop encountered a critical error: {e}", exc_info=True)
            self._log_and_append(f"CRITICAL: Worker run failed. Exiting.", logging.CRITICAL)
        finally:
            self._log_and_append(f"Worker Node {self.node_id} run cycle ending. Final cleanup...")
            self.stop_status_reporting()
            if self.channel:
                self.channel.close()
                self._log_and_append("Closed gRPC channel to Master.")
            self._log_and_append("Cleanup finished.")

# --- gRPC Servicer 類 ---
class WorkerNodeServiceServicer(nodepool_pb2_grpc.WorkerNodeServiceServicer):
    def __init__(self, worker_node_instance: WorkerNode):
        self.worker_node = worker_node_instance
        logging.info("WorkerNodeServiceServicer initialized.")

    def ExecuteTask(self, request, context):
        """Handles ExecuteTask RPC call from the Master."""
        # Adheres to proto: ExecuteTaskRequest(node_id, task_id, task_zip)
        task_id = request.task_id
        task_zip_bytes = request.task_zip
        # node_id = request.node_id # Can be used for verification if needed
        logging.info(f"gRPC ExecuteTask request received for Task ID: {task_id} ({len(task_zip_bytes)} bytes)")

        if self.worker_node.current_task_id:
            error_msg = f"Node {self.worker_node.node_id} busy with {self.worker_node.current_task_id}."
            logging.error(error_msg)
            context.set_details(error_msg); context.set_code(grpc.StatusCode.RESOURCE_EXHAUSTED)
            # Adheres to proto: ExecuteTaskResponse(success, message, result) - result likely unused on error
            return nodepool_pb2.ExecuteTaskResponse(success=False, message=error_msg)

        if not self.worker_node.docker_available:
             error_msg = f"Node {self.worker_node.node_id} cannot accept task: Docker unavailable."
             logging.error(error_msg)
             context.set_details(error_msg); context.set_code(grpc.StatusCode.FAILED_PRECONDITION)
             return nodepool_pb2.ExecuteTaskResponse(success=False, message=error_msg)

        if len(task_zip_bytes) > MAX_TASK_ZIP_SIZE:
            error_msg = f"Task {task_id} ZIP size exceeds limit."
            logging.error(error_msg)
            context.set_details(error_msg); context.set_code(grpc.StatusCode.INVALID_ARGUMENT)
            return nodepool_pb2.ExecuteTaskResponse(success=False, message=error_msg)

        # Accept task
        self.worker_node.current_task_id = task_id
        self.worker_node.status = f"Accepted Task: {task_id}"
        logging.info(f"Task {task_id} accepted by {self.worker_node.node_id}. Starting execution thread.")

        try:
            execution_thread = threading.Thread(
                target=self.worker_node._execute_task_thread,
                args=(task_id, task_zip_bytes), name=f"TaskExec-{task_id}", daemon=True
            )
            execution_thread.start()
            # Adheres to proto: ExecuteTaskResponse(success, message, result) - result unused on success accept
            return nodepool_pb2.ExecuteTaskResponse(
                success=True, message=f"Task {task_id} accepted and started."
            )
        except Exception as e:
            self.worker_node.current_task_id = None
            self.worker_node.status = "Error - Task Start Failed"
            error_msg = f"Failed to start execution thread for task {task_id}: {str(e)}"
            logging.error(error_msg, exc_info=True)
            # Notify master of failure to start (TaskCompleted is used for final status)
            self.worker_node._notify_master_task_completion(task_id, False, error_msg) # error_msg for local log only
            context.set_details(error_msg); context.set_code(grpc.StatusCode.INTERNAL)
            return nodepool_pb2.ExecuteTaskResponse(success=False, message=error_msg)

    def ReportOutput(self, request, context):
        # This RPC seems redundant if worker uses StoreOutput to send logs to Master.
        # Implement if the worker needs to *receive* output reports *from* the master.
        # If it's meant for worker->master, StoreOutput should be used instead.
        logging.warning(f"Received unexpected ReportOutput call for task {request.task_id}. Ignoring.")
        # Adheres to proto: StatusResponse(success, message)
        return nodepool_pb2.StatusResponse(success=False, message="ReportOutput RPC not implemented/used by worker")


    def ReportRunningStatus(self, request, context):
        """Handles polling from master, potentially for rewards."""
        # Adheres to proto: RunningStatusRequest(node_id, task_id)
        node_id = self.worker_node.node_id
        # task_id_req = request.task_id # Can verify if needed
        status_msg = self.worker_node.status
        is_running_task = self.worker_node.current_task_id is not None

        # Simple reward logic (int64)
        reward = 0 # Default reward (int)
        if is_running_task:
            # Example: Give 1 unit if busy
            reward = 1
            status_msg = f"Running Task: {self.worker_node.current_task_id}"

        # Adheres to proto: RunningStatusResponse(success, message, cpt_reward)
        # is_busy field removed
        return nodepool_pb2.RunningStatusResponse(
                success=True,
                message=status_msg,
                cpt_reward=reward # Send int
                # is_busy field removed
                )

# --- 主程式入口 ---
if __name__ == "__main__":
    script_dir = os.path.dirname(os.path.abspath(__file__))
    template_dir = os.path.join(script_dir, "templates")
    static_dir = os.path.join(script_dir, "static")
    session_dir = os.path.join(script_dir, "flask_session") # Session dir
    os.makedirs(template_dir, exist_ok=True)
    os.makedirs(static_dir, exist_ok=True)
    os.makedirs(session_dir, exist_ok=True) # Create session dir

    # Create default templates if they don't exist
    # (Keep the template generation logic, ensure monitor.html displays int balance)
    login_html_path = os.path.join(template_dir, "login.html")
    monitor_html_path = os.path.join(template_dir, "monitor.html")

    # Login Template (No changes needed for proto alignment)
    worker = None
    try:
        worker = WorkerNode()
        worker.run()
    except KeyboardInterrupt:
        logging.info("Ctrl+C detected. Initiating shutdown...")
    except SystemExit as e:
        logging.warning(f"System exit called with code: {e.code}")
    except ConnectionError as e:
         logging.critical(f"Initialization failed due to connection error: {e}")
    except FileNotFoundError as e:
         logging.critical(f"Initialization failed due to missing file: {e}")
    except Exception as e:
        logging.critical(f"Unhandled exception during worker startup or main run: {e}", exc_info=True)
    finally:
        logging.info("Worker node process beginning final shutdown sequence...")
        if worker:
            worker.stop_status_reporting()
            if worker.channel:
                worker.channel.close()
                logging.info("Closed gRPC channel to Master.")
        logging.info("Worker node shutdown sequence complete.")
        os._exit(0)