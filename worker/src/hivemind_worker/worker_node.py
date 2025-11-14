from os.path import join, dirname, abspath, exists
from os import makedirs
import sys
import os
sys.path.insert(0, os.path.abspath(os.path.dirname(__file__)))

# 修復模塊導入 - 支持直接運行和模塊導入
try:
    # 嘗試相對導入 (作為包運行時)
    from .config import NODE_PORT, FLASK_PORT, MASTER_ADDRESS, NODE_ID
    from . import nodepool_pb2
    from . import nodepool_pb2_grpc
except ImportError:
    # 直接導入 (直接運行時)
    from config import NODE_PORT, FLASK_PORT, MASTER_ADDRESS, NODE_ID
    import nodepool_pb2
    import nodepool_pb2_grpc

# System metrics moved to dedicated module
try:
    from .system_metrics import (
        VirtualMemory,
        cpu_count,
        virtual_memory,
        cpu_percent,
        get_hostname,
        get_cpu_score,
        get_gpu_info,
    )
except ImportError:
    from system_metrics import (
        VirtualMemory,
        cpu_count,
        virtual_memory,
        cpu_percent,
        get_hostname,
        get_cpu_score,
        get_gpu_info,
    )

import time
from zipfile import ZipFile, ZIP_DEFLATED
from io import BytesIO
from subprocess import run
from platform import node, system
# 只在 Windows 匯入 CREATE_NO_WINDOW
try:
    from subprocess import CREATE_NO_WINDOW
except ImportError:
    CREATE_NO_WINDOW = None
from datetime import datetime, timedelta
from secrets import token_hex
from shutil import rmtree
from uuid import uuid4
import grpc
from concurrent.futures import ThreadPoolExecutor
from logging import basicConfig, INFO, ERROR, WARNING, DEBUG, getLevelName

basicConfig(level=INFO, format='%(asctime)s - %(levelname)s - [%(threadName)s] - %(message)s')

# 使用 logging 模組提供的常量與 _log 方法，不再保留全域別名與包裝函式

from threading import Event, Lock, Thread
from flask import Flask

# Import gRPC servicer from separate module
try:
    from .grpc_servicer import WorkerNodeServicer
except ImportError:
    from grpc_servicer import WorkerNodeServicer

# Import Flask web setup from separate module
try:
    from .webapp import register_routes as register_web_routes, start_flask as start_web_flask, start_webview as start_webview_ui
except ImportError:
    from webapp import register_routes as register_web_routes, start_flask as start_web_flask, start_webview as start_webview_ui

