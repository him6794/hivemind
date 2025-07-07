from docker import from_env
from docker.errors import ImageNotFound, APIError
import grpc
from threading import Thread, Event, Lock
from logging import basicConfig, info, warning, error, critical, INFO, WARNING, ERROR, getLevelName
from flask import Flask, jsonify, request, render_template, session, redirect, url_for
import nodepool_pb2
import nodepool_pb2_grpc
from concurrent.futures import ThreadPoolExecutor
from os import environ, makedirs, chmod, walk, _exit, system
from os.path import join, dirname, abspath, exists, relpath
from psutil import cpu_count, virtual_memory, cpu_percent
from time import time, sleep
from zipfile import ZipFile, ZIP_DEFLATED
from io import BytesIO
from sys import exit
from tempfile import mkdtemp
from subprocess import run, CREATE_NO_WINDOW
from platform import node, system
from datetime import datetime, timedelta
from secrets import token_hex
from shutil import copy2, rmtree
from socket import socket, AF_INET, SOCK_DGRAM
from uuid import uuid4
from webbrowser import open as web_open
from netifaces import interfaces, ifaddresses, AF_INET
from requests import post, get, exceptions
basicConfig(level=INFO, format='%(asctime)s - %(levelname)s - [%(threadName)s] - %(message)s')

