# Web 服務模組文檔

## 📋 概述

Web 服務模組是 HiveMind 分散式計算平台的前端界面層，提供完整的 Web 管理界面、VPN 服務管理和 WireGuard 伺服器整合。基於 Flask 框架構建，為用戶提供直觀的圖形化管理介面。

## 🏗️ 系統架構

```
┌─────────────────────┐
│    Web 服務模組      │
├─────────────────────┤
│ • Flask 應用程式     │
│ • VPN 服務管理       │
│ • WireGuard 伺服器   │
│ • Web 界面           │
│ • 靜態資源管理       │
└─────────────────────┘
        │
        ├─ HTTP/HTTPS 協議
        ├─ VPN 網路管理
        ├─ 用戶認證服務
        └─ 系統監控界面
```

## 🔧 核心組件

### 1. Flask 應用程式 (`app.py`)
- **功能**: Web 應用程式主體
- **狀態**: 核心功能完整
- **目的**: 提供 RESTful API 和 Web 界面

**主要功能**:
```python
from flask import Flask, render_template, request, jsonify, session
from werkzeug.security import generate_password_hash, check_password_hash
import logging
import os

app = Flask(__name__)
app.secret_key = os.environ.get('SECRET_KEY', 'hivemind-secret-key')

# 配置日誌
logging.basicConfig(level=logging.INFO)
logger = logging.getLogger(__name__)

class HiveMindWebApp:
    def __init__(self):
        self.app = app
        self.setup_routes()
        self.setup_error_handlers()
        
    def setup_routes(self):
        """設置路由"""
        
        @self.app.route('/')
        def index():
            """首頁"""
            return render_template('index.html')
        
        @self.app.route('/dashboard')
        def dashboard():
            """管理儀表板"""
            if 'user_id' not in session:
                return redirect('/login')
            
            # 獲取系統狀態
            system_status = self.get_system_status()
            node_stats = self.get_node_statistics()
            
            return render_template('dashboard.html', 
                                 system_status=system_status,
                                 node_stats=node_stats)
        
        @self.app.route('/nodes')
        def nodes():
            """節點管理"""
            if 'user_id' not in session:
                return redirect('/login')
            
            # 獲取所有節點信息
            nodes = self.get_all_nodes()
            
            return render_template('nodes.html', nodes=nodes)
        
        @self.app.route('/tasks')
        def tasks():
            """任務管理"""
            if 'user_id' not in session:
                return redirect('/login')
            
            # 獲取任務列表
            tasks = self.get_task_list()
            
            return render_template('tasks.html', tasks=tasks)
        
        @self.app.route('/vpn')
        def vpn_management():
            """VPN 管理界面"""
            if 'user_id' not in session:
                return redirect('/login')
            
            # 獲取 VPN 狀態
            vpn_status = self.get_vpn_status()
            clients = self.get_vpn_clients()
            
            return render_template('vpn.html', 
                                 vpn_status=vpn_status,
                                 clients=clients)
        
        @self.app.route('/api/system/status')
        def api_system_status():
            """系統狀態 API"""
            try:
                status = {
                    'cpu_usage': self.get_cpu_usage(),
                    'memory_usage': self.get_memory_usage(),
                    'disk_usage': self.get_disk_usage(),
                    'network_status': self.get_network_status(),
                    'active_nodes': self.get_active_node_count(),
                    'running_tasks': self.get_running_task_count(),
                    'vpn_status': self.get_vpn_service_status()
                }
                return jsonify(status)
            except Exception as e:
                logger.error(f"Error getting system status: {e}")
                return jsonify({'error': str(e)}), 500
        
        @self.app.route('/api/nodes', methods=['GET'])
        def api_get_nodes():
            """獲取節點列表 API"""
            try:
                nodes = self.get_all_nodes()
                return jsonify(nodes)
            except Exception as e:
                logger.error(f"Error getting nodes: {e}")
                return jsonify({'error': str(e)}), 500
        
        @self.app.route('/api/tasks', methods=['GET', 'POST'])
        def api_tasks():
            """任務管理 API"""
            if request.method == 'GET':
                try:
                    tasks = self.get_task_list()
                    return jsonify(tasks)
                except Exception as e:
                    logger.error(f"Error getting tasks: {e}")
                    return jsonify({'error': str(e)}), 500
            
            elif request.method == 'POST':
                try:
                    task_data = request.get_json()
                    task_id = self.create_task(task_data)
                    return jsonify({'task_id': task_id, 'status': 'created'})
                except Exception as e:
                    logger.error(f"Error creating task: {e}")
                    return jsonify({'error': str(e)}), 500
        
        @self.app.route('/api/vpn/clients', methods=['GET', 'POST'])
        def api_vpn_clients():
            """VPN 客戶端管理 API"""
            if request.method == 'GET':
                try:
                    clients = self.get_vpn_clients()
                    return jsonify(clients)
                except Exception as e:
                    logger.error(f"Error getting VPN clients: {e}")
                    return jsonify({'error': str(e)}), 500
            
            elif request.method == 'POST':
                try:
                    client_data = request.get_json()
                    client_config = self.create_vpn_client(client_data)
                    return jsonify(client_config)
                except Exception as e:
                    logger.error(f"Error creating VPN client: {e}")
                    return jsonify({'error': str(e)}), 500
    
    def get_system_status(self):
        """獲取系統狀態"""
        import psutil
        
        return {
            'cpu_percent': psutil.cpu_percent(interval=1),
            'memory_percent': psutil.virtual_memory().percent,
            'disk_percent': psutil.disk_usage('/').percent,
            'boot_time': psutil.boot_time(),
            'load_average': os.getloadavg() if hasattr(os, 'getloadavg') else [0, 0, 0]
        }
    
    def get_node_statistics(self):
        """獲取節點統計信息"""
        # 這裡應該連接到 Node Pool 服務獲取實際數據
        return {
            'total_nodes': 10,
            'active_nodes': 8,
            'idle_nodes': 5,
            'busy_nodes': 3,
            'offline_nodes': 2
        }
    
    def get_all_nodes(self):
        """獲取所有節點信息"""
        # 這裡應該從 Node Pool 服務獲取節點列表
        return [
            {
                'node_id': 'node-001',
                'hostname': 'worker-001.hivemind.local',
                'ip_address': '10.0.1.101',
                'status': 'active',
                'cpu_usage': 45.2,
                'memory_usage': 67.8,
                'last_seen': '2025-08-20T10:30:00Z'
            },
            {
                'node_id': 'node-002',
                'hostname': 'worker-002.hivemind.local',
                'ip_address': '10.0.1.102',
                'status': 'busy',
                'cpu_usage': 89.5,
                'memory_usage': 78.3,
                'last_seen': '2025-08-20T10:29:45Z'
            }
        ]
    
    def start_server(self, host='0.0.0.0', port=5000, debug=False):
        """啟動 Web 伺服器"""
        logger.info(f"Starting HiveMind Web Server on {host}:{port}")
        self.app.run(host=host, port=port, debug=debug)

# 創建應用程式實例
web_app = HiveMindWebApp()

if __name__ == '__main__':
    web_app.start_server(debug=True)
```

