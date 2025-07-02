from flask import Flask, request, jsonify, render_template, send_file
from datetime import datetime, timedelta
import uuid
import os
import sys
import time
from collections import defaultdict
# 添加 VPN 模組路徑
sys.path.append(os.path.abspath(os.path.join(os.path.dirname(__file__), '..', 'vpn')))
sys.path.append(os.path.abspath(os.path.join(os.path.dirname(__file__), '..')))
from 節點池 import user_service
from wireguard_server import WireGuardServer
from dotenv import load_dotenv

# 載入環境變數
load_dotenv()

app = Flask(__name__)
app.config['SECRET_KEY'] = os.getenv('SECRET_KEY', 'dev-secret-key-change-in-production')

# 從環境變數載入配置
WIREGUARD_CONFIG = {
    'server_ip': os.getenv('WIREGUARD_SERVER_IP', '114.25.181.43'),
    'server_port': int(os.getenv('WIREGUARD_SERVER_PORT', '51820')),
    'network': os.getenv('WIREGUARD_NETWORK', '10.0.0.0/24'),
    'dns_servers': os.getenv('VPN_DNS_SERVERS', '8.8.8.8,1.1.1.1').split(','),
    'persistent_keepalive': int(os.getenv('VPN_PERSISTENT_KEEPALIVE', '25')),
    'mtu': int(os.getenv('VPN_MTU', '1420'))
}

SECURITY_CONFIG = {
    'rate_limit_seconds': int(os.getenv('RATE_LIMIT_SECONDS', '5')),
    'max_clients_per_user': int(os.getenv('MAX_CLIENTS_PER_USER', '5')),
    'jwt_secret': os.getenv('JWT_SECRET_KEY', 'jwt-secret-change-this'),
    'token_expiration_hours': int(os.getenv('TOKEN_EXPIRATION_HOURS', '24'))
}

STORAGE_CONFIG = {
    'config_dir': os.getenv('CONFIG_DIR', 'd:/hivemind/wireguard_configs'),
    'log_dir': os.getenv('LOG_DIR', 'd:/hivemind/vpn/logs'),
    'upload_dir': os.getenv('UPLOAD_DIR', 'd:/hivemind/uploads'),
    'max_file_size': int(os.getenv('MAX_FILE_SIZE', '10485760'))
}

# 初始化服務
user_service_obj = user_service.UserServiceServicer()
wireguard_server = None

# IP 限流字典
ip_rate_limit = defaultdict(float)
RATE_LIMIT_SECONDS = SECURITY_CONFIG['rate_limit_seconds']

def check_rate_limit(ip_address):
    """檢查 IP 是否在限流範圍內"""
    current_time = time.time()
    last_request_time = ip_rate_limit[ip_address]
    
    if current_time - last_request_time < RATE_LIMIT_SECONDS:
        return False
    
    ip_rate_limit[ip_address] = current_time
    return True

def init_wireguard_server():
    """初始化 WireGuard 伺服器"""
    global wireguard_server
    try:
        # 確保配置目錄存在
        os.makedirs(STORAGE_CONFIG['config_dir'], exist_ok=True)
        os.makedirs(STORAGE_CONFIG['log_dir'], exist_ok=True)
        
        wireguard_server = WireGuardServer(
            interface_name="wg0",
            server_port=WIREGUARD_CONFIG['server_port'],
            network=WIREGUARD_CONFIG['network']
        )
        
        # 如果有預設的伺服器密鑰，使用它們
        server_private_key = os.getenv('WIREGUARD_SERVER_PRIVATE_KEY')
        server_public_key = os.getenv('WIREGUARD_SERVER_PUBLIC_KEY')
        
        if server_private_key and server_public_key:
            wireguard_server.server_private_key = server_private_key
            wireguard_server.server_public_key = server_public_key
            print("使用環境變數中的 WireGuard 伺服器密鑰")
        
        print(f"WireGuard 伺服器初始化成功 - 端口: {WIREGUARD_CONFIG['server_port']}")
        
    except Exception as e:
        print(f"WireGuard 伺服器初始化失敗: {e}")
        wireguard_server = None

