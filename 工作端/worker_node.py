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
import webbrowser

# 配置乾淨的日誌輸出
logging.basicConfig(
    level=logging.INFO, 
    format='%(asctime)s - %(message)s',
    datefmt='%H:%M:%S'
)

# 配置
NODE_PORT = int(os.environ.get("NODE_PORT", 50053))
FLASK_PORT = int(os.environ.get("FLASK_PORT", 5000))
MASTER_ADDRESS = os.environ.get("MASTER_ADDRESS", "192.168.2.52:50051")
NODE_ID = os.environ.get("NODE_ID", f"worker-{platform.node().split('.')[0]}-{NODE_PORT}")

class WorkerNode:
    def __init__(self):
        self.node_id = NODE_ID
        self.port = NODE_PORT
        self.master_address = MASTER_ADDRESS
        self.flask_port = FLASK_PORT
        
        # 狀態管理
        self.status = "正在初始化..."
        self.current_task_id = None
        self.username = None
        self.token = None
        self.is_registered = False
        self.login_time = None
        self.cpt_balance = 0
        
        # VPN 相關
        self.vpn_connected = False
        self.vpn_config_path = None
        
        # 線程控制
        self.status_thread = None
        self._stop_event = threading.Event()
        self.logs = []
        self.log_lock = threading.Lock()
        
        # 硬體信息
        self._init_hardware()
        
        # 顯示啟動信息
        self._show_startup_banner()
        
        # 檢查 Docker
        self._wait_for_docker()
        
        # gRPC 連接
        self._init_grpc()
        
        # 請求 VPN 配置
        self._request_vpn_config()
        
        # Flask 應用
        self._init_flask()
        
        self.status = "等待登入"

        # 用戶會話管理
        self.user_sessions = {}
        self.session_lock = threading.Lock()
        self._stop_current_task = False

    def _show_startup_banner(self):
        """顯示啟動橫幅"""
        print("\n" + "="*60)
        print("🔥 HiveMind 工作節點啟動中...")
        print("="*60)
        print(f"節點ID: {self.node_id}")
        print(f"本機IP: {self.local_ip}")
        print(f"gRPC端口: {self.port}")
        print(f"Web端口: {self.flask_port}")
        print(f"目標節點池: {self.master_address}")
        print("="*60)

    def _wait_for_docker(self):
        """等待 Docker 服務啟動"""
        print("🐳 檢查 Docker 服務狀態...")
        
        max_wait = 60  # 最多等待60秒
        wait_time = 0
        
        while wait_time < max_wait:
            try:
                self.docker_client = docker.from_env(timeout=5)
                self.docker_client.ping()
                self.docker_available = True
                print("✅ Docker 服務已就緒")
                
                # 檢查鏡像
                try:
                    self.docker_client.images.get("hivemind-worker:latest")
                    print("✅ 工作節點鏡像已準備")
                except docker.errors.ImageNotFound:
                    print("⚠️  工作節點鏡像未找到，將使用默認配置")
                
                return True
                
            except Exception as e:
                self.docker_available = False
                if wait_time == 0:
                    print("⏳ Docker 服務未啟動，等待中...")
                    print("   請確保 Docker Desktop 已啟動")
                
                # 每10秒顯示一次等待狀態
                if wait_time > 0 and wait_time % 10 == 0:
                    print(f"   已等待 {wait_time}秒...")
                
                time.sleep(2)
                wait_time += 2
        
        print("❌ Docker 服務啟動超時，將在無 Docker 模式下運行")
        self.docker_available = False
        return False

    def _request_vpn_config(self):
        """請求 VPN 配置並嘗試連接"""
        print("\n🔐 VPN 配置階段...")
        
        if not hasattr(self, 'user_stub'):
            print("⚠️  gRPC 連接未建立，跳過 VPN 配置")
            return False
        
        try:
            print("🔄 嘗試獲取 VPN 配置...")
            
            # 嘗試不同的認證方式獲取 VPN 配置
            try:
                # 如果有現有 token，嘗試使用
                if hasattr(self, 'token') and self.token:
                    response = self.user_stub.GetVPNConfig(
                        nodepool_pb2.GetVPNConfigRequest(token=self.token),
                        timeout=10
                    )
                else:
                    # 嘗試匿名獲取（如果節點池支援）
                    response = self.user_stub.GetVPNConfig(
                        nodepool_pb2.GetVPNConfigRequest(token="anonymous"),
                        timeout=10
                    )
                
                if response.success and response.config:
                    print("✅ VPN 配置獲取成功")
                    return self._setup_vpn_connection(response.config, response.client_name)
                else:
                    print(f"⚠️  VPN 配置獲取失敗: {response.message}")
                    
            except grpc.RpcError as e:
                if e.code() == grpc.StatusCode.UNAUTHENTICATED:
                    print("ℹ️  需要登入後才能獲取 VPN 配置")
                else:
                    print(f"⚠️  VPN 配置請求失敗: {e.details()}")
                    
        except Exception as e:
            print(f"⚠️  VPN 配置過程出錯: {e}")
        
        print("ℹ️  將在無 VPN 模式下運行，部分功能可能受限")
        return False

    def _setup_vpn_connection(self, vpn_config, client_name):
        """設置 VPN 連接"""
        try:
            # 確保配置目錄存在
            config_dir = os.path.join(os.path.dirname(__file__), "vpn_configs")
            os.makedirs(config_dir, exist_ok=True)
            
            # 保存 VPN 配置文件
            config_filename = f"{client_name}.conf"
            self.vpn_config_path = os.path.join(config_dir, config_filename)
            
            with open(self.vpn_config_path, 'w', encoding='utf-8') as f:
                f.write(vpn_config)
            
            print(f"💾 VPN 配置已保存: {config_filename}")
            
            # 嘗試自動連接 VPN
            if self._connect_vpn():
                print("✅ VPN 連接成功")
                self.vpn_connected = True
                return True
            else:
                print("⚠️  VPN 自動連接失敗")
                print(f"📁 配置文件位置: {self.vpn_config_path}")
                print("ℹ️  請手動導入 WireGuard 配置並連接")
                return False
                
        except Exception as e:
            print(f"❌ VPN 設置失敗: {e}")
            return False

    def _connect_vpn(self):
        """嘗試自動連接 VPN"""
        try:
            if platform.system() == "Windows":
                return self._connect_vpn_windows()
            else:
                return self._connect_vpn_linux()
        except Exception as e:
            print(f"VPN 連接出錯: {e}")
            return False

    def _connect_vpn_windows(self):
        """Windows VPN 連接"""
        try:
            # 檢查 WireGuard 是否安裝
            wg_paths = [
                "C:/Program Files/WireGuard/wireguard.exe",
                "C:/Program Files (x86)/WireGuard/wireguard.exe"
            ]
            
            wg_path = None
            for path in wg_paths:
                if os.path.exists(path):
                    wg_path = path
                    break
            
            if not wg_path:
                print("⚠️  未找到 WireGuard，請手動連接")
                return False
            
            print("🔗 嘗試啟動 WireGuard 連接...")
            
            # 使用 WireGuard CLI 方式（如果支援）
            try:
                result = subprocess.run([
                    wg_path, "/installtunnelservice", self.vpn_config_path
                ], capture_output=True, text=True, timeout=15)
                
                if result.returncode == 0:
                    print("✅ VPN 隧道服務已安裝")
                    time.sleep(2)  # 等待連接建立
                    return True
                    
            except Exception:
                pass
            
            # 如果命令行方式失敗，嘗試打開 GUI
            print("🖥️  啟動 WireGuard GUI...")
            subprocess.Popen([wg_path], shell=False)
            print("ℹ️  請在 WireGuard GUI 中手動導入配置文件並連接")
            
            return False
            
        except Exception as e:
            print(f"Windows VPN 連接失敗: {e}")
            return False

    def _connect_vpn_linux(self):
        """Linux VPN 連接"""
        try:
            # 檢查 wg-quick 是否可用
            result = subprocess.run(['which', 'wg-quick'], 
                                  capture_output=True, text=True)
            
            if result.returncode != 0:
                print("⚠️  未找到 wg-quick，請手動安裝 WireGuard")
                return False
            
            print("🔗 嘗試啟動 WireGuard 連接...")
            
            # 使用 wg-quick 連接
            result = subprocess.run([
                'sudo', 'wg-quick', 'up', self.vpn_config_path
            ], capture_output=True, text=True, timeout=15)
            
            if result.returncode == 0:
                print("✅ VPN 連接成功")
                return True
            else:
                print(f"⚠️  VPN 連接失敗: {result.stderr}")
                return False
                
        except Exception as e:
            print(f"Linux VPN 連接失敗: {e}")
            return False

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
            
        except Exception as e:
            print(f"❌ 硬體檢測失敗: {e}")
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

    def _init_grpc(self):
        """初始化 gRPC 連接"""
        print("\n🌐 連接到節點池...")
        try:
            self.channel = grpc.insecure_channel(self.master_address)
            grpc.channel_ready_future(self.channel).result(timeout=10)
            
            self.user_stub = nodepool_pb2_grpc.UserServiceStub(self.channel)
            self.node_stub = nodepool_pb2_grpc.NodeManagerServiceStub(self.channel)
            self.master_stub = nodepool_pb2_grpc.MasterNodeServiceStub(self.channel)
            
            print(f"✅ 已連接到節點池: {self.master_address}")
        except Exception as e:
            print(f"❌ 節點池連接失敗: {e}")
            print("⚠️  請檢查節點池地址和網路連接")
            sys.exit(1)

    def _init_flask(self):
        """初始化 Flask 應用"""
        self.app = Flask(__name__, template_folder="templates", static_folder="static")
        self.app.secret_key = secrets.token_hex(32)
        
        self.app.config.update(
            SESSION_COOKIE_NAME='worker_session',
            SESSION_COOKIE_SECURE=False,
            SESSION_COOKIE_HTTPONLY=True,
            SESSION_COOKIE_SAMESITE='Lax',
            SESSION_COOKIE_PATH='/',
            SESSION_COOKIE_DOMAIN=None,
            PERMANENT_SESSION_LIFETIME=datetime.timedelta(hours=24),
            SESSION_REFRESH_EACH_REQUEST=True
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
            user_data = self._get_user_session(session_id) if session_id else None
            
            # 修復：如果沒有有效會話但有登錄用戶，允許訪問
            if not user_data and self.username:
                # 創建臨時會話數據用於 API 響應
                user_data = {
                    'username': self.username,
                    'cpt_balance': self.cpt_balance,
                    'login_time': self.login_time or datetime.datetime.now()
                }
            
            if not user_data:
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
                'login_time': user_data['login_time'].isoformat() if isinstance(user_data['login_time'], datetime.datetime) else str(user_data['login_time']),
                'ip': getattr(self, 'local_ip', '127.0.0.1')
            })

        @self.app.route('/api/logs')
        def api_logs():
            session_id = session.get('session_id')
            user_data = self._get_user_session(session_id) if session_id else None
            
            # 修復：如果沒有有效會話但有登錄用戶，允許訪問
            if not user_data and self.username:
                user_data = {'username': self.username}
            
            if not user_data:
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
                print(f"❌ Flask 啟動失敗: {e}")
                os._exit(1)
        
        threading.Thread(target=run_flask, daemon=True).start()
        
        def show_ready_message():
            time.sleep(2)
            print("\n" + "="*60)
            print("🚀 工作節點已就緒！")
            print("="*60)
            print(f"🌐 Web 管理介面: http://127.0.0.1:{self.flask_port}")
            print(f"🔧 節點狀態: {self.status}")
            print(f"🐳 Docker: {'✅ 可用' if self.docker_available else '❌ 不可用'}")
            print(f"🔐 VPN: {'✅ 已連接' if self.vpn_connected else '⚠️  未連接'}")
            print("="*60)
            print("ℹ️  請在 Web 介面中登入以開始接收任務")
            
            url = f"http://127.0.0.1:{self.flask_port}"
            try:
                webbrowser.open(url)
            except Exception:
                pass
        
        threading.Thread(target=show_ready_message, daemon=True).start()

    def _log(self, message, level=logging.INFO):
        """記錄日誌"""
        # 只記錄到內部日誌，不輸出到控制台（保持控制台乾淨）
        with self.log_lock:
            timestamp = datetime.datetime.now().strftime('%H:%M:%S')
            level_name = logging.getLevelName(level)
            self.logs.append(f"{timestamp} - {level_name} - {message}")
            if len(self.logs) > 500:
                self.logs.pop(0)

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
                hostname=self.local_ip,  # 使用本機 IP 而不是 127.0.0.1
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
                self._log(f"節點註冊成功，使用 IP: {self.local_ip}:{self.port}")
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
        self._stop_current_task = False  # 重置停止標誌
        
        temp_dir = None
        container = None
        success = False
        task_logs = []
        stop_requested = False
        
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

            # 執行容器
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
            
            self._log(f"Task {task_id} container started, monitoring...")
            
            # 發送初始日誌
            initial_log = f"任務 {task_id} 開始執行\n容器名稱: {container_name}\n工作目錄: {workspace}"
            task_logs.append(initial_log)
            self._send_task_logs(task_id, initial_log)
            
            # 簡化的監控邏輯 - 定期檢查停止請求和容器狀態
            log_buffer = []
            log_send_counter = 0
            last_log_fetch = time.time()
            
            while True:
                # 優先檢查停止請求
                if self._stop_current_task:
                    stop_requested = True
                    stop_log = f"收到停止請求，立即終止任務 {task_id}"
                    self._log(stop_log)
                    task_logs.append(stop_log)
                    self._send_task_logs(task_id, stop_log)
                    
                    # 立即強制停止容器
                    try:
                        container.kill()
                        stop_log = f"容器已強制停止: {container_name}"
                        self._log(stop_log)
                        task_logs.append(stop_log)
                        self._send_task_logs(task_id, stop_log)
                    except Exception as e:
                        error_log = f"強制停止容器失敗: {str(e)}"
                        self._log(error_log, logging.WARNING)
                        task_logs.append(error_log)
                        self._send_task_logs(task_id, error_log)
                    
                    break
                
                # 檢查容器狀態
                try:
                    container.reload()
                    if container.status != 'running':
                        self._log(f"Container {container_name} stopped naturally, status: {container.status}")
                        break
                except Exception as e:
                    self._log(f"Failed to check container status: {e}")
                    break
                
                # 每隔1秒嘗試收集一次日誌（非阻塞）
                current_time = time.time()
                if current_time - last_log_fetch > 1.0:
                    try:
                        # 獲取最新的日誌（非阻塞，只獲取新的日誌）
                        logs = container.logs(since=int(last_log_fetch)).decode('utf-8', errors='replace')
                        if logs.strip():
                            log_lines = logs.strip().split('\n')
                            for line in log_lines:
                                if line.strip():
                                    self._log(f"[Task {task_id}]: {line}")
                                    log_buffer.append(line)
                                    task_logs.append(line)  # 保存到任務日誌列表
                                    log_send_counter += 1
                            
                            # 每20行或每3秒發送一次日誌
                            if log_send_counter >= 20 or len(log_buffer) > 0:
                                logs_to_send = "\n".join(log_buffer)
                                self._send_task_logs(task_id, logs_to_send)
                                log_buffer.clear()
                                log_send_counter = 0
                        
                        last_log_fetch = current_time
                    except Exception as e:
                        self._log(f"Error collecting logs: {e}", logging.WARNING)
                
                # 短暫休眠，確保能快速響應停止請求
                time.sleep(0.1)
            
            # 發送剩餘的日誌
            if log_buffer:
                logs_to_send = "\n".join(log_buffer)
                self._send_task_logs(task_id, logs_to_send)
                # 也加入到任務日誌列表
                task_logs.extend(log_buffer)
            
            # 處理任務完成或停止
            if stop_requested:
                success = False
                completion_log = f"任務 {task_id} 被用戶強制停止"
            else:
                # 任務自然結束，檢查退出碼
                try:
                    result = container.wait(timeout=2)
                    success = result.get('StatusCode', -1) == 0
                    completion_log = f"任務 {task_id} 執行完成，退出碼: {result.get('StatusCode', -1)}"
                except Exception as e:
                    success = False
                    completion_log = f"任務 {task_id} 完成狀態檢查失敗: {str(e)}"
            
            self._log(completion_log)
            task_logs.append(completion_log)
            self._send_task_logs(task_id, completion_log)
            
            # 立即打包結果，包含任務日誌
            result_zip = self._create_result_zip(task_id, workspace, success, stop_requested, task_logs)
            
            # 發送結果到節點池
            if result_zip:
                try:
                    self.master_stub.ReturnTaskResult(
                        nodepool_pb2.ReturnTaskResultRequest(
                            task_id=task_id,
                            result_zip=result_zip
                        ),
                        metadata=[('authorization', f'Bearer {self.token}')],
                        timeout=30
                    )
                    
                    result_log = f"任務 {task_id} 結果已發送到節點池（狀態: {'強制停止' if stop_requested else '完成'}）"
                    self._log(result_log)
                    task_logs.append(result_log)
                    self._send_task_logs(task_id, result_log)
                except Exception as e:
                    error_log = f"發送任務結果失敗: {str(e)}"
                    self._log(error_log, logging.ERROR)
                    task_logs.append(error_log)
                    self._send_task_logs(task_id, error_log)
            
        except Exception as e:
            error_log = f"任務 {task_id} 執行失敗: {str(e)}"
            self._log(error_log, logging.ERROR)
            task_logs.append(error_log)
            self._send_task_logs(task_id, error_log)
            success = False
        
        finally:
            # 清理容器（強制清理）
            if container:
                try:
                    try:
                        container.kill()  # 確保強制停止
                    except:
                        pass
                    container.remove(force=True)
                    cleanup_log = f"任務 {task_id} 容器已強制清理"
                    self._log(cleanup_log)
                    task_logs.append(cleanup_log)
                    self._send_task_logs(task_id, cleanup_log)
                except Exception as e:
                    cleanup_error = f"清理容器失敗: {str(e)}"
                    self._log(cleanup_error, logging.WARNING)
                    task_logs.append(cleanup_error)
                    self._send_task_logs(task_id, cleanup_error)
            
            # 清理臨時目錄
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
                        success=success and not stop_requested
                    ),
                    metadata=[('authorization', f'Bearer {self.token}')],
                    timeout=10
                )
            except Exception as e:
                self._log(f"Failed to notify task completion: {e}", logging.WARNING)
            
            # 重置狀態
            self.current_task_id = None
            self.status = "Idle"
            self._stop_current_task = False
            status_msg = "強制停止並已打包結果" if stop_requested else "執行完成"
            self._log(f"Task {task_id} cleanup completed, status reset to Idle ({status_msg})")

    def _create_result_zip(self, task_id, workspace, success, stopped=False, task_logs=None):
        """創建結果 ZIP，包含停止狀態信息和任務日誌"""
        try:
            # 創建執行日誌，包含停止信息
            log_file = os.path.join(workspace, "execution_log.txt")
            with open(log_file, 'w', encoding='utf-8') as f:
                f.write(f"Task ID: {task_id}\n")
                if stopped:
                    f.write(f"Status: Stopped by user\n")
                    f.write(f"Execution Result: Terminated\n")
                else:
                    f.write(f"Status: {'Success' if success else 'Failed'}\n")
                    f.write(f"Execution Result: {'Completed' if success else 'Error'}\n")
                f.write(f"Time: {datetime.datetime.now()}\n")
                f.write(f"Node: {self.node_id}\n")
                
                if stopped:
                    f.write(f"\nNote: This task was stopped by user request.\n")
                    f.write(f"Any partial results or intermediate files are included in this package.\n")
            
            # 創建任務完整日誌文件
            if task_logs:
                task_log_file = os.path.join(workspace, "task_logs.txt")
                with open(task_log_file, 'w', encoding='utf-8') as f:
                    f.write(f"=== Task {task_id} Complete Logs ===\n")
                    f.write(f"Generated at: {datetime.datetime.now()}\n")
                    f.write(f"Status: {'Stopped by user' if stopped else ('Success' if success else 'Failed')}\n")
                    f.write(f"Node: {self.node_id}\n\n")
                    
                    for log_entry in task_logs:
                        f.write(f"{log_entry}\n")
                    
                    f.write(f"\n=== End of Logs ===\n")
            
            # 創建停止狀態文件（如果任務被停止）
            if stopped:
                stop_file = os.path.join(workspace, "task_stopped.txt")
                with open(stop_file, 'w', encoding='utf-8') as f:
                    f.write(f"Task {task_id} was stopped by user request at {datetime.datetime.now()}\n")
                    f.write(f"This file indicates that the task did not complete normally.\n")
                    f.write(f"Check execution_log.txt and task_logs.txt for more details.\n")
            
            # 打包整個工作目錄
            zip_buffer = io.BytesIO()
            with zipfile.ZipFile(zip_buffer, 'w', zipfile.ZIP_DEFLATED) as zip_file:
                for root, dirs, files in os.walk(workspace):
                    for file in files:
                        file_path = os.path.join(root, file)
                        arcname = os.path.relpath(file_path, workspace)
                        zip_file.write(file_path, arcname)
            
            result_size = len(zip_buffer.getvalue())
            self._log(f"Created result zip for task {task_id}: {result_size} bytes ({'stopped' if stopped else 'completed'}), logs included")
            return zip_buffer.getvalue()
            
        except Exception as e:
            self._log(f"Failed to create result zip: {e}", logging.ERROR)
            
            # 如果打包失敗，創建一個包含錯誤信息的簡單ZIP
            try:
                zip_buffer = io.BytesIO()
                with zipfile.ZipFile(zip_buffer, 'w', zipfile.ZIP_DEFLATED) as zip_file:
                    error_content = f"Task {task_id} packaging failed: {str(e)}\n"
                    error_content += f"Status: {'Stopped' if stopped else 'Failed'}\n"
                    error_content += f"Time: {datetime.datetime.now()}\n"
                    
                    # 嘗試包含部分日誌
                    if task_logs:
                        error_content += f"\n=== Partial Logs ===\n"
                        for log_entry in task_logs[-50:]:  # 最後50行日誌
                            error_content += f"{log_entry}\n"
                    
                    zip_file.writestr("error_log.txt", error_content)
                return zip_buffer.getvalue()
            except:
                return None