### 2. VPN 服務管理 (`vpn_service.py`)
- **功能**: VPN 網路管理服務
- **狀態**: 基礎實現完成
- **協議**: WireGuard VPN

**實現範例**:
```python
import subprocess
import json
import os
from datetime import datetime, timedelta
import ipaddress

class VPNService:
    def __init__(self, config_path='/etc/wireguard'):
        self.config_path = config_path
        self.interface_name = 'wg0'
        self.server_port = 51820
        self.network_range = '10.0.1.0/24'
        self.allocated_ips = set()
        
    def initialize_server(self):
        """初始化 VPN 伺服器"""
        try:
            # 生成伺服器金鑰對
            private_key = self._generate_private_key()
            public_key = self._generate_public_key(private_key)
            
            # 創建伺服器配置
            server_config = self._create_server_config(private_key)
            
            # 保存配置文件
            config_file = os.path.join(self.config_path, f'{self.interface_name}.conf')
            with open(config_file, 'w') as f:
                f.write(server_config)
            
            # 啟動 WireGuard 介面
            self._start_wireguard_interface()
            
            return {
                'status': 'success',
                'public_key': public_key,
                'endpoint': f'{self._get_server_ip()}:{self.server_port}',
                'network': self.network_range
            }
            
        except Exception as e:
            return {'status': 'error', 'message': str(e)}
    
    def create_client_config(self, client_name, client_email=None):
        """創建客戶端配置"""
        try:
            # 生成客戶端金鑰對
            client_private_key = self._generate_private_key()
            client_public_key = self._generate_public_key(client_private_key)
            
            # 分配 IP 地址
            client_ip = self._allocate_client_ip()
            
            # 獲取伺服器公鑰
            server_public_key = self._get_server_public_key()
            
            # 創建客戶端配置
            client_config = f"""[Interface]
PrivateKey = {client_private_key}
Address = {client_ip}/32
DNS = 8.8.8.8, 8.8.4.4

[Peer]
PublicKey = {server_public_key}
Endpoint = {self._get_server_ip()}:{self.server_port}
AllowedIPs = 0.0.0.0/0
PersistentKeepalive = 25
"""
            
            # 添加客戶端到伺服器配置
            self._add_client_to_server(client_name, client_public_key, client_ip)
            
            # 保存客戶端信息
            client_info = {
                'name': client_name,
                'email': client_email,
                'public_key': client_public_key,
                'ip_address': client_ip,
                'created_at': datetime.now().isoformat(),
                'last_handshake': None,
                'bytes_sent': 0,
                'bytes_received': 0
            }
            
            self._save_client_info(client_name, client_info)
            
            return {
                'status': 'success',
                'config': client_config,
                'client_info': client_info
            }
            
        except Exception as e:
            return {'status': 'error', 'message': str(e)}
    
    def get_client_status(self):
        """獲取所有客戶端狀態"""
        try:
            # 獲取 WireGuard 狀態
            result = subprocess.run(['wg', 'show', self.interface_name], 
                                  capture_output=True, text=True)
            
            if result.returncode != 0:
                return {'status': 'error', 'message': 'Failed to get WireGuard status'}
            
            # 解析狀態信息
            clients = self._parse_wireguard_status(result.stdout)
            
            return {'status': 'success', 'clients': clients}
            
        except Exception as e:
            return {'status': 'error', 'message': str(e)}
    
    def revoke_client(self, client_name):
        """撤銷客戶端訪問"""
        try:
            # 從伺服器配置移除客戶端
            self._remove_client_from_server(client_name)
            
            # 釋放 IP 地址
            client_info = self._load_client_info(client_name)
            if client_info:
                self._release_client_ip(client_info['ip_address'])
            
            # 刪除客戶端信息
            self._delete_client_info(client_name)
            
            # 重新加載 WireGuard 配置
            self._reload_wireguard_config()
            
            return {'status': 'success', 'message': f'Client {client_name} revoked'}
            
        except Exception as e:
            return {'status': 'error', 'message': str(e)}
    
    def _generate_private_key(self):
        """生成私鑰"""
        result = subprocess.run(['wg', 'genkey'], capture_output=True, text=True)
        return result.stdout.strip()
    
    def _generate_public_key(self, private_key):
        """生成公鑰"""
        process = subprocess.Popen(['wg', 'pubkey'], 
                                 stdin=subprocess.PIPE, 
                                 stdout=subprocess.PIPE, 
                                 text=True)
        public_key, _ = process.communicate(input=private_key)
        return public_key.strip()
    
    def _allocate_client_ip(self):
        """分配客戶端 IP 地址"""
        network = ipaddress.IPv4Network(self.network_range)
        
        # 跳過網路地址、廣播地址和伺服器地址
        for ip in network.hosts():
            if str(ip) not in self.allocated_ips and str(ip) != network.network_address + 1:
                self.allocated_ips.add(str(ip))
                return str(ip)
        
        raise Exception("No available IP addresses in the network range")
    
    def _create_server_config(self, private_key):
        """創建伺服器配置"""
        server_ip = str(ipaddress.IPv4Network(self.network_range).network_address + 1)
        
        config = f"""[Interface]
PrivateKey = {private_key}
Address = {server_ip}/24
ListenPort = {self.server_port}
PostUp = iptables -A FORWARD -i {self.interface_name} -j ACCEPT; iptables -t nat -A POSTROUTING -o eth0 -j MASQUERADE
PostDown = iptables -D FORWARD -i {self.interface_name} -j ACCEPT; iptables -t nat -D POSTROUTING -o eth0 -j MASQUERADE

"""
        return config
```