def validate_user_limits(username):
    """檢查用戶客戶端數量限制"""
    if not wireguard_server:
        return False
        
    user_client_count = sum(1 for client_name in wireguard_server.clients.keys() 
                           if client_name.startswith(username))
    
    return user_client_count < SECURITY_CONFIG['max_clients_per_user']

def get_server_endpoint():
    """獲取伺服器端點"""
    external_ip = WIREGUARD_CONFIG['server_ip']
    
    # 如果是佔位符IP，嘗試獲取真實外部IP
    if external_ip == "YOUR_SERVER_IP" or external_ip == "0.0.0.0":
        try:
            import urllib.request
            ip_services = os.getenv('EXTERNAL_IP_SERVICE', 'https://ipv4.icanhazip.com')
            backup_services = os.getenv('BACKUP_IP_SERVICES', '').split(',')
            
            for service in [ip_services] + backup_services:
                if service.strip():
                    try:
                        response = urllib.request.urlopen(service.strip(), timeout=5)
                        external_ip = response.read().decode().strip()
                        break
                    except:
                        continue
        except:
            external_ip = "YOUR_SERVER_IP"
    
    return f"{external_ip}:{WIREGUARD_CONFIG['server_port']}"

# 啟動時初始化
init_wireguard_server()

# API路由 - 使用節點池服務
@app.route('/api/register', methods=['POST'])
def register():
    if not user_service_obj:
        return jsonify({'error': '用戶服務不可用'}), 500
    
    try:
        data = request.get_json()
        username = data.get('username')
        email = data.get('email')
        password = data.get('password')
        
        # 創建模擬的gRPC請求對象
        class MockRequest:
            def __init__(self, username, password):
                self.username = username
                self.password = password
        
        class MockContext:
            def __init__(self):
                self.code = None
                self.details = None
            
            def set_code(self, code):
                self.code = code
            
            def set_details(self, details):
                self.details = details
        
        mock_request = MockRequest(username, password)
        mock_context = MockContext()
        
        # 調用 user_service_obj 的註冊服務
        response = user_service_obj.Register(mock_request, mock_context)
        
        if response.success:
            # 登入獲取token
            login_request = MockRequest(username, password)
            login_response = user_service_obj.Login(login_request, mock_context)
            
            if login_response.success:
                return jsonify({
                    'message': '註冊成功',
                    'access_token': login_response.token,
                    'user': {
                        'username': username,
                        'email': email,
                        'balance': 0.0
                    }
                }), 201
            else:
                return jsonify({'error': '註冊成功但登入失敗'}), 500
        else:
            return jsonify({'error': response.message}), 400
            
    except Exception as e:
        return jsonify({'error': str(e)}), 500

@app.route('/api/login', methods=['POST'])
def login():
    if not user_service_obj:
        return jsonify({'error': '用戶服務不可用'}), 500
    
    try:
        data = request.get_json()
        username = data.get('username')
        password = data.get('password')
        
        # 創建模擬的gRPC請求對象
        class MockRequest:
            def __init__(self, username, password):
                self.username = username
                self.password = password
        
        class MockContext:
            pass
        
        mock_request = MockRequest(username, password)
        mock_context = MockContext()
        
        # 調用 user_service_obj 的登入服務
        response = user_service_obj.Login(mock_request, mock_context)
        
        if response.success:
            # 獲取用戶餘額
            balance_request = type('obj', (object,), {'token': response.token})()
            balance_response = user_service_obj.GetBalance(balance_request, mock_context)
            
            balance = balance_response.balance if balance_response.success else 0.0
            
            return jsonify({
                'message': '登入成功',
                'access_token': response.token,
                'user': {
                    'username': username,
                    'balance': balance
                }
            }), 200
        else:
            return jsonify({'error': response.message}), 401
            
    except Exception as e:
        return jsonify({'error': str(e)}), 500