NODE_PORT = int(environ.get("NODE_PORT", 50053))
FLASK_PORT = int(environ.get("FLASK_PORT", 5000))
MASTER_ADDRESS = environ.get("MASTER_ADDRESS", "10.0.0.1:50051")
NODE_ID = environ.get("NODE_ID", f"worker-{node().split('.')[0]}-{NODE_PORT}")

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
        self._stop_event = Event()
        self.logs = []
        self.log_lock = Lock()

        # 用戶會話管理
        self.user_sessions = {}
        self.session_lock = Lock()
        self._stop_current_task = False

        # 先自動連線 VPN
        self._auto_join_vpn()

        # 硬體信息
        self._init_hardware()
        # Docker 初始化
        self._init_docker()
        # gRPC 連接
        self._init_grpc()
        # Flask 應用
        self._init_flask()
        self.status = "Waiting for Login"

    def _auto_join_vpn(self):
        """自動請求主控端 /api/vpn/join 取得 WireGuard 配置並連線 VPN"""
        try:
            api_url = "https://hivemind.justin0711.com/api/vpn/join"
            client_name = self.node_id
            resp = post(api_url, json={"client_name": client_name}, timeout=15, verify=True)
            try:
                resp_json = resp.json()
            except Exception:
                resp_json = {}
            if resp.status_code == 200 and resp_json.get("success"):
                config_content = resp_json.get("config")
                config_path = join(dirname(abspath(__file__)), "wg0.conf")
                try:
                    with open(config_path, "w") as f:
                        f.write(config_content)
                    self._log(f"自動取得 WireGuard 配置並寫入 {config_path}")
                except Exception as e:
                    self._log(f"寫入 WireGuard 配置失敗: {e}", WARNING)
                    return
                # Windows/Linux 都在當前目錄執行 wg-quick，需有權限與路徑
                from os import system
                result = system(f"wg-quick down {config_path} 2>/dev/null; wg-quick up {config_path}")
                if result == 0:
                    self._log("WireGuard VPN 啟動成功")
                else:
                    self._log("WireGuard VPN 啟動失敗，請檢查權限與配置", WARNING)
                    self._prompt_manual_vpn(config_path)
            else:
                error_msg = resp_json.get("error") if resp_json else resp.text
                self._log(f"自動取得 WireGuard 配置失敗: {error_msg}", WARNING)
                if error_msg and "VPN 服務不可用" in error_msg:
                    self._log("請確認主控端 Flask 啟動時有正確初始化 WireGuardServer，且 /api/vpn/join 可用", WARNING)
                self._prompt_manual_vpn()
        except Exception as e:
            self._log(f"自動請求 /api/vpn/join 失敗: {e}", WARNING)
            self._prompt_manual_vpn()

    def _prompt_manual_vpn(self, config_path=None):
        """提示用戶手動連線 WireGuard"""
        msg = (
            "\n[提示] 自動連線 WireGuard 失敗，請手動連線 VPN：\n"
            "1. 請找到您的設定檔(wg0.conf)。\n"
            "2. 手動打開wireguard客戶端導入配置\n"
            "3. 如遇權限問題請用管理員/Root 權限執行。\n"
        )
        print(msg)
        print('如果您已經連線好請按y')
        a = input()
        if a == 'y':
            self._log("用戶已確認手動連線 WireGuard")

    def _init_hardware(self):
        """初始化硬體信息"""
        try:
            self.hostname = node()
            self.cpu_cores = cpu_count(logical=True)
            
            # 使用可用記憶體而不是總記憶體
            memory_info = virtual_memory()
            self.memory_gb = round(memory_info.available / (1024**3), 2)
            self.total_memory_gb = round(memory_info.total / (1024**3), 2)
            
            # 自動檢測地區，不進行用戶交互
            self.location = self._auto_detect_location() or "Unknown"
            
            # 獲取本機 IP
            self.local_ip = self._get_local_ip()
            
            # 簡化的效能計算
            self.cpu_score = self._benchmark_cpu()
            self.gpu_score, self.gpu_name, self.gpu_memory_gb = self._detect_gpu()
            
            self._log(f"Hardware: CPU={self.cpu_cores} cores, RAM={self.memory_gb:.1f}GB available (Total: {self.total_memory_gb:.1f}GB)")
            self._log(f"Performance: CPU={self.cpu_score}, GPU={self.gpu_score}")
            self._log(f"Location: {self.location}")
            self._log(f"Local IP: {self.local_ip}")
        except Exception as e:
            self._log(f"Hardware detection failed: {e}", ERROR)
            # 設置預設值
            self.hostname = "unknown"
            self.cpu_cores = 1
            self.memory_gb = 1.0
            self.total_memory_gb = 1.0
            self.location = "Unknown"
            self.local_ip = "127.0.0.1"
            self.cpu_score = 0
            self.gpu_score = 0
            self.gpu_name = "Not Detected"
            self.gpu_memory_gb = 0.0

    def _auto_detect_location(self):
        """靜默自動檢測地區"""
        try:
            # 使用多個 API 嘗試檢測
            apis = [
                'http://ip-api.com/json/',
                'https://ipapi.co/json/',
                'http://www.geoplugin.net/json.gp'
            ]
            
            for api_url in apis:
                try:
                    response = get(api_url, timeout=5)
                    
                    if response.status_code == 200:
                        data = response.json()
                        
                        # 根據不同 API 的響應格式處理
                        continent = None
                        country = None
                        
                        if 'continent' in data:
                            continent = data.get('continent', '')
                            country = data.get('country', '')
                        elif 'continent_code' in data:
                            continent_codes = {
                                'AS': 'Asia', 'AF': 'Africa', 'NA': 'North America',
                                'SA': 'South America', 'EU': 'Europe', 'OC': 'Oceania'
                            }
                            continent = continent_codes.get(data.get('continent_code', ''))
                            country = data.get('country_name', '')
                        elif 'geoplugin_continentName' in data:
                            continent = data.get('geoplugin_continentName', '')
                            country = data.get('geoplugin_countryName', '')
                        
                        if continent and country:
                            continent_mapping = {
                                'Asia': '亞洲', 'Africa': '非洲', 'North America': '北美',
                                'South America': '南美', 'Europe': '歐洲', 'Oceania': '大洋洲'
                            }
                            
                            detected_region = continent_mapping.get(continent)
                            if detected_region:
                                self._log(f"自動檢測地區: {country} -> {detected_region}")
                                return detected_region
                        
                except (exceptions.RequestException, Exception):
                    continue
            
            self._log("地區檢測失敗，使用 Unknown")
            return "Unknown"
                    
        except Exception as e:
            self._log(f"地區檢測錯誤: {e}")
            return "Unknown"

    def _get_local_ip(self):
        """獲取本機 IP 地址（優先使用 WireGuard 網卡）"""
        try:
            # 檢查所有網卡接口
            interfaces_list = interfaces()
            self._log(f"檢測到網卡接口: {interfaces_list}")
            
            # 優先檢查 WireGuard 相關接口
            wg_interfaces = [iface for iface in interfaces_list if 'wg' in iface.lower() or 'wireguard' in iface.lower()]
            
            if wg_interfaces:
                for wg_iface in wg_interfaces:
                    try:
                        addrs = ifaddresses(wg_iface)
                        if AF_INET in addrs:
                            wg_ip = addrs[AF_INET][0]['addr']
                            self._log(f"檢測到 WireGuard 網卡 {wg_iface}，IP: {wg_ip}")
                            return wg_ip
                    except Exception as e:
                        self._log(f"檢查 {wg_iface} 接口失敗: {e}")
                        continue
            
            # 檢查是否有 10.0.0.x 網段的 IP（VPN 網段）
            for iface in interfaces_list:
                try:
                    addrs = ifaddresses(iface)
                    if AF_INET in addrs:
                        for addr_info in addrs[AF_INET]:
                            ip = addr_info['addr']
                            # 檢查是否在 VPN 網段
                            if ip.startswith('10.0.0.') and ip != '10.0.0.1':
                                self._log(f"檢測到 VPN 網段 IP: {ip} (接口: {iface})")
                                return ip
                except Exception as e:
                    continue
            
            # 如果沒有找到 VPN IP，使用預設方法
            self._log("未檢測到 WireGuard 網卡，使用預設網卡")
            
        except Exception as e:
            self._log(f"網卡檢測失敗: {e}")
        
        # 預設方法：連接外部服務獲取本機 IP
        try:
            s = socket(AF_INET, SOCK_DGRAM)
            s.connect(("8.8.8.8", 80))
            ip = s.getsockname()[0]
            s.close()
            self._log(f"使用預設方法獲取 IP: {ip}")
            return ip
        except:
            self._log("所有方法都失敗，使用 127.0.0.1")
            return "127.0.0.1"

    def update_location(self, new_location):
        """更新節點地區設定"""
        available_locations = ["亞洲", "非洲", "北美", "南美", "歐洲", "大洋洲", "Unknown"]
        
        if new_location in available_locations:
            old_location = self.location
            self.location = new_location
            self._log(f"地區已更新: {old_location} -> {new_location}")
            
            # 如果已註冊，需要重新註冊以更新地區信息
            if self.is_registered and self.token:
                self._register()
            
            return True, f"地區已更新為: {new_location}"
        else:
            return False, f"無效的地區選擇: {new_location}"

    def _benchmark_cpu(self):
        """簡化的 CPU 基準測試"""
        try:
            start_time = time()
            result = 0
            for i in range(10_000_000):
                result = (result + i * i) % 987654321
            duration = time() - start_time
            return int((10_000_000 / duration) / 1000) if duration > 0.01 else 10000
        except:
            return 1000

    def _detect_gpu(self):
        """簡化的 GPU 檢測"""
        try:
            if system() == "Windows":
                cmd = 'wmic path Win32_VideoController get Name, AdapterRAM /VALUE'
                result = run(cmd, capture_output=True, text=True, timeout=10, 
                            creationflags=CREATE_NO_WINDOW)
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
            self.docker_client = from_env(timeout=10)
            self.docker_client.ping()
            self.docker_available = True
            
            # 檢查或拉取鏡像
            try:
                self.docker_client.images.get("justin308/hivemind-worker:latest")
                self._log("Docker image found")
            except ImageNotFound:
                self._log("Docker image not found, pulling justin308/hivemind-worker:latest")
                try:
                    self.docker_client.images.pull("justin308/hivemind-worker:latest")
                    self._log("Docker image pulled successfully")
                except Exception as e:
                    self._log(f"Failed to pull docker image: {e}", WARNING)
                    
        except Exception as e:
            self._log(f"Docker initialization failed: {e}", WARNING)
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
            self._log(f"gRPC connection failed: {e}", ERROR)

    def _init_flask(self):
        """初始化 Flask 應用"""
        self.app = Flask(__name__, template_folder="templates", static_folder="static")
        self.app.secret_key = token_hex(32)
        
        # 配置會話持久性，使用不同的cookie名稱避免與主控端衝突
        self.app.config.update(
            SESSION_COOKIE_NAME='worker_session',  # 與主控端不同的cookie名稱
            SESSION_COOKIE_SECURE=False,  # 如果使用HTTPS則設為True
            SESSION_COOKIE_HTTPONLY=True,
            SESSION_COOKIE_SAMESITE='Lax',
            SESSION_COOKIE_PATH='/',
            SESSION_COOKIE_DOMAIN=None,
            PERMANENT_SESSION_LIFETIME=timedelta(hours=24),  # 24小時會話
            SESSION_REFRESH_EACH_REQUEST=True  # 每次請求刷新會話
        )
        
        self._setup_routes()
        self._start_flask()

    def _create_user_session(self, username, token):
        """創建用戶會話"""
        session_id = str(uuid4())
        session_data = {
            'username': username,
            'token': token,
            'login_time': datetime.now(),
            'cpt_balance': 0,
            'created_at': time()
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
            available_locations = ["亞洲", "非洲", "北美", "南美", "歐洲", "大洋洲", "Unknown"]
            return render_template('login.html', 
                                 node_id=self.node_id, 
                                 current_status=self.status,
                                 current_location=self.location,
                                 available_locations=available_locations)

        @self.app.route('/monitor')
        def monitor():
            session_id = session.get('session_id')
            user_data = self._get_user_session(session_id) if session_id else None
            
            if not user_data:
                return redirect(url_for('index'))
            
            available_locations = ["亞洲", "非洲", "北美", "南美", "歐洲", "大洋洲", "Unknown"]
            return render_template('monitor.html', 
                                 username=user_data['username'],
                                 node_id=self.node_id, 
                                 initial_status=self.status,
                                 current_location=self.location,
                                 available_locations=available_locations)

        @self.app.route('/login', methods=['GET', 'POST'])
        def login_route():
            if request.method == 'GET':
                session_id = session.get('session_id')
                user_data = self._get_user_session(session_id) if session_id else None
                
                if user_data and user_data['username'] == self.username:
                    return redirect(url_for('monitor'))
                
                available_locations = ["亞洲", "非洲", "北美", "南美", "歐洲", "大洋洲", "Unknown"]
                return render_template('login.html', 
                                     node_id=self.node_id, 
                                     current_status=self.status,
                                     current_location=self.location,
                                     available_locations=available_locations)

            # POST 登入
            username = request.form.get('username')
            password = request.form.get('password')
            selected_location = request.form.get('location')
            
            # 更新地區設定
            if selected_location:
                success, message = self.update_location(selected_location)
                if not success:
                    available_locations = ["亞洲", "非洲", "北美", "南美", "歐洲", "大洋洲", "Unknown"]
                    return render_template('login.html', 
                                         error=f"地區設定錯誤: {message}", 
                                         node_id=self.node_id, 
                                         current_status=self.status,
                                         current_location=self.location,
                                         available_locations=available_locations)
            
            if not username or not password:
                available_locations = ["亞洲", "非洲", "北美", "南美", "歐洲", "大洋洲", "Unknown"]
                return render_template('login.html', 
                                     error="請輸入用戶名和密碼", 
                                     node_id=self.node_id, 
                                     current_status=self.status,
                                     current_location=self.location,
                                     available_locations=available_locations)

            if self._login(username, password) and self._register():
                session_id = self._create_user_session(username, self.token)
                session['session_id'] = session_id
                session.permanent = True
                
                self._log(f"User '{username}' logged in successfully, location: {self.location}")
                return redirect(url_for('monitor'))
            else:
                available_locations = ["亞洲", "非洲", "北美", "南美", "歐洲", "大洋洲", "Unknown"]
                return render_template('login.html', 
                                     error=f"登入失敗: {self.status}", 
                                     node_id=self.node_id, 
                                     current_status=self.status,
                                     current_location=self.location,
                                     available_locations=available_locations)

        @self.app.route('/api/update_location', methods=['POST'])
        def api_update_location():
            session_id = session.get('session_id')
            user_data = self._get_user_session(session_id) if session_id else None
            
            if not user_data:
                return jsonify({'success': False, 'error': 'Unauthorized'}), 401
            
            try:
                data = request.get_json()
                new_location = data.get('location')
                
                if not new_location:
                    return jsonify({'success': False, 'error': '請選擇地區'})
                
                success, message = self.update_location(new_location)
                return jsonify({'success': success, 'message': message, 'current_location': self.location})
                
            except Exception as e:
                return jsonify({'success': False, 'error': f'更新失敗: {str(e)}'})

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
                    'login_time': self.login_time or datetime.now()
                }
            
            if not user_data:
                return jsonify({'error': 'Unauthorized'}), 401
            
            try:
                cpu_percent_val = cpu_percent(interval=0.1)
                mem = virtual_memory()
                current_available_gb = round(mem.available / (1024**3), 2)
            except:
                cpu_percent_val, mem = 0, None
                current_available_gb = self.memory_gb

            return jsonify({
                'node_id': self.node_id,
                'status': self.status,
                'current_task_id': self.current_task_id or "None",
                'is_registered': self.is_registered,
                'docker_available': self.docker_available,
                'cpu_percent': round(cpu_percent_val, 1),
                'cpu_cores': self.cpu_cores,
                'memory_percent': round(mem.percent, 1) if mem else 0,
                'memory_used_gb': round(mem.used/(1024**3), 2) if mem else 0,
                'memory_available_gb': current_available_gb,
                'memory_total_gb': getattr(self, 'total_memory_gb', self.memory_gb),
                'cpu_score': self.cpu_score,
                'gpu_score': self.gpu_score,
                'gpu_name': self.gpu_name,
                'gpu_memory_gb': self.gpu_memory_gb,
                'cpt_balance': user_data['cpt_balance'],
                'login_time': user_data['login_time'].isoformat() if isinstance(user_data['login_time'], datetime) else str(user_data['login_time']),
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
                self._log(f"Flask failed to start: {e}", ERROR)
                _exit(1)
        
        # 啟動 Flask 服務
        Thread(target=run_flask, daemon=True).start()
        self._log(f"Flask started on port {self.flask_port}")
        
        # 延遲開啟瀏覽器
        def open_browser():
            sleep(2)  # 等待 Flask 完全啟動
            url = f"http://127.0.0.1:{self.flask_port}"
            try:
                web_open(url)
                self._log(f"瀏覽器已開啟: {url}")
            except Exception as e:
                self._log(f"無法開啟瀏覽器: {e}", WARNING)
                self._log(f"請手動開啟: {url}")
        
        # 在獨立線程中開啟瀏覽器
        Thread(target=open_browser, daemon=True).start()

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
                self.login_time = datetime.now()
                self.status = "Logged In"
                self._log(f"User {username} logged in successfully")
                return True
            else:
                self.status = "Login Failed"
                self._log(f"Login failed for user {username}")
                return False
        except Exception as e:
            self._log(f"Login error: {e}", ERROR)
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
                cpu_cores=int(self.cpu_cores/1000),
                memory_gb=int(self.memory_gb/10),
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
            self._log(f"Registration error: {e}", ERROR)
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
        self.status_thread = Thread(target=self._status_reporter, daemon=True)
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
                    self._log(f"Status report failed: {e}", WARNING)
            
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
            current_timestamp = int(time())  # 使用秒級時間戳而不是毫秒
            
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
            self._log(f"Error sending logs for task {task_id}: {e}", WARNING)
            return False

    def _execute_task(self, task_id, task_zip_bytes):
        """執行任務"""
        self.current_task_id = task_id
        self.status = f"Executing: {task_id}"
        self._stop_current_task = False
        
        temp_dir = None
        container = None
        success = False
        task_logs = []
        stop_requested = False
        
        try:
            if not self.docker_available:
                raise RuntimeError("Docker not available")

            # 創建臨時目錄
            temp_dir = mkdtemp(prefix=f"task_{task_id}_")
            workspace = join(temp_dir, "workspace")
            makedirs(workspace)

            # 解壓任務文件
            with ZipFile(BytesIO(task_zip_bytes), 'r') as zip_ref:
                zip_ref.extractall(workspace)

            self._log(f"Task {task_id} files extracted to {workspace}")

            # 確保 run_task.sh 存在於 workspace
            script_src = join(dirname(__file__), "run_task.sh")
            script_dst = join(workspace, "run_task.sh")
            copy2(script_src, script_dst)
            chmod(script_dst, 0o755)

            # 執行容器，使用新的鏡像名稱
            container_name = f"task-{task_id}-{token_hex(4)}"
            container = self.docker_client.containers.run(
                "justin308/hivemind-worker:latest",
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
            last_log_fetch = time()
            
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
                        self._log(error_log, WARNING)
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
                current_time = time()
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
                                    task_logs.append(line)
                                    log_send_counter += 1
                            
                            # 每20行或每3秒發送一次日誌
                            if log_send_counter >= 20 or len(log_buffer) > 0:
                                logs_to_send = "\n".join(log_buffer)
                                self._send_task_logs(task_id, logs_to_send)
                                log_buffer.clear()
                                log_send_counter = 0
                        
                        last_log_fetch = current_time
                    except Exception as e:
                        self._log(f"Error collecting logs: {e}", WARNING)
                
                # 短暫休眠，確保能快速響應停止請求
                sleep(0.1)
            
            # 發送剩餘的日誌
            if log_buffer:
                logs_to_send = "\n".join(log_buffer)
                self._send_task_logs(task_id, logs_to_send)
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
                    self._log(error_log, ERROR)
                    task_logs.append(error_log)
                    self._send_task_logs(task_id, error_log)
            
        except Exception as e:
            error_log = f"任務 {task_id} 執行失敗: {str(e)}"
            self._log(error_log, ERROR)
            task_logs.append(error_log)
            self._send_task_logs(task_id, error_log)
            success = False
        
        finally:
            # 清理容器（強制清理）
            if container:
                try:
                    try:
                        container.kill()
                    except:
                        pass
                    container.remove(force=True)
                    cleanup_log = f"任務 {task_id} 容器已強制清理"
                    self._log(cleanup_log)
                    task_logs.append(cleanup_log)
                    self._send_task_logs(task_id, cleanup_log)
                except Exception as e:
                    cleanup_error = f"清理容器失敗: {str(e)}"
                    self._log(cleanup_error, WARNING)
                    task_logs.append(cleanup_error)
                    self._send_task_logs(task_id, cleanup_error)
            
            # 清理臨時目錄
            if temp_dir and exists(temp_dir):
                try:
                    rmtree(temp_dir)
                    self._log(f"Temporary directory {temp_dir} cleaned up")
                except Exception as e:
                    self._log(f"Failed to clean up temp dir: {e}", WARNING)
            
            # 發送最終的完整日誌
            try:
                final_logs = "\n".join(task_logs)
                self._send_task_logs(task_id, f"=== 任務完整日誌 ===\n{final_logs}\n=== 日誌結束 ===")
            except Exception as e:
                self._log(f"Failed to send final logs: {e}", WARNING)
            
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
                self._log(f"Failed to notify task completion: {e}", WARNING)
            
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
            log_file = join(workspace, "execution_log.txt")
            with open(log_file, 'w', encoding='utf-8') as f:
                f.write(f"Task ID: {task_id}\n")
                if stopped:
                    f.write(f"Status: Stopped by user\n")
                    f.write(f"Execution Result: Terminated\n")
                else:
                    f.write(f"Status: {'Success' if success else 'Failed'}\n")
                    f.write(f"Execution Result: {'Completed' if success else 'Error'}\n")
                f.write(f"Time: {datetime.now()}\n")
                f.write(f"Node: {self.node_id}\n")
                
                if stopped:
                    f.write(f"\nNote: This task was stopped by user request.\n")
                    f.write(f"Any partial results or intermediate files are included in this package.\n")
            
            # 創建任務完整日誌文件
            if task_logs:
                task_log_file = join(workspace, "task_logs.txt")
                with open(task_log_file, 'w', encoding='utf-8') as f:
                    f.write(f"=== Task {task_id} Complete Logs ===\n")
                    f.write(f"Generated at: {datetime.now()}\n")
                    f.write(f"Status: {'Stopped by user' if stopped else ('Success' if success else 'Failed')}\n")
                    f.write(f"Node: {self.node_id}\n\n")
                    
                    for log_entry in task_logs:
                        f.write(f"{log_entry}\n")
                    
                    f.write(f"\n=== End of Logs ===\n")
            
            # 創建停止狀態文件（如果任務被停止）
            if stopped:
                stop_file = join(workspace, "task_stopped.txt")
                with open(stop_file, 'w', encoding='utf-8') as f:
                    f.write(f"Task {task_id} was stopped by user request at {datetime.now()}\n")
                    f.write(f"This file indicates that the task did not complete normally.\n")
                    f.write(f"Check execution_log.txt and task_logs.txt for more details.\n")
            
            # 打包整個工作目錄
            zip_buffer = BytesIO()
            with ZipFile(zip_buffer, 'w', ZIP_DEFLATED) as zip_file:
                for root, dirs, files in walk(workspace):
                    for file in files:
                        file_path = join(root, file)
                        arcname = relpath(file_path, workspace)
                        zip_file.write(file_path, arcname)
            
            result_size = len(zip_buffer.getvalue())
            self._log(f"Created result zip for task {task_id}: {result_size} bytes ({'stopped' if stopped else 'completed'}), logs included")
            return zip_buffer.getvalue()
            
        except Exception as e:
            self._log(f"Failed to create result zip: {e}", ERROR)
            
            # 如果打包失敗，創建一個包含錯誤信息的簡單ZIP
            try:
                zip_buffer = BytesIO()
                with ZipFile(zip_buffer, 'w', ZIP_DEFLATED) as zip_file:
                    error_content = f"Task {task_id} packaging failed: {str(e)}\n"
                    error_content += f"Status: {'Stopped' if stopped else 'Failed'}\n"
                    error_content += f"Time: {datetime.now()}\n"
                    
                    # 嘗試包含部分日誌
                    if task_logs:
                        error_content += f"\n=== Partial Logs ===\n"
                        for log_entry in task_logs[-50:]:  # 最後50行日誌
                            error_content += f"{log_entry}\n"
                    
                    zip_file.writestr("error_log.txt", error_content)
                return zip_buffer.getvalue()
            except:
                return None

    def _log(self, message, level=INFO):
        """記錄日誌"""
        if level == INFO:
            info(message)
        elif level == WARNING:
            warning(message)
        elif level == ERROR:
            error(message)
        
        with self.log_lock:
            timestamp = datetime.now().strftime('%H:%M:%S')
            level_name = getLevelName(level)
            self.logs.append(f"{timestamp} - {level_name} - {message}")
            if len(self.logs) > 500:
                self.logs.pop(0)

# gRPC 服務實現
class WorkerNodeServicer(nodepool_pb2_grpc.WorkerNodeServiceServicer):
    def __init__(self, worker_node):
        self.worker_node = worker_node

    def ExecuteTask(self, request, context):
        """執行任務 RPC"""
        task_id = request.task_id
        task_zip = request.task_zip
        
        file_size_mb = len(task_zip) / (1024 * 1024)
        info(f"===== 收到執行任務請求 =====")
        info(f"任務ID: {task_id}")
        info(f"檔案大小: {file_size_mb:.1f}MB")
        info(f"當前節點狀態: {self.worker_node.status}")
        info(f"是否已註冊: {self.worker_node.is_registered}")
        info(f"Docker 可用: {self.worker_node.docker_available}")
        
        # 快速檢查節點狀態
        if self.worker_node.current_task_id:
            error_msg = f"節點忙碌中，拒絕任務 {task_id} (當前任務: {self.worker_node.current_task_id})"
            warning(error_msg)
            return nodepool_pb2.ExecuteTaskResponse(
                success=False, 
                message=error_msg
            )
        
        if not self.worker_node.docker_available:
            error_msg = f"Docker 不可用，拒絕任務 {task_id}"
            error(error_msg)
            return nodepool_pb2.ExecuteTaskResponse(
                success=False, 
                message="Docker not available"
            )
        
        # 檢查任務數據完整性和大小
        if not task_zip:
            error_msg = f"任務 {task_id} 數據為空"
            error(error_msg)
            return nodepool_pb2.ExecuteTaskResponse(
                success=False, 
                message="Task data is empty"
            )
        
        # 檢查檔案大小限制（100MB）
        if file_size_mb > 100:
            error_msg = f"任務 {task_id} 檔案太大: {file_size_mb:.1f}MB，超過100MB限制"
            error(error_msg)
            return nodepool_pb2.ExecuteTaskResponse(
                success=False, 
                message=f"Task file too large: {file_size_mb:.1f}MB (limit: 100MB)"
            )
        
        try:
            # 立即響應接受任務，避免超時
            self.worker_node.current_task_id = task_id
            self.worker_node.status = f"Receiving: {task_id} ({file_size_mb:.1f}MB)"
            
            info(f"開始接受任務 {task_id}，狀態更新為: {self.worker_node.status}")
            
            # 預先驗證 ZIP 檔案完整性
            try:
                with ZipFile(BytesIO(task_zip), 'r') as zip_ref:
                    zip_ref.testzip()
                info(f"任務 {task_id} ZIP 檔案驗證成功")
            except Exception as zip_error:
                self.worker_node.current_task_id = None
                self.worker_node.status = "Idle"
                error_msg = f"任務 {task_id} ZIP 檔案損壞: {zip_error}"
                error(error_msg)
                return nodepool_pb2.ExecuteTaskResponse(
                    success=False, 
                    message=f"Invalid ZIP file: {str(zip_error)}"
                )
            
            # 更新狀態為準備執行
            self.worker_node.status = f"Preparing: {task_id}"
            info(f"任務 {task_id} 檔案驗證完成，開始準備執行")
            
            # 啟動執行線程
            execution_thread = Thread(
                target=self.worker_node._execute_task,
                args=(task_id, task_zip),
                daemon=True,
                name=f"TaskExecution-{task_id}"
            )
            execution_thread.start()
            
            success_msg = f"任務 {task_id} 已接受並開始準備執行 (檔案大小: {file_size_mb:.1f}MB)"
            info(success_msg)
            info(f"===== 任務接受完成 =====")
            
            return nodepool_pb2.ExecuteTaskResponse(
                success=True, 
                message=f"Task {task_id} accepted, file size: {file_size_mb:.1f}MB"
            )
            
        except Exception as e:
            # 如果出錯，重置狀態
            self.worker_node.current_task_id = None
            self.worker_node.status = "Idle"
            error_msg = f"接受任務 {task_id} 時發生錯誤: {e}"
            error(error_msg)
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
            info(f"收到停止任務 {task_id} 的請求，立即執行強制停止")
            
            # 立即設置停止標誌
            self.worker_node._stop_current_task = True
            
            # 等待任務處理停止請求，但時間較短
            max_wait_time = 10
            wait_count = 0
            while self.worker_node.current_task_id == task_id and wait_count < max_wait_time:
                sleep(0.5)
                wait_count += 0.5
                
                # 每2秒報告一次進度
                if int(wait_count) % 2 == 0 and wait_count > 0:
                    info(f"強制停止任務 {task_id} 處理中... ({wait_count:.1f}/{max_wait_time}秒)")
            
            if wait_count >= max_wait_time:
                # 如果超時，直接重置狀態
                self.worker_node.current_task_id = None
                self.worker_node.status = "Idle"
                self.worker_node._stop_current_task = False
                warning(f"任務 {task_id} 超時強制停止（{max_wait_time} 秒）")
                return nodepool_pb2.StopTaskExecutionResponse(
                    success=True,
                    message=f"Task {task_id} force stopped (timeout after {max_wait_time}s)"
                )
            else:
                info(f"任務 {task_id} 已成功停止並打包結果（耗時 {wait_count:.1f} 秒）")
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
        worker = WorkerNode()
        
        # 啟動 gRPC 服務
        server = grpc.server(ThreadPoolExecutor(max_workers=5))
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
                sleep(60)
        except KeyboardInterrupt:
            worker._log("Shutting down...")
            worker._stop_status_reporting()
            server.stop(grace=5)
            
    except Exception as e:
        critical(f"Failed to start worker: {e}")
        exit(1)