### 3. WireGuard 伺服器 (`wireguard_server.py`)
- **功能**: WireGuard VPN 伺服器管理
- **狀態**: 核心功能實現
- **用途**: 提供安全的點對點 VPN 連接

**實現範例**:
```python
import os
import subprocess
import json
import logging
from datetime import datetime
import configparser

class WireGuardServer:
    def __init__(self, config_dir='/etc/wireguard', interface='wg0'):
        self.config_dir = config_dir
        self.interface = interface
        self.config_file = os.path.join(config_dir, f'{interface}.conf')
        self.clients_dir = os.path.join(config_dir, 'clients')
        
        # 確保目錄存在
        os.makedirs(config_dir, exist_ok=True)
        os.makedirs(self.clients_dir, exist_ok=True)
        
        # 設置日誌
        self.logger = logging.getLogger(__name__)
    
    def setup_server(self, server_ip='10.0.1.1/24', port=51820):
        """設置 WireGuard 伺服器"""
        try:
            # 生成伺服器金鑰
            server_private_key = self._generate_key()
            server_public_key = self._get_public_key(server_private_key)
            
            # 創建伺服器配置
            config_content = f"""[Interface]
PrivateKey = {server_private_key}
Address = {server_ip}
ListenPort = {port}
PostUp = iptables -A FORWARD -i %i -j ACCEPT; iptables -A FORWARD -o %i -j ACCEPT; iptables -t nat -A POSTROUTING -o eth0 -j MASQUERADE
PostDown = iptables -D FORWARD -i %i -j ACCEPT; iptables -D FORWARD -o %i -j ACCEPT; iptables -t nat -D POSTROUTING -o eth0 -j MASQUERADE

"""
            
            # 保存配置文件
            with open(self.config_file, 'w') as f:
                f.write(config_content)
            
            # 設置文件權限
            os.chmod(self.config_file, 0o600)
            
            # 啟用 IP 轉發
            self._enable_ip_forwarding()
            
            # 啟動 WireGuard 介面
            self.start_interface()
            
            self.logger.info(f"WireGuard server setup completed on {self.interface}")
            
            return {
                'success': True,
                'public_key': server_public_key,
                'endpoint': f'{self._get_public_ip()}:{port}',
                'network': server_ip
            }
            
        except Exception as e:
            self.logger.error(f"Error setting up WireGuard server: {e}")
            return {'success': False, 'error': str(e)}
    
    def add_peer(self, peer_name, peer_ip, allowed_ips='10.0.1.0/24'):
        """添加對等節點"""
        try:
            # 生成對等節點金鑰
            peer_private_key = self._generate_key()
            peer_public_key = self._get_public_key(peer_private_key)
            
            # 創建對等節點配置文件
            peer_config = self._create_peer_config(
                peer_private_key, peer_ip, allowed_ips
            )
            
            # 保存對等節點配置
            peer_config_file = os.path.join(self.clients_dir, f'{peer_name}.conf')
            with open(peer_config_file, 'w') as f:
                f.write(peer_config)
            
            # 添加對等節點到伺服器配置
            self._add_peer_to_server_config(peer_name, peer_public_key, peer_ip)
            
            # 重新加載配置
            self.reload_config()
            
            self.logger.info(f"Added peer {peer_name} with IP {peer_ip}")
            
            return {
                'success': True,
                'peer_name': peer_name,
                'public_key': peer_public_key,
                'config_file': peer_config_file,
                'config_content': peer_config
            }
            
        except Exception as e:
            self.logger.error(f"Error adding peer {peer_name}: {e}")
            return {'success': False, 'error': str(e)}
    
    def remove_peer(self, peer_name):
        """移除對等節點"""
        try:
            # 從伺服器配置移除對等節點
            self._remove_peer_from_server_config(peer_name)
            
            # 刪除對等節點配置文件
            peer_config_file = os.path.join(self.clients_dir, f'{peer_name}.conf')
            if os.path.exists(peer_config_file):
                os.remove(peer_config_file)
            
            # 重新加載配置
            self.reload_config()
            
            self.logger.info(f"Removed peer {peer_name}")
            
            return {'success': True, 'message': f'Peer {peer_name} removed'}
            
        except Exception as e:
            self.logger.error(f"Error removing peer {peer_name}: {e}")
            return {'success': False, 'error': str(e)}
    
    def start_interface(self):
        """啟動 WireGuard 介面"""
        try:
            result = subprocess.run(['wg-quick', 'up', self.interface], 
                                  capture_output=True, text=True)
            
            if result.returncode == 0:
                self.logger.info(f"WireGuard interface {self.interface} started")
                return {'success': True}
            else:
                self.logger.error(f"Failed to start interface: {result.stderr}")
                return {'success': False, 'error': result.stderr}
                
        except Exception as e:
            self.logger.error(f"Error starting interface: {e}")
            return {'success': False, 'error': str(e)}
    
    def stop_interface(self):
        """停止 WireGuard 介面"""
        try:
            result = subprocess.run(['wg-quick', 'down', self.interface], 
                                  capture_output=True, text=True)
            
            if result.returncode == 0:
                self.logger.info(f"WireGuard interface {self.interface} stopped")
                return {'success': True}
            else:
                self.logger.error(f"Failed to stop interface: {result.stderr}")
                return {'success': False, 'error': result.stderr}
                
        except Exception as e:
            self.logger.error(f"Error stopping interface: {e}")
            return {'success': False, 'error': str(e)}
    
    def reload_config(self):
        """重新加載配置"""
        try:
            # 同步配置到 WireGuard
            result = subprocess.run(['wg', 'syncconf', self.interface, self.config_file], 
                                  capture_output=True, text=True)
            
            if result.returncode == 0:
                self.logger.info(f"WireGuard configuration reloaded for {self.interface}")
                return {'success': True}
            else:
                self.logger.error(f"Failed to reload config: {result.stderr}")
                return {'success': False, 'error': result.stderr}
                
        except Exception as e:
            self.logger.error(f"Error reloading config: {e}")
            return {'success': False, 'error': str(e)}
    
    def get_peer_status(self):
        """獲取對等節點狀態"""
        try:
            result = subprocess.run(['wg', 'show', self.interface], 
                                  capture_output=True, text=True)
            
            if result.returncode == 0:
                status = self._parse_wg_status(result.stdout)
                return {'success': True, 'status': status}
            else:
                return {'success': False, 'error': result.stderr}
                
        except Exception as e:
            self.logger.error(f"Error getting peer status: {e}")
            return {'success': False, 'error': str(e)}
    
    def _generate_key(self):
        """生成 WireGuard 金鑰"""
        result = subprocess.run(['wg', 'genkey'], capture_output=True, text=True)
        return result.stdout.strip()
    
    def _get_public_key(self, private_key):
        """從私鑰獲取公鑰"""
        process = subprocess.Popen(['wg', 'pubkey'], 
                                 stdin=subprocess.PIPE, 
                                 stdout=subprocess.PIPE, 
                                 text=True)
        public_key, _ = process.communicate(input=private_key)
        return public_key.strip()
    
    def _enable_ip_forwarding(self):
        """啟用 IP 轉發"""
        try:
            with open('/proc/sys/net/ipv4/ip_forward', 'w') as f:
                f.write('1')
            
            # 永久啟用
            with open('/etc/sysctl.conf', 'a') as f:
                f.write('\nnet.ipv4.ip_forward=1\n')
                
        except Exception as e:
            self.logger.warning(f"Could not enable IP forwarding: {e}")
    
    def _get_public_ip(self):
        """獲取公共 IP 地址"""
        try:
            result = subprocess.run(['curl', '-s', 'ifconfig.me'], 
                                  capture_output=True, text=True, timeout=5)
            return result.stdout.strip()
        except:
            return '127.0.0.1'  # 默認回退
```