@app.route('/api/balance', methods=['GET'])
def get_balance():
    if not user_service_obj:
        return jsonify({'error': '用戶服務不可用'}), 500
    
    try:
        auth_header = request.headers.get('Authorization', '')
        if not auth_header.startswith('Bearer '):
            return jsonify({'error': '無效的認證令牌'}), 401
        
        token = auth_header[7:]  # 移除 'Bearer ' 前綴
        
        # 創建模擬的gRPC請求對象
        class MockRequest:
            def __init__(self, token):
                self.token = token
        
        class MockContext:
            pass
        
        mock_request = MockRequest(token)
        mock_context = MockContext()
        
        # 調用 user_service_obj 的餘額查詢服務
        response = user_service_obj.GetBalance(mock_request, mock_context)
        
        if response.success:
            return jsonify({'balance': response.balance}), 200
        else:
            return jsonify({'error': response.message}), 400
            
    except Exception as e:
        return jsonify({'error': str(e)}), 500

@app.route('/api/transfer', methods=['POST'])
def transfer():
    if not user_service_obj:
        return jsonify({'error': '用戶服務不可用'}), 500
    
    try:
        auth_header = request.headers.get('Authorization', '')
        if not auth_header.startswith('Bearer '):
            return jsonify({'error': '無效的認證令牌'}), 401
        
        token = auth_header[7:]
        data = request.get_json()
        amount = data.get('amount')
        transaction_type = data.get('type', 'payment')
        receiver = data.get('receiver', 'system')
        description = data.get('description', '')
        
        # 創建模擬的gRPC請求對象
        class MockRequest:
            def __init__(self, token, amount, receiver):
                self.token = token
                self.amount = amount
                self.receiver_username = receiver
        
        class MockContext:
            pass
        
        mock_request = MockRequest(token, amount, receiver)
        mock_context = MockContext()
        
        # 調用 user_service_obj 的轉帳服務
        response = user_service_obj.Transfer(mock_request, mock_context)
        
        if response.success:
            # 獲取更新後的餘額
            balance_request = type('obj', (object,), {'token': token})()
            balance_response = user_service_obj.GetBalance(balance_request, mock_context)
            
            return jsonify({
                'message': '轉帳成功',
                'new_balance': balance_response.balance if balance_response.success else 0.0
            }), 200
        else:
            return jsonify({'error': response.message}), 400
            
    except Exception as e:
        return jsonify({'error': str(e)}), 500

@app.route('/api/vpn/join', methods=['POST'])
def join_vpn():
    """加入VPN - 無需登入，僅限流保護，直接返回配置文件"""
    global wireguard_server
    
    # 獲取客戶端真實 IP
    client_ip = request.headers.get('X-Forwarded-For', request.remote_addr)
    if ',' in client_ip:
        client_ip = client_ip.split(',')[0].strip()
    
    # 檢查限流
    if not check_rate_limit(client_ip):
        return jsonify({
            'error': '請求過於頻繁，請等待 5 秒後再試',
            'rate_limit_seconds': RATE_LIMIT_SECONDS,
            'client_ip': client_ip
        }), 429
    
    if not wireguard_server:
        return jsonify({'error': 'VPN 服務不可用'}), 500
    
    try:
        data = request.get_json() if request.is_json else {}
        custom_name = data.get('client_name', '').strip()
        
        # 生成客戶端名稱
        timestamp = int(datetime.now().timestamp())
        ip_suffix = client_ip.replace('.', '_').replace(':', '_')
        
        if custom_name and len(custom_name) <= 20 and custom_name.replace('_', '').replace('-', '').isalnum():
            client_name = f"{custom_name}_{timestamp}"
        else:
            client_name = f"client_{ip_suffix}_{timestamp}"
        
        # 檢查客戶端名稱是否已存在
        if client_name in wireguard_server.clients:
            client_name = f"{client_name}_{uuid.uuid4().hex[:6]}"
        
        # 使用 WireGuard 伺服器生成配置
        try:
            client_config = wireguard_server.add_client(client_name)
            
            # 使用環境變數自定義配置內容
            dns_servers = ', '.join(WIREGUARD_CONFIG['dns_servers'])
            server_endpoint = get_server_endpoint()
            
            vpn_config_content = f"""[Interface]
PrivateKey = {client_config['private_key']}
Address = {client_config['ip']}/24
DNS = {dns_servers}
MTU = {WIREGUARD_CONFIG['mtu']}

[Peer]
PublicKey = {client_config['server_public_key']}
Endpoint = {server_endpoint}
AllowedIPs = 0.0.0.0/0
PersistentKeepalive = {WIREGUARD_CONFIG['persistent_keepalive']}
"""
            
            # 保存配置文件到指定目錄
            config_file_path = os.path.join(STORAGE_CONFIG['config_dir'], f"{client_name}.conf")
            with open(config_file_path, 'w', encoding='utf-8') as f:
                f.write(vpn_config_content)
            
            # 記錄生成事件
            if hasattr(wireguard_server, 'logger'):
                wireguard_server.logger.info(f"為 IP {client_ip} 生成 VPN 配置，客戶端: {client_name}")
            
            return jsonify({
                'success': True,
                'message': 'VPN 配置生成成功',
                'config': vpn_config_content,
                'client_name': client_name,
                'client_ip': client_config['ip'],
                'server_endpoint': server_endpoint,
                'generated_at': datetime.now().isoformat(),
                'config_file_path': config_file_path,
                'filename': f"{client_name}.conf"
            }), 200
            
        except Exception as e:
            return jsonify({
                'success': False,
                'error': f'生成 VPN 配置失敗: {str(e)}'
            }), 500
            
    except Exception as e:
        return jsonify({
            'success': False,
            'error': f'服務器錯誤: {str(e)}'
        }), 500

