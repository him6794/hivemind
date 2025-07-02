from flask import Flask, request, jsonify, render_template, send_file
from datetime import datetime, timedelta
import uuid
import os
import sys
import time
from collections import defaultdict
import requests
# 添加節點池模組路徑
sys.path.append(os.path.abspath(os.path.join(os.path.dirname(__file__), '..', 'vpn')))
sys.path.append(os.path.abspath(os.path.join(os.path.dirname(__file__), '..')))
from 節點池 import user_service
from dotenv import load_dotenv

# 載入環境變數
load_dotenv()

app = Flask(__name__)
app.config['SECRET_KEY'] = os.getenv('SECRET_KEY', 'dev-secret-key-change-in-production')

# VPN 服務配置
VPN_SERVICE_CONFIG = {
    'host': os.getenv('VPN_SERVICE_HOST', '127.0.0.1'),
    'port': int(os.getenv('VPN_SERVICE_PORT', '5008')),
    'timeout': int(os.getenv('VPN_SERVICE_TIMEOUT', '10'))
}

VPN_SERVICE_URL = f"http://{VPN_SERVICE_CONFIG['host']}:{VPN_SERVICE_CONFIG['port']}"

SECURITY_CONFIG = {
    'rate_limit_seconds': int(os.getenv('RATE_LIMIT_SECONDS', '5')),
    'max_clients_per_user': int(os.getenv('MAX_CLIENTS_PER_USER', '5')),
    'jwt_secret': os.getenv('JWT_SECRET_KEY', 'jwt-secret-change-this'),
    'token_expiration_hours': int(os.getenv('TOKEN_EXPIRATION_HOURS', '24'))
}

STORAGE_CONFIG = {
    'log_dir': os.getenv('LOG_DIR', '/mnt/myusb/hivemind/vpn/logs'),
    'upload_dir': os.getenv('UPLOAD_DIR', '/mnt/myusb/hivemind/uploads'),
    'max_file_size': int(os.getenv('MAX_FILE_SIZE', '10485760'))
}

# 初始化服務
user_service_obj = user_service.UserServiceServicer()

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

def call_vpn_service(endpoint, method='GET', data=None):
    """調用 VPN 服務的通用方法"""
    try:
        url = f"{VPN_SERVICE_URL}{endpoint}"
        
        if method == 'GET':
            response = requests.get(url, timeout=VPN_SERVICE_CONFIG['timeout'])
        elif method == 'POST':
            response = requests.post(url, json=data, timeout=VPN_SERVICE_CONFIG['timeout'])
        elif method == 'DELETE':
            response = requests.delete(url, timeout=VPN_SERVICE_CONFIG['timeout'])
        else:
            return None, f"不支援的 HTTP 方法: {method}"
        
        if response.status_code == 200:
            return response.json(), None
        else:
            return None, f"VPN 服務錯誤: {response.status_code}"
            
    except requests.exceptions.ConnectionError:
        return None, "無法連接到 VPN 服務，請確認 VPN 服務器已啟動"
    except requests.exceptions.Timeout:
        return None, "VPN 服務請求超時"
    except Exception as e:
        return None, f"VPN 服務調用失敗: {str(e)}"

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

    try:
        data = request.get_json() if request.is_json else {}
        custom_name = data.get('client_name', '').strip()
        
        # 調用 VPN 服務創建客戶端
        vpn_data = {
            'client_ip': client_ip,
            'client_name': custom_name
        }
        
        result, error = call_vpn_service('/vpn/create_client', 'POST', vpn_data)
        
        if error:
            return jsonify({
                'success': False,
                'error': error
            }), 500
        
        return jsonify(result), 200
            
    except Exception as e:
        return jsonify({
            'success': False,
            'error': f'服務器錯誤: {str(e)}'
        }), 500

@app.route('/api/vpn/status', methods=['GET'])
def vpn_status():
    try:
        auth_header = request.headers.get('Authorization', '')
        if not auth_header.startswith('Bearer '):
            return jsonify({'error': '無效的認證令牌'}), 401
        
        token = auth_header[7:]
        user_info = user_service_obj.verify_token(token)
        if not user_info:
            return jsonify({'error': '無效的認證令牌'}), 401
        
        username = user_info.get('username', f"user_{user_info['user_id']}")
        
        # 調用 VPN 服務獲取狀態
        result, error = call_vpn_service('/vpn/status')
        
        if error:
            return jsonify({'error': error}), 500
        
        # 過濾用戶的客戶端
        all_clients = result.get('clients', [])
        user_clients = [client for client in all_clients if client.startswith(username)]
        
        result['user_clients'] = user_clients
        return jsonify(result), 200
        
    except Exception as e:
        return jsonify({'error': str(e)}), 500

@app.route('/api/vpn/disconnect/<client_name>', methods=['DELETE'])
def disconnect_vpn_client(client_name):
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
        
        # 調用 VPN 服務移除客戶端
        result, error = call_vpn_service(f'/vpn/remove_client/{client_name}', 'DELETE')
        
        if error:
            return jsonify({'error': error}), 500
        
        return jsonify(result), 200
        
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
            # 調用 VPN 服務生成配置
            vpn_data = {
                'client_ip': request.remote_addr,
                'client_name': username
            }
            
            result, error = call_vpn_service('/vpn/create_client', 'POST', vpn_data)
            
            if error:
                return jsonify({'error': error}), 500
            
            return jsonify({
                'config': result.get('config'),
                'client_name': result.get('client_name'),
                'client_ip': result.get('client_ip')
            }), 200
        else:
            return jsonify({'error': '不支援的文件類型'}), 404
            
    except Exception as e:
        return jsonify({'error': str(e)}), 500

@app.route('/api/vpn/config', methods=['GET'])
def get_vpn_config_info():
    """獲取 VPN 配置信息（不包含敏感資料）"""
    result, error = call_vpn_service('/vpn/config_info')
    
    if error:
        return jsonify({'error': error}), 500
    
    return jsonify(result), 200

@app.route('/api/system/health', methods=['GET'])
def health_check():
    """系統健康檢查"""
    # 檢查 VPN 服務狀態
    vpn_health, vpn_error = call_vpn_service('/health')
    
    health_status = {
        'status': 'healthy',
        'timestamp': datetime.now().isoformat(),
        'services': {
            'user_service': bool(user_service_obj),
            'vpn_service': vpn_health is not None,
            'log_directory': os.path.exists(STORAGE_CONFIG['log_dir'])
        },
        'configuration': {
            'rate_limit_enabled': RATE_LIMIT_SECONDS > 0,
            'max_clients_per_user': SECURITY_CONFIG['max_clients_per_user'],
            'debug_mode': app.debug,
            'vpn_service_url': VPN_SERVICE_URL
        }
    }
    
    if vpn_health:
        health_status['vpn_service_status'] = vpn_health
    else:
        health_status['vpn_service_error'] = vpn_error
    
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

if __name__ == '__main__':
    # 從環境變數讀取運行配置
    host = os.getenv('FLASK_HOST', '0.0.0.0')
    port = int(os.getenv('FLASK_PORT', '5007'))
    debug = os.getenv('FLASK_DEBUG', 'False').lower() == 'true'
    
    print(f"啟動 Flask 應用程式:")
    print(f"  - 主機: {host}")
    print(f"  - 端口: {port}")
    print(f"  - 調試模式: {debug}")
    print(f"  - VPN 服務 URL: {VPN_SERVICE_URL}")
    print(f"  - 限流設置: {RATE_LIMIT_SECONDS} 秒")
    
    app.run(debug=debug, host=host, port=port)