## 🗂️ 檔案結構

```
web/
├── app.py                     # Flask 主應用程式
├── vpn_service.py            # VPN 服務管理
├── wireguard_server.py       # WireGuard 伺服器
├── __pycache__/              # Python 緩存
├── static/                   # 靜態資源
│   ├── css/                  # 樣式表
│   ├── js/                   # JavaScript 腳本
│   └── images/               # 圖片資源
└── templates/                # HTML 模板
    ├── base.html             # 基礎模板
    ├── index.html            # 首頁
    ├── dashboard.html        # 儀表板
    ├── nodes.html            # 節點管理
    ├── tasks.html            # 任務管理
    └── vpn.html              # VPN 管理
```

## 🌐 Web 界面功能

### 1. 系統儀表板
```html
<!-- dashboard.html 範例 -->
<!DOCTYPE html>
<html lang="zh-TW">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>HiveMind 管理儀表板</title>
    <link rel="stylesheet" href="{{ url_for('static', filename='css/dashboard.css') }}">
</head>
<body>
    <div class="container">
        <header>
            <h1>HiveMind 分散式計算平台</h1>
            <nav>
                <a href="/dashboard">儀表板</a>
                <a href="/nodes">節點管理</a>
                <a href="/tasks">任務管理</a>
                <a href="/vpn">VPN 管理</a>
            </nav>
        </header>
        
        <main>
            <div class="stats-grid">
                <div class="stat-card">
                    <h3>系統狀態</h3>
                    <div class="metric">
                        <span class="label">CPU 使用率:</span>
                        <span class="value">{{ system_status.cpu_percent }}%</span>
                    </div>
                    <div class="metric">
                        <span class="label">記憶體使用率:</span>
                        <span class="value">{{ system_status.memory_percent }}%</span>
                    </div>
                </div>
                
                <div class="stat-card">
                    <h3>節點統計</h3>
                    <div class="metric">
                        <span class="label">總節點數:</span>
                        <span class="value">{{ node_stats.total_nodes }}</span>
                    </div>
                    <div class="metric">
                        <span class="label">活躍節點:</span>
                        <span class="value">{{ node_stats.active_nodes }}</span>
                    </div>
                </div>
            </div>
            
            <div class="charts-section">
                <div class="chart-container">
                    <canvas id="cpuChart"></canvas>
                </div>
                <div class="chart-container">
                    <canvas id="networkChart"></canvas>
                </div>
            </div>
        </main>
    </div>
    
    <script src="{{ url_for('static', filename='js/dashboard.js') }}"></script>
</body>
</html>
```