@app.route('/api/vpn/status', methods=['GET'])
def vpn_status():
    global wireguard_server
    
    if not wireguard_server:
        return jsonify({'error': 'VPN 服務不可用'}), 500
    
    try:
        auth_header = request.headers.get('Authorization', '')
        if not auth_header.startswith('Bearer '):
            return jsonify({'error': '無效的認證令牌'}), 401
        
        token = auth_header[7:]
        user_info = user_service_obj.verify_token(token)
        if not user_info:
            return jsonify({'error': '無效的認證令牌'}), 401
        
        username = user_info.get('username', f"user_{user_info['user_id']}")
        
        # 獲取用戶的客戶端列表
        user_clients = []
        for client_name, client_info in wireguard_server.clients.items():
            if client_name.startswith(username):
                user_clients.append({
                    'name': client_name,
                    'ip': client_info['ip'],
                    'created': 'N/A'  # 可以從文件時間戳獲取
                })
        
        return jsonify({
            'server_status': 'running' if wireguard_server.monitoring_active else 'stopped',
            'server_ip': wireguard_server.server_ip,
            'server_port': wireguard_server.server_port,
            'total_clients': len(wireguard_server.clients),
            'user_clients': user_clients,
            'recent_connections': wireguard_server.connection_log[-10:] if hasattr(wireguard_server, 'connection_log') else []
        }), 200
        
    except Exception as e:
        return jsonify({'error': str(e)}), 500

@app.route('/api/vpn/disconnect/<client_name>', methods=['DELETE'])
def disconnect_vpn_client(client_name):
    global wireguard_server
    
    if not wireguard_server:
        return jsonify({'error': 'VPN 服務不可用'}), 500
    
    try:
        auth_header = request.headers.get('Authorization', '')
        if not auth_header.startswith('Bearer '):
            return jsonify({'error': '無效的認證令牌'}), 401
        
        token = auth_header[7:]
        user_info = user_service_obj.verify_token(token)
        if not user_info:
            return jsonify({'error': '無效的認證令牌'}), 401
        
        username = user_info.get('username', f"user_{user_info['user_id']}")
        
        # 檢查客戶端是否屬於該用戶
        if not client_name.startswith(username):
            return jsonify({'error': '無權限操作此客戶端'}), 403
        
        # 移除客戶端
        if wireguard_server.remove_client(client_name):
            wireguard_server.logger.info(f"用戶 {username} 移除客戶端: {client_name}")
            return jsonify({'message': f'客戶端 {client_name} 已移除'}), 200
        else:
            return jsonify({'error': '客戶端不存在'}), 404
            
    except Exception as e:
        return jsonify({'error': str(e)}), 500