# gRPC 服務實現
class WorkerNodeServicer(nodepool_pb2_grpc.WorkerNodeServiceServicer):
    def __init__(self, worker_node):
        self.worker_node = worker_node

    def ExecuteTask(self, request, context):
        """執行任務 RPC - 改善大檔案處理和錯誤處理"""
        task_id = request.task_id
        task_zip = request.task_zip
        
        file_size_mb = len(task_zip) / (1024 * 1024)
        logging.info(f"===== 收到執行任務請求 =====")
        logging.info(f"任務ID: {task_id}")
        logging.info(f"檔案大小: {file_size_mb:.1f}MB")
        logging.info(f"當前節點狀態: {self.worker_node.status}")
        logging.info(f"是否已註冊: {self.worker_node.is_registered}")
        logging.info(f"Docker 可用: {self.worker_node.docker_available}")
        
        # 快速檢查節點狀態
        if self.worker_node.current_task_id:
            error_msg = f"節點忙碌中，拒絕任務 {task_id} (當前任務: {self.worker_node.current_task_id})"
            logging.warning(error_msg)
            return nodepool_pb2.ExecuteTaskResponse(
                success=False, 
                message=error_msg
            )
        
        if not self.worker_node.docker_available:
            error_msg = f"Docker 不可用，拒絕任務 {task_id}"
            logging.error(error_msg)
            return nodepool_pb2.ExecuteTaskResponse(
                success=False, 
                message="Docker not available"
            )
        
        # 檢查任務數據完整性和大小
        if not task_zip:
            error_msg = f"任務 {task_id} 數據為空"
            logging.error(error_msg)
            return nodepool_pb2.ExecuteTaskResponse(
                success=False, 
                message="Task data is empty"
            )
        
        # 檢查檔案大小限制（100MB）
        if file_size_mb > 100:
            error_msg = f"任務 {task_id} 檔案太大: {file_size_mb:.1f}MB，超過100MB限制"
            logging.error(error_msg)
            return nodepool_pb2.ExecuteTaskResponse(
                success=False, 
                message=f"Task file too large: {file_size_mb:.1f}MB (limit: 100MB)"
            )
        
        try:
            # 立即響應接受任務，避免超時
            self.worker_node.current_task_id = task_id
            self.worker_node.status = f"Receiving: {task_id} ({file_size_mb:.1f}MB)"
            
            logging.info(f"開始接受任務 {task_id}，狀態更新為: {self.worker_node.status}")
            
            # 預先驗證 ZIP 檔案完整性
            try:
                import zipfile
                import io
                with zipfile.ZipFile(io.BytesIO(task_zip), 'r') as zip_ref:
                    zip_ref.testzip()  # 驗證 ZIP 檔案完整性
                logging.info(f"任務 {task_id} ZIP 檔案驗證成功")
            except Exception as zip_error:
                self.worker_node.current_task_id = None
                self.worker_node.status = "Idle"
                error_msg = f"任務 {task_id} ZIP 檔案損壞: {zip_error}"
                logging.error(error_msg)
                return nodepool_pb2.ExecuteTaskResponse(
                    success=False, 
                    message=f"Invalid ZIP file: {str(zip_error)}"
                )
            
            # 更新狀態為準備執行
            self.worker_node.status = f"Preparing: {task_id}"
            logging.info(f"任務 {task_id} 檔案驗證完成，開始準備執行")
            
            # 啟動執行線程
            execution_thread = threading.Thread(
                target=self.worker_node._execute_task,
                args=(task_id, task_zip),
                daemon=True,
                name=f"TaskExecution-{task_id}"
            )
            execution_thread.start()
            
            success_msg = f"任務 {task_id} 已接受並開始準備執行 (檔案大小: {file_size_mb:.1f}MB)"
            logging.info(success_msg)
            logging.info(f"===== 任務接受完成 =====")
            
            return nodepool_pb2.ExecuteTaskResponse(
                success=True, 
                message=f"Task {task_id} accepted, file size: {file_size_mb:.1f}MB"
            )
            
        except Exception as e:
            # 如果出錯，重置狀態
            self.worker_node.current_task_id = None
            self.worker_node.status = "Idle"
            error_msg = f"接受任務 {task_id} 時發生錯誤: {e}"
            logging.error(error_msg, exc_info=True)
            return nodepool_pb2.ExecuteTaskResponse(
                success=False, 
                message=f"Failed to accept task: {str(e)}"
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
        """立即強制停止任務執行並打包結果"""
        task_id = request.task_id
        if self.worker_node.current_task_id == task_id:
            logging.info(f"收到停止任務 {task_id} 的請求，立即執行強制停止")
            
            # 立即設置停止標誌
            self.worker_node._stop_current_task = True
            
            # 等待任務處理停止請求，但時間較短
            max_wait_time = 10  # 減少到10秒
            wait_count = 0
            while self.worker_node.current_task_id == task_id and wait_count < max_wait_time:
                time.sleep(0.5)  # 更頻繁檢查
                wait_count += 0.5
                
                # 每2秒報告一次進度
                if int(wait_count) % 2 == 0 and wait_count > 0:
                    logging.info(f"強制停止任務 {task_id} 處理中... ({wait_count:.1f}/{max_wait_time}秒)")
            
            if wait_count >= max_wait_time:
                # 如果超時，直接重置狀態
                self.worker_node.current_task_id = None
                self.worker_node.status = "Idle"
                self.worker_node._stop_current_task = False
                logging.warning(f"任務 {task_id} 超時強制停止（{max_wait_time} 秒）")
                return nodepool_pb2.StopTaskExecutionResponse(
                    success=True,
                    message=f"Task {task_id} force stopped (timeout after {max_wait_time}s)"
                )
            else:
                logging.info(f"任務 {task_id} 已成功停止並打包結果（耗時 {wait_count:.1f} 秒）")
                return nodepool_pb2.StopTaskExecutionResponse(
                    success=True,
                    message=f"Task {task_id} successfully stopped and results packaged (took {wait_count:.1f}s)"
                )
        else:
            return nodepool_pb2.StopTaskExecutionResponse(
                success=False,
                message=f"Task {task_id} not running"
            )

if __name__ == "__main__":
    try:
        print("🔥 啟動 HiveMind 工作節點...")
        
        # 創建工作節點
        worker = WorkerNode()
        
        # 啟動 gRPC 服務
        print("\n⚙️  啟動 gRPC 服務...")
        server = grpc.server(futures.ThreadPoolExecutor(max_workers=5))
        nodepool_pb2_grpc.add_WorkerNodeServiceServicer_to_server(
            WorkerNodeServicer(worker), server
        )
        
        server.add_insecure_port(f'[::]:{NODE_PORT}')
        server.start()
        
        print(f"✅ gRPC 服務已啟動在端口 {NODE_PORT}")
        
        # 保持運行
        try:
            print("\n⏳ 工作節點運行中... (按 Ctrl+C 停止)")
            while True:
                time.sleep(60)
        except KeyboardInterrupt:
            print("\n🛑 收到停止信號...")
            worker._stop_status_reporting()
            server.stop(grace=5)
            print("✅ 工作節點已停止")
            
    except Exception as e:
        print(f"❌ 工作節點啟動失敗: {e}")
        input("按 Enter 鍵退出...")
        sys.exit(1)