### 2. 節點管理界面
```javascript
// static/js/nodes.js
class NodeManager {
    constructor() {
        this.nodes = [];
        this.init();
    }
    
    init() {
        this.loadNodes();
        this.setupEventListeners();
        this.startAutoRefresh();
    }
    
    async loadNodes() {
        try {
            const response = await fetch('/api/nodes');
            this.nodes = await response.json();
            this.renderNodes();
        } catch (error) {
            console.error('Error loading nodes:', error);
        }
    }
    
    renderNodes() {
        const container = document.getElementById('nodes-container');
        container.innerHTML = '';
        
        this.nodes.forEach(node => {
            const nodeCard = this.createNodeCard(node);
            container.appendChild(nodeCard);
        });
    }
    
    createNodeCard(node) {
        const card = document.createElement('div');
        card.className = `node-card status-${node.status}`;
        
        card.innerHTML = `
            <div class="node-header">
                <h3>${node.hostname}</h3>
                <span class="status-badge ${node.status}">${node.status}</span>
            </div>
            <div class="node-details">
                <p><strong>IP:</strong> ${node.ip_address}</p>
                <p><strong>CPU:</strong> ${node.cpu_usage}%</p>
                <p><strong>記憶體:</strong> ${node.memory_usage}%</p>
                <p><strong>最後上線:</strong> ${this.formatTime(node.last_seen)}</p>
            </div>
            <div class="node-actions">
                <button onclick="nodeManager.viewNode('${node.node_id}')">查看詳情</button>
                <button onclick="nodeManager.restartNode('${node.node_id}')">重啟節點</button>
            </div>
        `;
        
        return card;
    }
    
    async restartNode(nodeId) {
        try {
            const response = await fetch(`/api/nodes/${nodeId}/restart`, {
                method: 'POST'
            });
            
            if (response.ok) {
                alert('節點重啟命令已發送');
                this.loadNodes();
            } else {
                alert('重啟節點失敗');
            }
        } catch (error) {
            console.error('Error restarting node:', error);
            alert('重啟節點時發生錯誤');
        }
    }
    
    formatTime(timestamp) {
        return new Date(timestamp).toLocaleString('zh-TW');
    }
    
    startAutoRefresh() {
        setInterval(() => {
            this.loadNodes();
        }, 30000); // 每 30 秒刷新一次
    }
}

// 初始化節點管理器
const nodeManager = new NodeManager();
```