class WorkerNode:
    def __init__(self):
        self.node_id = NODE_ID
        self.port = NODE_PORT
        self.master_address = MASTER_ADDRESS
        self.flask_port = FLASK_PORT

        # 狀態管理
        self.status = "Initializing"
        # 改為字典以支持多任務
        self.running_tasks = {}  # {task_id: {"status": status, "resources": {}, "start_time": time.time()}}
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
        # 供外部模組使用的系統度量函式引用
        self.cpu_percent = cpu_percent
        self.virtual_memory = virtual_memory

        # 效能上限設定（可由 Web 設定頁調整）
        self.performance_limits = {
            'cpu_percent': 100,          # 0-100
            'memory_percent': 100,       # 0-100
            'gpu_percent': 100,          # 0-100
            'gpu_memory_percent': 100,   # 0-100
            'disk_percent': 100,         # 0-100（目前僅作展示，不參與註冊）
            'network_mbps': 1000,        # 0-1000（目前僅作展示，不參與註冊）
        }
        # 對外廣告資源（註冊到 node_pool 使用）
        self.advertised_cpu_score = 0
        self.advertised_memory_gb = 0
        self.advertised_gpu_score = 0
        self.advertised_gpu_memory_gb = 0

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

    # VPN 自動連線已移除，若需要可於未來以選配功能實作

        # 硬體信息
        self._init_hardware()
        # Docker 初始化
        self._init_docker()
        # gRPC 連接
        self._init_grpc()  # 確保 gRPC stub 初始化在 Flask 之前
        # Flask 應用
        self._init_flask()
        self.status = "Waiting for Login"

        # 啟動自動服務：VPN + 更新（非阻塞）
        try:
            self._init_auto_services()
        except Exception as e:
            self._log(f"Auto services init failed: {e}", WARNING)

    # 移除未使用的 WireGuard 自動連線/提示函式，避免多餘相依與混淆

    def _init_auto_services(self):
        """啟動 VPN 連線與自動更新執行緒。若 VPN 已連線則跳過。"""
        from .vpn_client import connect_vpn_once, default_config_path
        from .auto_update import start_update_thread
        # VPN 嘗試
        cfg_path = default_config_path()
        ok, msg = connect_vpn_once(cfg_path=cfg_path)
        if ok:
            self._log(f"VPN ready: {msg}")
        else:
            self._log(f"VPN not connected: {msg}", WARNING)
        # 啟動更新執行緒
        if not hasattr(self, '_stop_event'):
            from threading import Event
            self._stop_event = Event()
        self.update_thread = start_update_thread(self._stop_event)
        self._log("Auto-update thread started")

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

            # 依照效能上限設定計算對外廣告資源
            self._apply_performance_limits()
            
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
            # 依照效能上限設定計算對外廣告資源（退化情況）
            self._apply_performance_limits()

    def _apply_performance_limits(self):
        """依照 performance_limits 套用上限，計算註冊用的廣告資源，並調整可用資源上限。"""
        try:
            cpu_pct = max(0, min(100, int(self.performance_limits.get('cpu_percent', 100))))
            mem_pct = max(0, min(100, int(self.performance_limits.get('memory_percent', 100))))
            gpu_pct = max(0, min(100, int(self.performance_limits.get('gpu_percent', 100))))
            gpum_pct = max(0, min(100, int(self.performance_limits.get('gpu_memory_percent', 100))))

            self.advertised_cpu_score = int(self.cpu_score * cpu_pct / 100)
            # 記憶體避免為 0，至少 1 GB 以免註冊異常
            self.advertised_memory_gb = max(1, int(self.memory_gb * mem_pct / 100))
            self.advertised_gpu_score = int(self.gpu_score * gpu_pct / 100)
            self.advertised_gpu_memory_gb = int(self.gpu_memory_gb * gpum_pct / 100)

            # 也同步限制在本地資源模型上（若無任務，重置可用資源）
            with self.resources_lock:
                self.total_resources.update({
                    'cpu': self.advertised_cpu_score,
                    'memory_gb': self.advertised_memory_gb,
                    'gpu': self.advertised_gpu_score,
                    'gpu_memory_gb': self.advertised_gpu_memory_gb,
                })
                if not self.running_tasks:
                    self.available_resources = self.total_resources.copy()
        except Exception as e:
            self._log(f"Apply performance limits failed: {e}", WARNING)

    def _auto_detect_location(self):
        """靜默自動檢測地區（委派到 network_utils.auto_detect_location）"""
        try:
            try:
                from .network_utils import auto_detect_location
            except ImportError:
                from network_utils import auto_detect_location
            return auto_detect_location(self)
        except Exception as e:
            self._log(f"Location detection error: {e}")
            return "Unknown"

    def _get_local_ip(self):
        """獲取本機 IP（委派到 network_utils.get_local_ip）"""
        try:
            try:
                from .network_utils import get_local_ip
            except ImportError:
                from network_utils import get_local_ip
            return get_local_ip(self)
        except Exception:
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
        """初始化 Docker（委派到 docker_utils）"""
        try:
            try:
                from .docker_utils import init_docker
            except ImportError:
                from docker_utils import init_docker
            init_docker(self)
        except Exception as e:
            self._log(f"Docker initialization failed: {e}", WARNING)
            self.docker_available = False
            self.docker_client = None
            self.docker_status = "unavailable"

    def _init_grpc(self):
        """初始化 gRPC 連接（委派到 grpc_client）"""
        try:
            try:
                from .grpc_client import init_grpc
            except ImportError:
                from grpc_client import init_grpc
            init_grpc(self)
        except Exception as e:
            self._log(f"gRPC connection failed: {e}", ERROR)
            self.channel = None
            self.user_stub = None
            self.node_stub = None
            self.master_stub = None

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
        # 使用外部模組註冊路由與啟動 Flask
        register_web_routes(self.app, self)
        start_web_flask(self.app, self)

    def _create_user_session(self, username, token):
        """創建用戶會話（委派到 session_manager.create_user_session）"""
        try:
            try:
                from .session_manager import create_user_session
            except ImportError:
                from session_manager import create_user_session
            return create_user_session(self, username, token)
        except Exception:
            session_id = str(uuid4())
            session_data = {
                'username': username,
                'token': token,
                'login_time': datetime.now(),
                'cpt_balance': 0,
                'created_at': time.time()
            }
            
            with self.session_lock:
                self.user_sessions[session_id] = session_data
            
            return session_id

    def _get_user_session(self, session_id):
        """根據會話ID獲取用戶資料（委派到 session_manager.get_user_session）"""
        try:
            try:
                from .session_manager import get_user_session
            except ImportError:
                from session_manager import get_user_session
            return get_user_session(self, session_id)
        except Exception:
            with self.session_lock:
                return self.user_sessions.get(session_id)

    def _update_session_balance(self, session_id, balance):
        """更新會話中的餘額（委派到 session_manager.update_session_balance）"""
        try:
            try:
                from .session_manager import update_session_balance
            except ImportError:
                from session_manager import update_session_balance
            return update_session_balance(self, session_id, balance)
        except Exception:
            with self.session_lock:
                if session_id in self.user_sessions:
                    self.user_sessions[session_id]['cpt_balance'] = balance

    def _clear_user_session(self, session_id):
        """清除用戶會話（委派到 session_manager.clear_user_session）"""
        try:
            try:
                from .session_manager import clear_user_session
            except ImportError:
                from session_manager import clear_user_session
            return clear_user_session(self, session_id)
        except Exception:
            with self.session_lock:
                if session_id in self.user_sessions:
                    del self.user_sessions[session_id]


    def _login(self, username, password):
        """登入到節點池"""
        try:
            # 檢查 gRPC 連接是否可用
            if not self.user_stub:
                # 嘗試即時重新建立 gRPC 連線
                self._log("gRPC not ready, trying to reconnect before login...", WARNING)
                try:
                    self._init_grpc()
                except Exception:
                    pass
                if not self.user_stub:
                    self._log("gRPC connection not available, cannot login", ERROR)
                    self.status = "Login Failed - No Connection"
                    return False
                
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
            self._log("Registration failed: No token available", ERROR)
            return False
            
        # 檢查 gRPC 連接是否可用
        if not self.node_stub:
            # 嘗試即時重新建立 gRPC 連線
            self._log("gRPC not ready, trying to reconnect before registration...", WARNING)
            try:
                self._init_grpc()
            except Exception:
                pass
            if not self.node_stub:
                self._log("Registration failed: gRPC connection not available", ERROR)
                self.status = "Registration Failed - No Connection"
                return False

        try:
            # 第一次註冊使用「全部效能」上報（總容量），直接回報硬體總量而非限額
            self._log(f"Attempting to register node with master at {self.master_address}")
            self._log(
                f"Registration details - totals: CPUscore={int(self.cpu_score)}, RAM={int(self.total_memory_gb)}GB, "
                f"GPUscore={int(self.gpu_score)}, GPUMEM={int(self.gpu_memory_gb)}GB; IP: {self.local_ip}, Port: {self.port}, User: {self.username}"
            )

            # 建立註冊請求（使用總容量值）
            request = nodepool_pb2.RegisterWorkerNodeRequest(
                node_id=self.username,
                hostname=self.local_ip,  # 使用本機 IP 而不是 127.0.0.1
                cpu_cores=int(self.cpu_cores),
                memory_gb=int(self.total_memory_gb),
                cpu_score=int(self.cpu_score),
                gpu_score=int(self.gpu_score),
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
            
            self._log(f"Registration response received - Success: {response.success}, Message: {response.message}")
            
            if response.success:
                self.node_id = self.username
                self.is_registered = True
                self.status = "Idle"
                # 註冊之後才套用使用者配置的效能上限，避免覆蓋首次上報的總容量
                self._apply_performance_limits()
                self._start_status_reporting()
                self._log(f"節點註冊成功，使用 IP: {self.local_ip}:{self.port}")
                return True
            else:
                self.status = "Registration Failed: Server returned false"
                self._log(f"Registration failed: Server returned success=false, message: {response.message}", ERROR)
                return False
                
        except Exception as e:
            error_msg = str(e)
            self._log(f"Registration exception: {error_msg}", ERROR)
            self.status = f"Registration Failed: {error_msg}"
            return False

    def _refresh_registration(self):
        """刷新註冊以維持心跳，不會改變狀態只是更新心跳時間"""
        if not self.is_registered or not self.node_stub:
            return False
            
        try:
            # 心跳刷新不改變總容量，維持首次註冊的總量值
            request = nodepool_pb2.RegisterWorkerNodeRequest(
                node_id=self.username,
                hostname=self.local_ip,
                cpu_cores=int(self.cpu_cores),
                memory_gb=int(self.total_memory_gb),
                cpu_score=int(self.cpu_score),
                gpu_score=int(self.gpu_score),
                gpu_name=self.gpu_name,
                gpu_memory_gb=int(self.gpu_memory_gb),
                location=self.location,
                port=self.port,
                docker_status=self.docker_status
            )
            
            response = self.node_stub.RegisterWorkerNode(request, timeout=5)
            return response.success
            
        except Exception as e:
            self._log(f"Heartbeat refresh failed: {e}", WARNING)
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
        start_time = time.time()
        while self.running_tasks and time.time() - start_time < timeout:
            time.sleep(0.5)
        
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
        """開始狀態報告（委派到 heartbeat.run_status_reporter）"""
        if self.status_thread and self.status_thread.is_alive():
            return
        
        self._stop_event.clear()
        try:
            try:
                from .heartbeat import run_status_reporter
            except ImportError:
                from heartbeat import run_status_reporter
            self.status_thread = Thread(target=lambda: run_status_reporter(self, self._stop_event), daemon=True)
        except Exception:
            # 後備：仍使用本地方法
            self.status_thread = Thread(target=self._status_reporter, daemon=True)
        self.status_thread.start()

    def _stop_status_reporting(self):
        """停止狀態報告"""
        self._stop_event.set()
        if self.status_thread and self.status_thread.is_alive():
            self.status_thread.join(timeout=5)

    def _status_reporter(self):
        """狀態報告線程（保留作為後備，預設委派到 heartbeat 模組）"""
        try:
            try:
                from .heartbeat import run_status_reporter
            except ImportError:
                from heartbeat import run_status_reporter
            return run_status_reporter(self, self._stop_event)
        except Exception:
            # 簡化後備實作
            while not self._stop_event.is_set():
                if self.is_registered and self.token:
                    try:
                        with self.resources_lock:
                            task_count = len(self.running_tasks)
                        status_msg = f"Running {task_count} tasks" if task_count > 0 else self.status
                        if not hasattr(self, '_last_reported_status') or self._last_reported_status != status_msg:
                            self._log(f"Local status: {status_msg}")
                            self._last_reported_status = status_msg
                        current_time = time.time()
                        if not hasattr(self, '_last_heartbeat_time') or current_time - self._last_heartbeat_time >= 30:
                            try:
                                self._refresh_registration()
                                self._last_heartbeat_time = current_time
                            except Exception as e:
                                self._log(f"Heartbeat registration failed: {e}", WARNING)
                        self._update_balance()
                        self._report_all_tasks_resource_usage()
                    except Exception as e:
                        self._log(f"Status monitoring failed: {e}", WARNING)
                self._stop_event.wait(5)

    def _report_all_tasks_resource_usage(self):
        """回報所有任務的資源使用情況（委派到 resource_monitor）"""
        try:
            try:
                from .resource_monitor import report_all_tasks_resource_usage
            except ImportError:
                from resource_monitor import report_all_tasks_resource_usage
            return report_all_tasks_resource_usage(self)
        except Exception as e:
            self._log(f"回報任務資源使用發生錯誤: {e}", WARNING)

    def _report_task_resource_usage(self, task_id):
        """回報單個任務的資源使用情況（委派到 resource_monitor）"""
        try:
            try:
                from .resource_monitor import report_task_resource_usage
            except ImportError:
                from resource_monitor import report_task_resource_usage
            return report_task_resource_usage(self, task_id)
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
        """發送任務日誌到節點池（WorkerNodeService.TaskOutputUpload）"""
        if not task_id or not logs_content:
            return False

        # 盡量使用 worker_stub（直連節點池 50051），避免依賴 Master 端 RPC
        try:
            if getattr(self, 'worker_stub', None):
                try:
                    # 延遲導入 pb 以避免循環引用
                    try:
                        from . import nodepool_pb2 as pb2
                    except Exception:
                        import nodepool_pb2 as pb2

                    req = pb2.TaskOutputUploadRequest(
                        task_id=task_id,
                        output=logs_content,
                        token=self.token or ""
                    )
                    metadata = [('authorization', f'Bearer {self.token}')] if self.token else None
                    if metadata:
                        self.worker_stub.TaskOutputUpload(req, metadata=metadata, timeout=10)
                    else:
                        self.worker_stub.TaskOutputUpload(req, timeout=10)
                    return True
                except Exception as e:
                    # 回退為本地記錄，避免打斷任務
                    self._log(f"Task {task_id} logs upload failed: {e}", WARNING)
                    self._log(f"Task {task_id} logs (local fallback): {logs_content[:100]}...")
                    return False
            else:
                # 還沒連上 gRPC，先本地記錄
                self._log(f"worker_stub 未初始化，暫存日誌: {logs_content[:100]}...", WARNING)
                return False
        except Exception as e:
            self._log(f"Error handling logs for task {task_id}: {e}", WARNING)
            return False

    def _check_resources_available(self, required_resources):
        """檢查是否有足夠資源運行任務（委派到 resource_manager）"""
        try:
            try:
                from .resource_manager import check_resources_available
            except ImportError:
                from resource_manager import check_resources_available
            return check_resources_available(self, required_resources)
        except Exception:
            with self.resources_lock:
                for resource_type, required_amount in required_resources.items():
                    if resource_type in self.available_resources:
                        if self.available_resources[resource_type] < required_amount:
                            return False
                return True

    def _allocate_resources(self, task_id, required_resources):
        """分配資源給任務（委派到 resource_manager）"""
        try:
            try:
                from .resource_manager import allocate_resources
            except ImportError:
                from resource_manager import allocate_resources
            return allocate_resources(self, task_id, required_resources)
        except Exception:
            with self.resources_lock:
                for resource_type, required_amount in required_resources.items():
                    if resource_type in self.available_resources:
                        if self.available_resources[resource_type] < required_amount:
                            return False
                for resource_type, required_amount in required_resources.items():
                    if resource_type in self.available_resources:
                        self.available_resources[resource_type] -= required_amount
                self.running_tasks[task_id] = {
                    "status": "Allocated",
                    "resources": required_resources,
                    "start_time": time.time()
                }
                self.task_locks[task_id] = Lock()
                self.task_stop_events[task_id] = Event()
                return True

    def _release_resources(self, task_id):
        """釋放任務使用的資源（委派到 resource_manager）"""
        try:
            try:
                from .resource_manager import release_resources
            except ImportError:
                from resource_manager import release_resources
            return release_resources(self, task_id)
        except Exception:
            with self.resources_lock:
                if task_id not in self.running_tasks:
                    return
                task_resources = self.running_tasks[task_id].get('resources', {})
                for resource_type, amount in task_resources.items():
                    if resource_type in self.available_resources:
                        self.available_resources[resource_type] += amount
                del self.running_tasks[task_id]
                if task_id in self.task_locks:
                    del self.task_locks[task_id]
                if task_id in self.task_stop_events:
                    del self.task_stop_events[task_id]

    def _execute_task(self, task_id, task_zip_bytes, required_resources=None):
        # 委派到模組化執行器
        try:
            try:
                from .task_executor import execute_task
            except ImportError:
                from task_executor import execute_task
            return execute_task(self, task_id, task_zip_bytes, required_resources)
        except Exception as e:
            self._log(f"execute_task error: {e}", ERROR)
            # 確保資源釋放
            self._release_resources(task_id)
            return

    # _create_result_zip 已移至 task_executor 模組

    def _log(self, message, level=INFO):
        """Log errors and important info to console"""
        # Print to console for debugging purposes
        level_name = getLevelName(level)
        print(f"[{level_name}] {message}")
        
        # Keep logs in memory for web UI
        from datetime import datetime
        with self.log_lock:
            timestamp = datetime.now().strftime('%H:%M:%S')
            # Store all logs for web UI
            self.logs.append(f"{timestamp} - {level_name} - {message}")
            if len(self.logs) > 500:
                self.logs.pop(0)

    # _find_executable_script 已移至 task_executor 模組

# gRPC 服務實現
 


def run_worker_node():
    """啟動 Worker 節點"""
    try:
        print("=== HiveMind Worker Node Starting ===")
        print(f"Node Port: {NODE_PORT}")
        print(f"Flask Port: {FLASK_PORT}")
        print(f"Master Address: {MASTER_ADDRESS}")
        
        # 創建 Worker 節點實例
        worker = WorkerNode()
        
        # 啟動 gRPC 服務器
        server = grpc.server(ThreadPoolExecutor(max_workers=10))
        nodepool_pb2_grpc.add_WorkerNodeServiceServicer_to_server(
            WorkerNodeServicer(worker), server
        )
        server.add_insecure_port(f'[::]:{NODE_PORT}')
        server.start()
        
        print(f"Worker node gRPC server started on port {NODE_PORT}")
        print(f"Worker node web interface available at http://127.0.0.1:{FLASK_PORT}")

        # 在主線程啟動 WebView（若可用），否則維持舊有阻塞迴圈
        try:
            start_webview_ui(worker)
            # webview 視窗關閉後到此，關閉 gRPC 服務
            print("\n=== WebView closed, shutting down Worker Node ===")
            server.stop(5)
            print("Worker Node stopped")
        except Exception as e:
            print(f"WebView unavailable or failed: {e}")
            print("Falling back to headless mode (no embedded UI). Press Ctrl+C to exit.")
            try:
                while True:
                    time.sleep(1)
            except KeyboardInterrupt:
                print("\n=== Shutting down Worker Node ===")
                server.stop(5)
                print("Worker Node stopped")
            
    except Exception as e:
        print(f"Failed to start Worker Node: {e}")
        raise


if __name__ == "__main__":
    run_worker_node()