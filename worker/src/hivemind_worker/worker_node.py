from sys import exit, path
from os.path import join, dirname, abspath, exists, relpath
from os import environ, makedirs, chmod, walk, _exit, system
import sys
import os
sys.path.insert(0, os.path.abspath(os.path.dirname(__file__)))
from docker import from_env
from docker.errors import ImageNotFound, APIError
import grpc
from threading import Thread, Event, Lock
from logging import basicConfig, info, warning, error, critical, INFO, WARNING, ERROR, getLevelName
from flask import Flask, jsonify, request, render_template, session, redirect, url_for
import nodepool_pb2
import nodepool_pb2_grpc
from concurrent.futures import ThreadPoolExecutor
from psutil import cpu_count, virtual_memory, cpu_percent
from time import time, sleep
from zipfile import ZipFile, ZIP_DEFLATED
from io import BytesIO
from tempfile import mkdtemp
from subprocess import run
from platform import node, system
# 只在 Windows 匯入 CREATE_NO_WINDOW
try:
    from subprocess import CREATE_NO_WINDOW
except ImportError:
    CREATE_NO_WINDOW = None
from datetime import datetime, timedelta
from secrets import token_hex
from shutil import copy2, rmtree
from socket import socket, AF_INET, SOCK_DGRAM
from uuid import uuid4
from webbrowser import open as web_open
from netifaces import interfaces, ifaddresses, AF_INET
from requests import post, get, exceptions
import venv
import threading
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
        # 改為字典以支持多任務
        self.running_tasks = {}  # {task_id: {"status": status, "resources": {}, "start_time": time()}}
        self.task_locks = {}  # {task_id: threading.Lock()}
        self.username = None
        self.token = None
        self.is_registered = False
        self.login_time = None
        self.cpt_balance = 0
        self.trust_score = 0  # 添加信任分數
        self.trust_group = "low"  # 信任分組: high, medium, low

        # 線程控制
        self.status_thread = None
        self._stop_event = Event()
        self.logs = []
        self.log_lock = Lock()
        self.resources_lock = Lock()  # 添加資源鎖

        # 資源管理
        self.available_resources = {
            "cpu": 0,        # CPU 分數
            "memory_gb": 0,  # 可用內存GB
            "gpu": 0,        # GPU 分數
            "gpu_memory_gb": 0  # GPU 內存GB
        }
        self.total_resources = {
            "cpu": 0,
            "memory_gb": 0,
            "gpu": 0,
            "gpu_memory_gb": 0
        }

        # 用戶會話管理
        self.user_sessions = {}
        self.session_lock = Lock()
        self.task_stop_events = {}  # {task_id: Event()}

        # 先自動連線 VPN
        self._auto_join_vpn()

        # 硬體信息
        self._init_hardware()
        # Docker 初始化
        self._init_docker()
        # gRPC 連接
        self._init_grpc()  # 確保 gRPC stub 初始化在 Flask 之前
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
                    self._log(f"Automatically obtained WireGuard config and wrote to {config_path}")
                except Exception as e:
                    self._log(f"Failed to write WireGuard config: {e}", WARNING)
                    return
                # Windows/Linux 都在當前目錄執行 wg-quick，需有權限與路徑
                from os import system
                result = system(f"wg-quick down {config_path} 2>/dev/null; wg-quick up {config_path}")
                if result == 0:
                    self._log("WireGuard VPN started successfully")
                else:
                    self._log("WireGuard VPN failed to start, please check permissions and config", WARNING)
                    self._prompt_manual_vpn(config_path)
            else:
                error_msg = resp_json.get("error") if resp_json else resp.text
                self._log(f"Failed to automatically obtain WireGuard config: {error_msg}", WARNING)
                if error_msg and "VPN 服務不可用" in error_msg:
                    self._log("Please ensure the master Flask started with WireGuardServer initialized and /api/vpn/join is available", WARNING)
                self._prompt_manual_vpn()
        except Exception as e:
            self._log(f"Failed to request /api/vpn/join automatically: {e}", WARNING)
            self._prompt_manual_vpn()

    def _prompt_manual_vpn(self, config_path=None):
        """提示用戶手動連線 WireGuard"""
        msg = (
            "\n[Notice] Failed to connect to WireGuard automatically. Please connect to VPN manually:\n"
            "1. Find your config file (wg0.conf).\n"
            "2. Open your WireGuard client and import the config.\n"
            "3. If you encounter permission issues, run as administrator/root.\n"
        )
        print(msg)
        print('If you have already connected, please press y')
        a = input()
        if a == 'y':
            # 不記錄任何日誌
            pass

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
            
            # 設置初始可用資源與總資源
            self.total_resources = {
                "cpu": self.cpu_score,
                "memory_gb": self.memory_gb,
                "gpu": self.gpu_score,
                "gpu_memory_gb": self.gpu_memory_gb
            }
            
            # 初始時所有資源都可用
            self.available_resources = self.total_resources.copy()
            
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
            
            # 設置預設資源
            self.total_resources = {
                "cpu": 0,
                "memory_gb": 0,
                "gpu": 0,
                "gpu_memory_gb": 0
            }
            self.available_resources = self.total_resources.copy()

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
                                'Asia': 'Asia', 'Africa': 'Africa', 'North America': 'North America',
                                'South America': 'South America', 'Europe': 'Europe', 'Oceania': 'Oceania'
                            }
                            
                            detected_region = continent_mapping.get(continent)
                            if detected_region:
                                self._log(f"Auto-detected location: {country} -> {detected_region}")
                                return detected_region
                        
                except (exceptions.RequestException, Exception):
                    continue
            
            self._log("Location detection failed, using Unknown")
            return "Unknown"
                    
        except Exception as e:
            self._log(f"Location detection error: {e}")
            return "Unknown"

    def _get_local_ip(self):
        """獲取本機 IP 地址（優先使用 WireGuard 網卡）"""
        try:
            # 檢查所有網卡接口
            interfaces_list = interfaces()
            self._log(f"Detected network interfaces: {interfaces_list}")
            
            # 優先檢查 WireGuard 相關接口
            wg_interfaces = [iface for iface in interfaces_list if 'wg' in iface.lower() or 'wireguard' in iface.lower()]
            
            if wg_interfaces:
                for wg_iface in wg_interfaces:
                    try:
                        addrs = ifaddresses(wg_iface)
                        if AF_INET in addrs:
                            wg_ip = addrs[AF_INET][0]['addr']
                            self._log(f"Detected WireGuard interface {wg_iface}, IP: {wg_ip}")
                            return wg_ip
                    except Exception as e:
                        self._log(f"Failed to check interface {wg_iface}: {e}")
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
                                self._log(f"Detected VPN subnet IP: {ip} (interface: {iface})")
                                return ip
                except Exception as e:
                    continue
            
            # 如果沒有找到 VPN IP，使用預設方法
            self._log("No WireGuard interface detected, using default interface")
            
        except Exception as e:
            self._log(f"Network interface detection failed: {e}")
        
        # 預設方法：連接外部服務獲取本機 IP
        try:
            s = socket(AF_INET, SOCK_DGRAM)
            s.connect(("8.8.8.8", 80))
            ip = s.getsockname()[0]
            s.close()
            self._log(f"Obtained IP using default method: {ip}")
            return ip
        except:
            self._log("All methods failed, using 127.0.0.1")
            return "127.0.0.1"

    def update_location(self, new_location):
        """更新節點地區設定"""
        available_locations = ["Asia", "Africa", "North America", "South America", "Europe", "Oceania", "Unknown"]
        
        if new_location in available_locations:
            old_location = self.location
            self.location = new_location
            self._log(f"Location updated: {old_location} -> {new_location}")
            
            # 如果已註冊，需要重新註冊以更新地區信息
            if self.is_registered and self.token:
                self._register()
            
            return True, f"Location updated to: {new_location}"
        else:
            return False, f"Invalid location selection: {new_location}"

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
                # 僅在 Windows 下傳遞 creationflags
                result = run(cmd, capture_output=True, text=True, timeout=10, 
                            creationflags=CREATE_NO_WINDOW if CREATE_NO_WINDOW else 0)
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
            self.docker_status = "available"
            
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
            self.docker_status = "unavailable"

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
        base_dir = dirname(abspath(__file__))
        self.app = Flask(
            __name__,
            template_folder=join(base_dir, "templates"),
            static_folder=join(base_dir, "static")
        )
        self.app.secret_key = token_hex(32)
        
        # 關閉 Flask 預設日誌
        import logging
        log = logging.getLogger('werkzeug')
        log.setLevel(logging.ERROR)
        self.app.logger.disabled = True

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
            available_locations = ["Asia", "Africa", "North America", "South America", "Europe", "Oceania", "Unknown"]
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
            
            available_locations = ["Asia", "Africa", "North America", "South America", "Europe", "Oceania", "Unknown"]
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
                
                available_locations = ["Asia", "Africa", "North America", "South America", "Europe", "Oceania", "Unknown"]
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
                    available_locations = ["Asia", "Africa", "North America", "South America", "Europe", "Oceania", "Unknown"]
                    return render_template('login.html', 
                                         error=f"Location setting error: {message}", 
                                         node_id=self.node_id, 
                                         current_status=self.status,
                                         current_location=self.location,
                                         available_locations=available_locations)
            
            if not username or not password:
                available_locations = ["Asia", "Africa", "North America", "South America", "Europe", "Oceania", "Unknown"]
                return render_template('login.html', 
                                     error="Please enter username and password", 
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
                available_locations = ["Asia", "Africa", "North America", "South America", "Europe", "Oceania", "Unknown"]
                return render_template('login.html', 
                                     error=f"Login failed: {self.status}", 
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
                    return jsonify({'success': False, 'error': 'Please select a location'})
                
                success, message = self.update_location(new_location)
                return jsonify({'success': success, 'message': message, 'current_location': self.location})
                
            except Exception as e:
                return jsonify({'success': False, 'error': f'Update failed: {str(e)}'})

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

            # 獲取目前執行中的任務
            with self.resources_lock:
                task_count = len(self.running_tasks)
                # 對於前端相容性，如果有任務則使用第一個任務的ID
                current_task_id = next(iter(self.running_tasks.keys()), None) if task_count > 0 else None
                
                # 生成任務列表
                tasks = []
                for task_id, task_info in self.running_tasks.items():
                    tasks.append({
                        'id': task_id,
                        'status': task_info.get('status', 'Unknown'),
                        'start_time': datetime.fromtimestamp(task_info.get('start_time', time())).isoformat(),
                        'resources': task_info.get('resources', {})
                    })

            return jsonify({
                'node_id': self.node_id,
                'status': self.status,
                'current_task_id': current_task_id or "None",  # backward compatibility for old frontend
                'is_registered': self.is_registered,
                'docker_available': self.docker_available,
                'docker_status': getattr(self, 'docker_status', 'unknown'),
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
                'ip': getattr(self, 'local_ip', '127.0.0.1'),
                'task_count': task_count,
                'tasks': tasks,  # add task list
                'available_resources': self.available_resources,
                'total_resources': self.total_resources
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

        # 添加任務狀態路由
        @self.app.route('/api/tasks')
        def api_tasks():
            session_id = session.get('session_id')
            user_data = self._get_user_session(session_id) if session_id else None
            
            if not user_data and self.username:
                user_data = {'username': self.username}
            
            if not user_data:
                return jsonify({'error': 'Unauthorized'}), 401
                
            # 返回所有正在運行的任務
            tasks_info = []
            with self.resources_lock:
                for task_id, task_data in self.running_tasks.items():
                    tasks_info.append({
                        'task_id': task_id,
                        'status': task_data.get('status', 'Unknown'),
                        'start_time': datetime.fromtimestamp(task_data.get('start_time', 0)).isoformat(),
                        'elapsed': round(time() - task_data.get('start_time', time()), 1),
                        'resources': task_data.get('resources', {})
                    })
            
            return jsonify({
                'tasks': tasks_info,
                'total_resources': self.total_resources,
                'available_resources': self.available_resources
            })

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
                
                # 嘗試獲取用戶信任分數
                try:
                    balance_response = self.user_stub.GetBalance(
                        nodepool_pb2.GetBalanceRequest(username=username, token=response.token),
                        metadata=[('authorization', f'Bearer {response.token}')],
                        timeout=10
                    )
                    if balance_response.success:
                        self.cpt_balance = balance_response.balance
                        
                        # 從用戶數據獲取真實信任分數（如果 API 支持）
                        # TODO: 實現獲取用戶信任分數的 API
                        # 目前根據餘額計算一個基礎信任分數
                        self.trust_score = min(int(balance_response.balance / 10), 1000)
                        
                        # 根據信任分數設置信任群組
                        if self.trust_score >= 200:
                            self.trust_group = "high"
                        elif self.trust_score >= 100:
                            self.trust_group = "medium"
                        else:
                            self.trust_group = "low"
                        
                        # Docker不可用則降級信任群組
                        if not self.docker_available:
                            if self.trust_group == "high":
                                self.trust_group = "medium"
                            elif self.trust_group == "medium":
                                self.trust_group = "low"
                            
                        self._log(f"User {username} balance: {self.cpt_balance} CPT, trust score: {self.trust_score}, group: {self.trust_group}")
                except Exception as e:
                    self._log(f"Failed to get user balance and trust info: {e}", WARNING)
                    # 使用保守的預設值
                    self.cpt_balance = 0
                    self.trust_score = 0
                    self.trust_group = "low"

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
            # 添加docker狀態到註冊請求
            request = nodepool_pb2.RegisterWorkerNodeRequest(
                node_id=self.username,
                hostname=self.local_ip,  # 使用本機 IP 而不是 127.0.0.1
                cpu_cores=int(self.cpu_cores),
                memory_gb=int(self.memory_gb),
                cpu_score=self.cpu_score,
                gpu_score=self.gpu_score,
                gpu_name=self.gpu_name,
                gpu_memory_gb=int(self.gpu_memory_gb),
                location=self.location,
                port=self.port,
                docker_status=self.docker_status  # 新增docker狀態
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
        
        # 清理所有運行中任務
        self._stop_all_tasks()
        
        self.login_time = None
        self.cpt_balance = 0
        self.trust_score = 0
        self.trust_group = "low"
        self._stop_status_reporting()
        
        if old_username:
            self._log(f"User {old_username} logged out")
    
    def _stop_all_tasks(self):
        """停止所有運行中的任務"""
        task_ids = list(self.running_tasks.keys())
        for task_id in task_ids:
            self._log(f"停止任務 {task_id} (登出操作)")
            self._stop_task(task_id)
        
        # 等待所有任務停止
        timeout = 30
        start_time = time()
        while self.running_tasks and time() - start_time < timeout:
            sleep(0.5)
        
        # 強制清理
        with self.resources_lock:
            self.running_tasks.clear()
            self.task_locks.clear()
            self.task_stop_events.clear()
            
            # 重置可用資源
            self.available_resources = self.total_resources.copy()

    def _stop_task(self, task_id):
        """停止指定任務"""
        if task_id in self.task_stop_events:
            self.task_stop_events[task_id].set()
            self._log(f"已發送停止信號給任務 {task_id}")
            return True
        return False

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
                    # 決定狀態消息
                    with self.resources_lock:
                        task_count = len(self.running_tasks)
                    
                    if task_count > 0:
                        status_msg = f"Running {task_count} tasks"
                    else:
                        status_msg = self.status
                    
                    # 發送心跳
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
                    
                    # 為所有運行中的任務回報資源使用情況
                    self._report_all_tasks_resource_usage()
                    
                except Exception as e:
                    self._log(f"Status report failed: {e}", WARNING)
            
            # 縮短心跳間隔以確保連接穩定
            self._stop_event.wait(5) 

    def _report_all_tasks_resource_usage(self):
        """回報所有任務的資源使用情況"""
        if not self.running_tasks:
            return
            
        try:
            with self.resources_lock:
                task_ids = list(self.running_tasks.keys())
            
            for task_id in task_ids:
                try:
                    self._report_task_resource_usage(task_id)
                except Exception as e:
                    self._log(f"回報任務 {task_id} 資源使用失敗: {e}", WARNING)
        except Exception as e:
            self._log(f"回報任務資源使用發生錯誤: {e}", WARNING)

    def _report_task_resource_usage(self, task_id):
        """回報單個任務的資源使用情況"""
        if not self.token:
            return
            
        try:
            # 獲取任務資源使用情況
            with self.resources_lock:
                if task_id not in self.running_tasks:
                    return
                    
                task_data = self.running_tasks[task_id]
                resources = task_data.get('resources', {})
            
            # 獲取真實的系統資源使用情況
            try:
                # 獲取當前系統 CPU 使用率
                current_cpu_percent = cpu_percent(interval=0.1)
                
                # 獲取當前內存使用情況
                memory_info = virtual_memory()
                memory_used_mb = int((memory_info.total - memory_info.available) / (1024 * 1024))
                
                # 對於 GPU，由於複雜性，暫時設為 0（需要 nvidia-ml-py 等庫）
                gpu_usage_percent = 0
                gpu_memory_used_mb = 0
                
                # 如果有 GPU 資源分配，嘗試獲取 GPU 使用情況
                if resources.get('gpu', 0) > 0:
                    # TODO: 實現真實的 GPU 監控
                    # 目前暫時使用預設值
                    gpu_usage_percent = 0
                    gpu_memory_used_mb = 0
                
            except Exception as e:
                self._log(f"獲取系統資源使用失敗: {e}", WARNING)
                # 如果獲取失敗，使用保守的預設值
                current_cpu_percent = 50
                memory_used_mb = int(resources.get('memory_gb', 1) * 512)  # 假設使用一半
                gpu_usage_percent = 0
                gpu_memory_used_mb = 0
            
            # 發送資源使用報告
            try:
                self._log(f"向主節點匯報任務 {task_id} 真實資源使用: CPU={current_cpu_percent:.1f}%, MEM={memory_used_mb}MB, GPU={gpu_usage_percent}%, GPU_MEM={gpu_memory_used_mb}MB")
                
                # 使用 StoreOutput 方法傳送資源使用信息
                usage_info = f"{{\"cpu_percent\":{current_cpu_percent:.1f},\"memory_mb\":{memory_used_mb},\"gpu_percent\":{gpu_usage_percent},\"gpu_memory_mb\":{gpu_memory_used_mb}}}"
                
                response = self.master_stub.StoreOutput(
                    nodepool_pb2.StoreOutputRequest(
                        task_id=task_id,
                        output=f"RESOURCE_USAGE: {usage_info}"
                    ),
                    metadata=[('authorization', f'Bearer {self.token}')],
                    timeout=10
                )
                
                if response.success:
                    self._log(f"成功匯報任務 {task_id} 真實資源使用")
            except Exception as e:
                self._log(f"向主節點匯報資源使用失敗: {e}", WARNING)
            
        except Exception as e:
            self._log(f"回報任務 {task_id} 資源使用失敗: {e}", WARNING)

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
                self.cpt_balance = response.balance
                # 更新所有該用戶的會話餘額
                with self.session_lock:
                    for session_id, session_data in self.user_sessions.items():
                        if session_data['username'] == self.username:
                            session_data['cpt_balance'] = response.balance
        except Exception as e:
            self._log(f"更新餘額失敗: {e}", WARNING)

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

    def _check_resources_available(self, required_resources):
        """檢查是否有足夠資源運行任務"""
        with self.resources_lock:
            for resource_type, required_amount in required_resources.items():
                if resource_type in self.available_resources:
                    if self.available_resources[resource_type] < required_amount:
                        return False
            return True

    def _allocate_resources(self, task_id, required_resources):
        """分配資源給任務"""
        with self.resources_lock:
            # 檢查資源是否足夠
            for resource_type, required_amount in required_resources.items():
                if resource_type in self.available_resources:
                    if self.available_resources[resource_type] < required_amount:
                        return False
            
            # 扣除資源
            for resource_type, required_amount in required_resources.items():
                if resource_type in self.available_resources:
                    self.available_resources[resource_type] -= required_amount
            
            # 記錄任務使用的資源
            self.running_tasks[task_id] = {
                "status": "Allocated",
                "resources": required_resources,
                "start_time": time()
            }
            
            # 創建任務的鎖和停止事件
            self.task_locks[task_id] = threading.Lock()
            self.task_stop_events[task_id] = Event()
            
            return True

    def _release_resources(self, task_id):
        """釋放任務使用的資源"""
        with self.resources_lock:
            if task_id not in self.running_tasks:
                return
            
            # 獲取任務使用的資源
            task_resources = self.running_tasks[task_id].get('resources', {})
            
            # 歸還資源
            for resource_type, amount in task_resources.items():
                if resource_type in self.available_resources:
                    self.available_resources[resource_type] += amount
            
            # 清理任務數據
            del self.running_tasks[task_id]
            
            # 清理鎖和停止事件
            if task_id in self.task_locks:
                del self.task_locks[task_id]
            if task_id in self.task_stop_events:
                del self.task_stop_events[task_id]

    def _execute_task(self, task_id, task_zip_bytes, required_resources=None):
        """執行任務"""
        if not required_resources:
            required_resources = {
                "cpu": 1,
                "memory_gb": 1,
                "gpu": 0,
                "gpu_memory_gb": 0
            }
        
        # 更新任務狀態
        with self.resources_lock:
            if task_id in self.running_tasks:
                self.running_tasks[task_id]["status"] = "Executing"
        
        # 獲取任務的停止事件
        stop_event = self.task_stop_events.get(task_id)
        if not stop_event:
            self._log(f"找不到任務 {task_id} 的停止事件", ERROR)
            self._release_resources(task_id)
            return
        
        temp_dir = None
        container = None
        success = False
        task_logs = []
        stop_requested = False
        
        try:
            # 決定使用Docker還是venv
            use_docker = self.docker_available
            
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

            if use_docker:
                # 使用Docker運行任務
                container_name = f"task-{task_id}-{token_hex(4)}"
                
                # 設置Docker資源限制
                mem_limit = f"{int(required_resources.get('memory_gb', 1) * 1024)}m"
                cpu_limit = required_resources.get('cpu', 1) / 100  # Docker CPU限制為相對值
                
                # 設置GPU相關配置
                device_requests = []
                if required_resources.get('gpu', 0) > 0:
                    device_requests.append({
                        'Driver': 'nvidia',
                        'Count': -1,  # 使用所有可用GPU
                        'Capabilities': [['gpu']]
                    })
                
                container = self.docker_client.containers.run(
                    "justin308/hivemind-worker:latest",
                    command=["bash", "/app/task/run_task.sh"],
                    detach=True,
                    name=container_name,
                    volumes={workspace: {'bind': '/app/task', 'mode': 'rw'}},
                    working_dir="/app/task",
                    environment={"TASK_ID": task_id, "PYTHONUNBUFFERED": "1"},
                    mem_limit=mem_limit,
                    nano_cpus=int(cpu_limit * 1e9),  # 轉換為納秒CPU時間
                    device_requests=device_requests if device_requests else None,
                    remove=False
                )
                
                self._log(f"Task {task_id} Docker container started with resources: CPU={cpu_limit}, Memory={mem_limit}")
                
                # 監控Docker容器
                log_buffer = []
                log_send_counter = 0
                last_log_fetch = time()
                
                while not stop_event.is_set():
                    try:
                        # 檢查容器狀態
                        container.reload()
                        if container.status != 'running':
                            self._log(f"Container {container_name} stopped, status: {container.status}")
                            break
                        
                        # 收集日誌
                        current_time = time()
                        if current_time - last_log_fetch > 1.0:
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
                        self._log(f"Error monitoring container: {e}", WARNING)
                        break
                    
                    # 短暫休眠
                    sleep(0.1)
                
                # 處理停止請求
                if stop_event.is_set():
                    stop_requested = True
                    stop_log = f"收到停止請求，立即終止任務 {task_id}"
                    self._log(stop_log)
                    task_logs.append(stop_log)
                    self._send_task_logs(task_id, stop_log)
                    
                    try:
                        container.kill()
                    except Exception as e:
                        self._log(f"強制停止容器失敗: {e}", WARNING)
                else:
                    # 檢查容器退出狀態
                    try:
                        result = container.wait(timeout=2)
                        success = result.get('StatusCode', -1) == 0
                    except Exception as e:
                        self._log(f"Failed to get container exit status: {e}", WARNING)
                        success = False
            else:
                # 使用venv運行任務
                venv_dir = join(temp_dir, "venv")
                self._log(f"Creating virtual environment for task {task_id} at {venv_dir}")
                
                # 創建虛擬環境
                venv.create(venv_dir, with_pip=True)
                
                # 準備運行腳本（修改run_task.sh或創建新腳本）
                venv_script = join(workspace, "run_venv.sh")
                with open(venv_script, 'w') as f:
                    f.write(f"""#!/bin/bash
# 激活虛擬環境
source {join(venv_dir, 'bin/activate')} || source {join(venv_dir, 'Scripts/activate')}

# 安裝依賴
if [ -f requirements.txt ]; then
    pip install -r requirements.txt
fi

# 運行任務
if [ -f run.py ]; then
    python run.py
elif [ -f main.py ]; then
    python main.py
else
    # 尋找並運行第一個找到的Python文件
    PYTHON_FILE=$(find . -maxdepth 1 -name "*.py" | head -1)
    if [ ! -z "$PYTHON_FILE" ]; then
        python $PYTHON_FILE
    else
        echo "No Python file found"
        exit 1
    fi
fi
""")
                chmod(venv_script, 0o755)
                
                # 啟動任務執行進程
                import subprocess
                process = None
                
                try:
                    self._log(f"Starting task {task_id} with venv")
                    
                    # 使用subprocess運行腳本
                    process = subprocess.Popen(
                        [venv_script],
                        cwd=workspace,
                        stdout=subprocess.PIPE,
                        stderr=subprocess.STDOUT,
                        universal_newlines=True,
                        bufsize=1
                    )
                    
                    # 設置結果監控
                    log_buffer = []
                    log_send_counter = 0
                    
                    # 監控輸出
                    for line in iter(process.stdout.readline, ''):
                        if stop_event.is_set():
                            break
                            
                        if line.strip():
                            self._log(f"[Task {task_id}]: {line.strip()}")
                            log_buffer.append(line.strip())
                            task_logs.append(line.strip())
                            log_send_counter += 1
                            
                            # 定期發送日誌
                            if log_send_counter >= 20:
                                logs_to_send = "\n".join(log_buffer)
                                self._send_task_logs(task_id, logs_to_send)
                                log_buffer.clear()
                                log_send_counter = 0
                    
                    # 等待進程完成
                    process.wait(timeout=1)
                    success = process.returncode == 0
                    
                except subprocess.TimeoutExpired:
                    self._log(f"Process timed out for task {task_id}", WARNING)
                    success = False
                    
                # 處理停止請求
                if stop_event.is_set():
                    stop_requested = True
                    stop_log = f"收到停止請求，立即終止任務 {task_id}"
                    self._log(stop_log)
                    task_logs.append(stop_log)
                    self._send_task_logs(task_id, stop_log)
                    
                    # 終止進程
                    if process and process.poll() is None:
                        try:
                            process.terminate()
                            sleep(0.5)
                            if process.poll() is None:
                                process.kill()
                        except Exception as e:
                            self._log(f"終止進程失敗: {e}", WARNING)
            
            # 處理任務完成或停止
            if stop_requested:
                completion_log = f"任務 {task_id} 被用戶強制停止"
            else:
                completion_log = f"任務 {task_id} 執行完成，狀態: {'成功' if success else '失敗'}"
            
            self._log(completion_log)
            task_logs.append(completion_log)
            self._send_task_logs(task_id, completion_log)
            
            # 打包結果
            result_zip = self._create_result_zip(task_id, workspace, success, stop_requested, task_logs)
            
            # 發送結果
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
                    self._log(f"任務 {task_id} 結果已發送")
                except Exception as e:
                    self._log(f"發送任務結果失敗: {e}", ERROR)
                    
        except Exception as e:
            error_log = f"任務 {task_id} 執行失敗: {e}"
            self._log(error_log, ERROR)
            task_logs.append(error_log)
            self._send_task_logs(task_id, error_log)
            success = False
            
        finally:
            # 清理容器
            if container:
                try:
                    container.remove(force=True)
                except:
                    pass
                    
            # 清理臨時目錄
            if temp_dir and exists(temp_dir):
                try:
                    rmtree(temp_dir)
                except:
                    pass
            
            # 通知任務完成
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
            
            # 釋放資源
            self._release_resources(task_id)
            self._log(f"Task {task_id} resources released, status: {'success' if success else 'failed'}")

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
        """Log only errors in English, except VPN prompt."""
        if level == ERROR or level == WARNING:
            # Only print error/warning to console, always in English
            print(f"[{level}] {message}")
        # Keep logs in memory for web UI, but only in English
        if level == INFO:
            return  # Do not store info logs
        from datetime import datetime
        with self.log_lock:
            timestamp = datetime.now().strftime('%H:%M:%S')
            level_name = getLevelName(level)
            # Only store error/warning logs, always in English
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
        
        # 獲取任務所需資源
        required_resources = {
            "cpu": request.cpu,
            "memory_gb": request.memory_gb,
            "gpu": request.gpu,
            "gpu_memory_gb": request.gpu_memory_gb
        }
        
        file_size_mb = len(task_zip) / (1024 * 1024)
        info(f"===== 收到執行任務請求 =====")
        info(f"任務ID: {task_id}")
        info(f"檔案大小: {file_size_mb:.1f}MB")
        info(f"請求資源: CPU={required_resources['cpu']}, RAM={required_resources['memory_gb']}GB, GPU={required_resources['gpu']}, GPU_MEM={required_resources['gpu_memory_gb']}GB")
        info(f"當前節點狀態: {self.worker_node.status}")
        
        # 檢查是否有足夠資源
        if not self.worker_node._check_resources_available(required_resources):
            error_msg = f"資源不足，拒絕任務 {task_id}"
            warning(error_msg)
            return nodepool_pb2.ExecuteTaskResponse(
                success=False, 
                message=error_msg
            )
        
        # 檢查Docker要求
        if not self.worker_node.docker_available and self.worker_node.trust_score <= 50:
            error_msg = f"無Docker環境且信任分數低，拒絕任務 {task_id}"
            warning(error_msg)
            return nodepool_pb2.ExecuteTaskResponse(
                success=False, 
                message="Docker unavailable and trust score too low"
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
            # 驗證ZIP檔案
            try:
                with ZipFile(BytesIO(task_zip), 'r') as zip_ref:
                    zip_ref.testzip()
                info(f"任務 {task_id} ZIP 檔案驗證成功")
            except Exception as zip_error:
                error_msg = f"任務 {task_id} ZIP 檔案損壞: {zip_error}"
                error(error_msg)
                return nodepool_pb2.ExecuteTaskResponse(
                    success=False, 
                    message=f"Invalid ZIP file: {str(zip_error)}"
                )
            
            # 分配資源
            if not self.worker_node._allocate_resources(task_id, required_resources):
                error_msg = f"資源分配失敗，拒絕任務 {task_id}"
                warning(error_msg)
                return nodepool_pb2.ExecuteTaskResponse(
                    success=False, 
                    message="Failed to allocate resources"
                )
            
            # 啟動執行線程
            execution_thread = Thread(
                target=self.worker_node._execute_task,
                args=(task_id, task_zip, required_resources),
                daemon=True,
                name=f"TaskExecution-{task_id}"
            )
            execution_thread.start()
            
            success_msg = f"任務 {task_id} 已接受並開始準備執行 (檔案大小: {file_size_mb:.1f}MB)"
            info(success_msg)
            
            return nodepool_pb2.ExecuteTaskResponse(
                success=True, 
                message=success_msg
            )
            
        except Exception as e:
            # 如果出錯，釋放資源
            self.worker_node._release_resources(task_id)
            error_msg = f"接受任務 {task_id} 時發生錯誤: {e}"
            error(error_msg)
            return nodepool_pb2.ExecuteTaskResponse(
                success=False, 
                message=f"Failed to accept task: {str(e)}"
            )

    def ReportOutput(self, request, context):
        """報告任務輸出"""
        node_id = request.node_id
        task_id = request.task_id
        output = request.output
        
        if node_id != self.worker_node.node_id:
            return nodepool_pb2.StatusResponse(
                success=False,
                message=f"Node ID mismatch: {node_id} != {self.worker_node.node_id}"
            )
        
        try:
            # 檢查任務是否存在
            with self.worker_node.resources_lock:
                if task_id not in self.worker_node.running_tasks:
                    return nodepool_pb2.StatusResponse(
                        success=False,
                        message=f"Task {task_id} not found"
                    )
            
            # 發送輸出到主節點
            self.worker_node._send_task_logs(task_id, output)
            
            return nodepool_pb2.StatusResponse(
                success=True,
                message=f"Output reported for task {task_id}"
            )
        except Exception as e:
            return nodepool_pb2.StatusResponse(
                success=False,
                message=f"Failed to report output: {str(e)}"
            )

    def ReportRunningStatus(self, request, context):
        """報告運行狀態"""
        task_id = request.task_id
        
        # 檢查任務是否存在
        with self.worker_node.resources_lock:
            if task_id not in self.worker_node.running_tasks:
                return nodepool_pb2.RunningStatusResponse(
                    success=False,
                    message=f"Not running task {task_id}"
                )
        
        # 獲取真實的系統資源使用情況
        try:
            cpu_usage = cpu_percent(interval=0.1)
            memory_info = virtual_memory()
            memory_usage = int((memory_info.total - memory_info.available) / (1024 * 1024))
            
            # GPU 使用情況（需要專門的庫來獲取，暫時設為 0）
            gpu_usage = 0
            gpu_memory_usage = 0
        except Exception as e:
            self.worker_node._log(f"獲取系統資源使用失敗: {e}", WARNING)
            # 使用請求中的值作為備用
            cpu_usage = request.cpu_usage
            memory_usage = request.memory_usage
            gpu_usage = request.gpu_usage
            gpu_memory_usage = request.gpu_memory_usage
        
        # 發送狀態報告
        try:
            self.worker_node._log(f"Reporting real status for task {task_id}: CPU={cpu_usage:.1f}%, MEM={memory_usage}MB, GPU={gpu_usage}%, GPU_MEM={gpu_memory_usage}MB")
            
            # 根據真實資源使用情況計算 CPT 獎勵
            # 基礎獎勵根據資源使用情況動態計算
            base_reward = 0.1  # 基礎獎勵
            cpu_reward = (cpu_usage / 100) * 0.5  # CPU 使用率獎勵
            memory_reward = (memory_usage / 1024) * 0.1  # 每 GB 內存使用獎勵 0.1 CPT
            
            cpt_reward = base_reward + cpu_reward + memory_reward
            
            return nodepool_pb2.RunningStatusResponse(
                success=True,
                message=f"Task {task_id} running - CPU: {cpu_usage:.1f}%, Memory: {memory_usage}MB",
                cpt_reward=cpt_reward
            )
        except Exception as e:
            self.worker_node._log(f"Failed to report status: {e}", WARNING)
            return nodepool_pb2.RunningStatusResponse(
                success=False,
                message=f"Failed to report status: {str(e)}"
            )

    def StopTaskExecution(self, request, context):
        """立即強制停止任務執行並打包結果"""
        task_id = request.task_id
        
        # 檢查任務是否存在
        with self.worker_node.resources_lock:
            if task_id not in self.worker_node.running_tasks:
                return nodepool_pb2.StopTaskExecutionResponse(
                    success=False,
                    message=f"Task {task_id} not running"
                )
        
        info(f"收到停止任務 {task_id} 的請求，立即執行強制停止")
        
        # 發送停止信號
        success = self.worker_node._stop_task(task_id)
        
        if success:
            return nodepool_pb2.StopTaskExecutionResponse(
                success=True,
                message=f"Stop signal sent to task {task_id}"
            )
        else:
            return nodepool_pb2.StopTaskExecutionResponse(
                success=False,
                message=f"Failed to send stop signal to task {task_id}"
            )
def run_worker_node():
    """啟動 Worker Node"""
    try:
        worker = WorkerNode()
        
        # 啟動 gRPC 服務
        server = grpc.server(ThreadPoolExecutor(max_workers=10))  # 增加工作線程數
        nodepool_pb2_grpc.add_WorkerNodeServiceServicer_to_server(
            WorkerNodeServicer(worker), server
        )
        
        server.add_insecure_port(f'[::]:{NODE_PORT}')
        server.start()
        
        worker._log(f"Worker Node started on port {NODE_PORT}")
        worker._log(f"Flask UI: http://localhost:{FLASK_PORT}")
        worker._log(f"Docker status: {worker.docker_status}")
        worker._log(f"Available resources: CPU={worker.available_resources['cpu']}, Memory={worker.available_resources['memory_gb']}GB, GPU={worker.available_resources['gpu']}")
        
        # 保持運行
        try:
            while True:
                sleep(60)
        except KeyboardInterrupt:
            worker._log("Shutting down...")
            worker._stop_status_reporting()
            worker._stop_all_tasks()
            server.stop(grace=5)
            
    except Exception as e:
        critical(f"Failed to start worker: {e}")
        exit(1)
if __name__ == '__main__':
    run_worker_node()