## 📊 監控和指標

### Web 服務指標收集
```python
class WebMetricsCollector:
    def __init__(self):
        self.metrics = {
            'requests': 0,
            'response_times': [],
            'active_sessions': 0,
            'vpn_connections': 0,
            'api_calls': {},
            'error_count': 0
        }
        
    def record_request(self, endpoint, response_time, status_code):
        """記錄請求指標"""
        self.metrics['requests'] += 1
        self.metrics['response_times'].append(response_time)
        
        if endpoint not in self.metrics['api_calls']:
            self.metrics['api_calls'][endpoint] = 0
        self.metrics['api_calls'][endpoint] += 1
        
        if status_code >= 400:
            self.metrics['error_count'] += 1
    
    def get_performance_metrics(self):
        """獲取性能指標"""
        avg_response_time = 0
        if self.metrics['response_times']:
            avg_response_time = sum(self.metrics['response_times']) / len(self.metrics['response_times'])
        
        return {
            'total_requests': self.metrics['requests'],
            'avg_response_time_ms': avg_response_time,
            'active_sessions': self.metrics['active_sessions'],
            'vpn_connections': self.metrics['vpn_connections'],
            'error_rate': self.metrics['error_count'] / max(1, self.metrics['requests']),
            'top_endpoints': sorted(
                self.metrics['api_calls'].items(), 
                key=lambda x: x[1], 
                reverse=True
            )[:10]
        }
```