@app.route('/api/download/<file_type>')
def download_file(file_type):
    if not user_service_obj:
        return jsonify({'error': '用戶服務不可用'}), 500
    
    try:
        auth_header = request.headers.get('Authorization', '')
        if not auth_header.startswith('Bearer '):
            return jsonify({'error': '請先登入'}), 401
        
        token = auth_header[7:]
        user_info = user_service_obj.verify_token(token)
        if not user_info:
            return jsonify({'error': '無效的認證令牌'}), 401
        
        username = user_info.get('username', f"user_{user_info['user_id']}")
        
        if file_type == 'client':
            return jsonify({
                'download_url': '/static/downloads/hivemind-client.zip',
                'version': '1.0.0',
                'size': '25.6 MB',
                'description': 'HiveMind 分布式運算客戶端'
            }), 200
            
        elif file_type == 'vpn-config':
            if not wireguard_server:
                return jsonify({'error': 'VPN 服務不可用'}), 500
                
            # 生成新的 VPN 配置
            client_name = f"{username}_{int(datetime.now().timestamp())}"
            
            try:
                client_config = wireguard_server.add_client(client_name)
                vpn_config_content = wireguard_server.get_client_config(client_name)
                
                return jsonify({
                    'config': vpn_config_content,
                    'client_name': client_name,
                    'client_ip': client_config['ip']
                }), 200
                
            except Exception as e:
                return jsonify({'error': f'生成 VPN 配置失敗: {str(e)}'}), 500
        else:
            return jsonify({'error': '不支援的文件類型'}), 404
            
    except Exception as e:
        return jsonify({'error': str(e)}), 500

@app.route('/api/vpn/config', methods=['GET'])
def get_vpn_config_info():
    """獲取 VPN 配置信息（不包含敏感資料）"""
    return jsonify({
        'server_info': {
            'endpoint': get_server_endpoint(),
            'network': WIREGUARD_CONFIG['network'],
            'dns_servers': WIREGUARD_CONFIG['dns_servers'],
            'persistent_keepalive': WIREGUARD_CONFIG['persistent_keepalive'],
            'mtu': WIREGUARD_CONFIG['mtu']
        },
        'limits': {
            'rate_limit_seconds': SECURITY_CONFIG['rate_limit_seconds'],
            'max_clients_per_user': SECURITY_CONFIG['max_clients_per_user']
        },
        'total_clients': len(wireguard_server.clients) if wireguard_server else 0
    }), 200

@app.route('/api/system/health', methods=['GET'])
def health_check():
    """系統健康檢查"""
    health_status = {
        'status': 'healthy',
        'timestamp': datetime.now().isoformat(),
        'services': {
            'user_service': bool(user_service_obj),
            'wireguard_server': bool(wireguard_server),
            'config_directory': os.path.exists(STORAGE_CONFIG['config_dir']),
            'log_directory': os.path.exists(STORAGE_CONFIG['log_dir'])
        },
        'configuration': {
            'rate_limit_enabled': RATE_LIMIT_SECONDS > 0,
            'max_clients_per_user': SECURITY_CONFIG['max_clients_per_user'],
            'debug_mode': app.debug
        }
    }
    
    # 檢查是否有任何服務異常
    if not all(health_status['services'].values()):
        health_status['status'] = 'degraded'
    
    return jsonify(health_status), 200

# 前端路由
@app.route('/')
def index():
    return render_template('index.html')

@app.route('/login')
def login_page():
    return render_template('login.html')

@app.route('/register')
def register_page():
    return render_template('register.html')

@app.route('/download')
def download_page():
    return render_template('download.html')

@app.route('/docs')
def docs_page():
    return render_template('docs.html')

@app.route('/balance')
def balance_page():
    return render_template('balance.html')

@app.route('/vpn')
def vpn_page():
    return render_template('vpn.html')

if __name__ == '__main__':
    # 從環境變數讀取運行配置
    host = os.getenv('FLASK_HOST', '0.0.0.0')
    port = int(os.getenv('FLASK_PORT', '5000'))  # 改為 5000
    debug = os.getenv('FLASK_DEBUG', 'False').lower() == 'true'
    
    print(f"啟動 Flask 應用程式:")
    print(f"  - 主機: {host}")
    print(f"  - 端口: {port}")
    print(f"  - 調試模式: {debug}")
    print(f"  - 配置目錄: {STORAGE_CONFIG['config_dir']}")
    print(f"  - 日誌目錄: {STORAGE_CONFIG['log_dir']}")
    print(f"  - 限流設置: {RATE_LIMIT_SECONDS} 秒")
    
    app.run(debug=debug, host=host, port=port)