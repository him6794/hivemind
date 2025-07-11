from kivy.app import App
from kivy.uix.boxlayout import BoxLayout
from kivy.uix.label import Label
from kivy.uix.textinput import TextInput
from kivy.uix.button import Button
from kivy.uix.scrollview import ScrollView
from kivy.uix.spinner import Spinner
from kivy.clock import Clock
from kivy.logger import Logger
import grpc
from threading import Thread, Event, Lock
from time import time, sleep
from datetime import datetime, timedelta
from platform import node
from uuid import uuid4
from requests import post, get, exceptions
import nodepool_pb2
import nodepool_pb2_grpc
from concurrent.futures import ThreadPoolExecutor
from os import environ

class AndroidWorkerApp(App):
    def __init__(self, **kwargs):
        super().__init__(**kwargs)
        self.node_port = int(environ.get("NODE_PORT", 50053))
        self.master_address = environ.get("MASTER_ADDRESS", "10.0.0.1:50051")
        self.node_id = environ.get("NODE_ID", f"android-worker-{node().split('.')[0]}")
        
        # 狀態管理
        self.status = "Initializing"
        self.current_task_id = None
        self.username = None
        self.token = None
        self.is_registered = False
        self.login_time = None
        self.cpt_balance = 0
        self.location = "Unknown"
        
        # 線程控制
        self.status_thread = None
        self._stop_event = Event()
        self.logs = []
        self.log_lock = Lock()
        
        # 硬體信息
        self.hostname = node()
        self.cpu_cores = 4  # Android 設備默認值
        self.memory_gb = 2.0  # Android 設備默認值
        self.local_ip = "127.0.0.1"
        self.cpu_score = 1000
        self.gpu_score = 500
        self.gpu_name = "Android GPU"
        self.gpu_memory_gb = 1.0
        
        # 初始化 gRPC
        self._init_grpc()
        self.status = "Waiting for Login"

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
            self._log(f"gRPC connection failed: {e}")

    def build(self):
        """構建 Kivy UI"""
        main_layout = BoxLayout(orientation='vertical', padding=10, spacing=10)
        
        # 標題
        title_label = Label(text='HiveMind Android Worker', 
                          size_hint=(1, 0.1), 
                          font_size='20sp')
        main_layout.add_widget(title_label)
        
        # 狀態顯示
        self.status_label = Label(text=f'Status: {self.status}',
                                size_hint=(1, 0.1),
                                font_size='16sp')
        main_layout.add_widget(self.status_label)
        
        # 節點信息
        node_info = Label(text=f'Node ID: {self.node_id}\nMaster: {self.master_address}',
                         size_hint=(1, 0.1),
                         font_size='14sp')
        main_layout.add_widget(node_info)
        
        # 登入區域
        login_layout = BoxLayout(orientation='horizontal', size_hint=(1, 0.1))
        
        self.username_input = TextInput(hint_text='Username', multiline=False)
        self.password_input = TextInput(hint_text='Password', password=True, multiline=False)
        login_button = Button(text='Login', size_hint=(0.3, 1))
        login_button.bind(on_press=self.login)
        
        login_layout.add_widget(self.username_input)
        login_layout.add_widget(self.password_input)
        login_layout.add_widget(login_button)
        main_layout.add_widget(login_layout)
        
        # 地區選擇
        location_layout = BoxLayout(orientation='horizontal', size_hint=(1, 0.1))
        location_label = Label(text='Location:', size_hint=(0.3, 1))
        self.location_spinner = Spinner(
            text=self.location,
            values=['亞洲', '非洲', '北美', '南美', '歐洲', '大洋洲', 'Unknown'],
            size_hint=(0.7, 1)
        )
        self.location_spinner.bind(text=self.update_location)
        location_layout.add_widget(location_label)
        location_layout.add_widget(self.location_spinner)
        main_layout.add_widget(location_layout)
        
        # 餘額顯示
        self.balance_label = Label(text=f'CPT Balance: {self.cpt_balance}',
                                 size_hint=(1, 0.1),
                                 font_size='16sp')
        main_layout.add_widget(self.balance_label)
        
        # 登出按鈕
        logout_button = Button(text='Logout', size_hint=(1, 0.1))
        logout_button.bind(on_press=self.logout)
        main_layout.add_widget(logout_button)
        
        # 日誌顯示
        scroll = ScrollView(size_hint=(1, 0.4))
        self.log_label = Label(text='Logs will appear here...',
                             text_size=(None, None),
                             valign='top',
                             font_size='12sp')
        scroll.add_widget(self.log_label)
        main_layout.add_widget(scroll)
        
        # 定時更新 UI
        Clock.schedule_interval(self.update_ui, 1.0)
        
        return main_layout

    def login(self, instance):
        """登入處理"""
        username = self.username_input.text.strip()
        password = self.password_input.text.strip()
        
        if not username or not password:
            self._log("Please enter username and password")
            return
        
        if self._login(username, password) and self._register():
            self._log(f"User '{username}' logged in successfully")
            self.username_input.text = ""
            self.password_input.text = ""
        else:
            self._log(f"Login failed: {self.status}")

    def update_location(self, spinner, text):
        """更新地區設定"""
        self.location = text
        self._log(f"Location updated to: {text}")
        if self.is_registered and self.token:
            self._register()

    def logout(self, instance):
        """登出處理"""
        self._logout()

    def update_ui(self, dt):
        """更新 UI 顯示"""
        self.status_label.text = f'Status: {self.status}'
        self.balance_label.text = f'CPT Balance: {self.cpt_balance}'
        self.location_spinner.text = self.location
        
        # 更新日誌顯示
        with self.log_lock:
            log_text = '\n'.join(self.logs[-20:])  # 只顯示最後20行
            self.log_label.text = log_text
            self.log_label.text_size = (self.log_label.parent.width, None)

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
            self._log(f"Login error: {e}")
            self.status = "Login Failed"
            return False

    def _register(self):
        """註冊節點"""
        if not self.token:
            return False

        try:
            request = nodepool_pb2.RegisterWorkerNodeRequest(
                node_id=self.username,
                hostname=self.local_ip,
                cpu_cores=int(self.cpu_cores),
                memory_gb=int(self.memory_gb),
                cpu_score=self.cpu_score,
                gpu_score=self.gpu_score,
                gpu_name=self.gpu_name,
                gpu_memory_gb=int(self.gpu_memory_gb),
                location=self.location,
                port=self.node_port
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
                self._log(f"Node registered successfully")
                return True
            else:
                self.status = f"Registration Failed: {response.message}"
                return False
                
        except Exception as e:
            self._log(f"Registration error: {e}")
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
                    
                except Exception as e:
                    self._log(f"Status report failed: {e}")
            
            self._stop_event.wait(5)

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
        except:
            pass

    def _log(self, message):
        """記錄日誌"""
        Logger.info(f"AndroidWorker: {message}")
        
        with self.log_lock:
            timestamp = datetime.now().strftime('%H:%M:%S')
            self.logs.append(f"{timestamp} - {message}")
            if len(self.logs) > 100:
                self.logs.pop(0)

    def on_stop(self):
        """應用程式停止時的清理"""
        self._logout()
        if hasattr(self, 'channel'):
            self.channel.close()