## 🔒 安全性功能

### 1. 用戶認證系統
```python
from werkzeug.security import generate_password_hash, check_password_hash
import jwt
from datetime import datetime, timedelta

class AuthenticationService:
    def __init__(self, secret_key):
        self.secret_key = secret_key
        self.users = {}  # 在生產環境中應使用資料庫
        
    def register_user(self, username, password, email):
        """註冊用戶"""
        if username in self.users:
            return {'success': False, 'message': '用戶名已存在'}
        
        password_hash = generate_password_hash(password)
        
        self.users[username] = {
            'password_hash': password_hash,
            'email': email,
            'created_at': datetime.now(),
            'last_login': None,
            'role': 'user'
        }
        
        return {'success': True, 'message': '用戶註冊成功'}
    
    def authenticate_user(self, username, password):
        """用戶認證"""
        if username not in self.users:
            return {'success': False, 'message': '用戶不存在'}
        
        user = self.users[username]
        
        if check_password_hash(user['password_hash'], password):
            # 更新最後登入時間
            self.users[username]['last_login'] = datetime.now()
            
            # 生成 JWT Token
            token = self.generate_token(username)
            
            return {
                'success': True,
                'token': token,
                'user': {
                    'username': username,
                    'email': user['email'],
                    'role': user['role']
                }
            }
        else:
            return {'success': False, 'message': '密碼錯誤'}
    
    def generate_token(self, username):
        """生成 JWT Token"""
        payload = {
            'username': username,
            'exp': datetime.utcnow() + timedelta(hours=24)
        }
        return jwt.encode(payload, self.secret_key, algorithm='HS256')
    
    def verify_token(self, token):
        """驗證 JWT Token"""
        try:
            payload = jwt.decode(token, self.secret_key, algorithms=['HS256'])
            return {'success': True, 'username': payload['username']}
        except jwt.ExpiredSignatureError:
            return {'success': False, 'message': 'Token 已過期'}
        except jwt.InvalidTokenError:
            return {'success': False, 'message': 'Token 無效'}
```

## 🔧 使用範例

### 啟動 Web 服務
```python
# 啟動完整 Web 服務
from web.app import web_app
from web.vpn_service import VPNService

# 初始化 VPN 服務
vpn_service = VPNService()
vpn_result = vpn_service.initialize_server()

if vpn_result['status'] == 'success':
    print(f"VPN 伺服器已啟動: {vpn_result['endpoint']}")

# 啟動 Web 應用程式
web_app.start_server(host='0.0.0.0', port=5000, debug=False)
```

### 創建 VPN 客戶端
```python
# 為新節點創建 VPN 配置
client_result = vpn_service.create_client_config('worker-001', 'admin@hivemind.local')

if client_result['status'] == 'success':
    print("客戶端配置:")
    print(client_result['config'])
    
    # 保存配置文件
    with open('worker-001.conf', 'w') as f:
        f.write(client_result['config'])
```

## 🔧 常見問題排解

### 1. Web 服務啟動失敗
**問題**: Flask 應用程式無法啟動
**解決方案**:
```python
def diagnose_web_service():
    # 檢查端口是否被占用
    import socket
    
    def check_port(port):
        sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        result = sock.connect_ex(('localhost', port))
        sock.close()
        return result == 0
    
    if check_port(5000):
        print("端口 5000 已被占用，請更換端口")
    else:
        print("端口 5000 可用")
    
    # 檢查依賴項
    try:
        import flask
        print(f"Flask 版本: {flask.__version__}")
    except ImportError:
        print("Flask 未安裝，請執行: pip install flask")
```

### 2. VPN 連接問題
**問題**: WireGuard VPN 無法連接
**解決方案**:
```python
def diagnose_vpn_issues():
    # 檢查 WireGuard 是否安裝
    import subprocess
    
    try:
        result = subprocess.run(['wg', '--version'], capture_output=True, text=True)
        print(f"WireGuard 版本: {result.stdout}")
    except FileNotFoundError:
        print("WireGuard 未安裝")
        return
    
    # 檢查防火牆設置
    print("檢查防火牆設置...")
    
    # 檢查介面狀態
    try:
        result = subprocess.run(['wg', 'show'], capture_output=True, text=True)
        if result.stdout:
            print("WireGuard 介面狀態:")
            print(result.stdout)
        else:
            print("沒有活躍的 WireGuard 介面")
    except Exception as e:
        print(f"檢查介面狀態時出錯: {e}")
```

### 3. 靜態資源載入失敗
**問題**: CSS/JS 文件無法載入
**解決方案**:
```python
def fix_static_resources():
    import os
    
    static_dir = 'static'
    required_dirs = ['css', 'js', 'images']
    
    # 確保靜態資源目錄存在
    for dir_name in required_dirs:
        dir_path = os.path.join(static_dir, dir_name)
        if not os.path.exists(dir_path):
            os.makedirs(dir_path)
            print(f"創建目錄: {dir_path}")
    
    # 檢查文件權限
    for root, dirs, files in os.walk(static_dir):
        for file in files:
            file_path = os.path.join(root, file)
            if not os.access(file_path, os.R_OK):
                print(f"文件權限問題: {file_path}")
```

---

**相關文檔**:
- [Master Node 模組](master-node.md)
- [Node Pool 模組](node-pool.md)
- [API 文檔](../api.md)
- [部署指南](../deployment.md)
- [安全指南](../security.md)
