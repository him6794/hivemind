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

# 添加調試信息來檢查環境變數是否正確載入
print("🔧 檢查環境變數載入狀態:")
print(f"  - SECRET_KEY: {'已設定' if os.getenv('SECRET_KEY') else '使用預設值'}")
print(f"  - TURNSTILE_SECRET_KEY: {os.getenv('TURNSTILE_SECRET_KEY', '未設定')}")
print(f"  - FLASK_HOST: {os.getenv('FLASK_HOST', '未設定')}")
print(f"  - FLASK_PORT: {os.getenv('FLASK_PORT', '未設定')}")
print(f"  - FLASK_DEBUG: {os.getenv('FLASK_DEBUG', '未設定')}")

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

def verify_turnstile(token, ip_address):
    """驗證 Cloudflare Turnstile token"""
    try:
        # 暫時跳過驗證用於測試
        print("🚧 暫時跳過 Turnstile 驗證（測試模式）")
        return True
        
        # 如果沒有提供 token，直接返回 False
        if not token:
            print("❌ Turnstile: 沒有提供 token")
            return False
            
        secret_key = os.getenv('TURNSTILE_SECRET_KEY')
        if not secret_key:
            print("❌ Turnstile: 沒有設定 TURNSTILE_SECRET_KEY 環境變數")
            return False
            
        print(f"🔑 Turnstile: 使用密鑰 {secret_key[:10]}...")
        
        response = requests.post('https://challenges.cloudflare.com/turnstile/v0/siteverify', {
            'secret': secret_key,
            'response': token,
            'remoteip': ip_address
        }, timeout=10)
        
        result = response.json()
        success = result.get('success', False)
        
        if not success:
            error_codes = result.get('error-codes', [])
            print(f"❌ Turnstile 驗證失敗: {error_codes}")
        else:
            print("✅ Turnstile 驗證成功")
        
        return success
    except Exception as e:
        print(f"❌ Turnstile 驗證錯誤: {e}")
        return False

# API路由 - 使用節點池服務
@app.route('/api/register', methods=['POST'])
def register():
    if not user_service_obj:
        return jsonify({'error': '用戶服務不可用'}), 500
    
    try:
        data = request.get_json()
        username = data.get('username')
        password = data.get('password')
        turnstile_response = data.get('cf-turnstile-response')
        
        # 獲取客戶端 IP
        client_ip = request.headers.get('X-Forwarded-For', request.remote_addr)
        if ',' in client_ip:
            client_ip = client_ip.split(',')[0].strip()
        
        # 驗證人機驗證
        if not turnstile_response:
            return jsonify({'error': '請完成人機驗證'}), 400
            
        if not verify_turnstile(turnstile_response, client_ip):
            return jsonify({'error': '人機驗證失敗，請重試'}), 400
        
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
        turnstile_response = data.get('cf-turnstile-response')
        
        # 獲取客戶端 IP
        client_ip = request.headers.get('X-Forwarded-For', request.remote_addr)
        if ',' in client_ip:
            client_ip = client_ip.split(',')[0].strip()
        
        # 驗證人機驗證
        if not turnstile_response:
            return jsonify({'error': '請完成人機驗證'}), 400
            
        if not verify_turnstile(turnstile_response, client_ip):
            return jsonify({'error': '人機驗證失敗，請重試'}), 400
        
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

@app.route('/api/refresh-token', methods=['POST'])
def refresh_token():
    """刷新用戶 Token"""
    if not user_service_obj:
        return jsonify({'error': '用戶服務不可用'}), 500
    
    try:
        auth_header = request.headers.get('Authorization', '')
        if not auth_header.startswith('Bearer '):
            return jsonify({'error': '無效的認證令牌'}), 401
        
        old_token = auth_header[7:]  # 移除 'Bearer ' 前綴
        
        # 創建模擬的gRPC請求對象
        class MockRequest:
            def __init__(self, token):
                self.token = token
        
        class MockContext:
            pass
        
        mock_request = MockRequest(old_token)
        mock_context = MockContext()
        
        # 調用 user_service_obj 的 Token 刷新服務
        response = user_service_obj.RefreshToken(mock_request, mock_context)
        
        if response.success:
            return jsonify({
                'message': 'Token 刷新成功',
                'access_token': response.token
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
            # 檢查是否為 Token 過期
            if response.message == "TOKEN_EXPIRED":
                return jsonify({
                    'error': 'Token 已過期',
                    'error_code': 'TOKEN_EXPIRED'
                }), 401
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
        
        # 定義下載檔案的路徑
        download_dir = os.path.join(os.path.dirname(__file__), 'downloads')
        
        if file_type == 'worker':
            # 返回工作端 ZIP 檔案
            file_path = os.path.join(download_dir, 'hivemind-worker.zip')
            if os.path.exists(file_path):
                return send_file(
                    file_path, 
                    as_attachment=True, 
                    download_name='HiveMind-Worker.zip',
                    mimetype='application/zip'
                )
            else:
                return jsonify({'error': '工作端檔案不存在'}), 404
                
        elif file_type == 'master':
            # 返回主控端 ZIP 檔案
            file_path = os.path.join(download_dir, 'hivemind-master.zip')
            if os.path.exists(file_path):
                return send_file(
                    file_path, 
                    as_attachment=True, 
                    download_name='HiveMind-Master.zip',
                    mimetype='application/zip'
                )
            else:
                return jsonify({'error': '主控端檔案不存在'}), 404
                
        elif file_type == 'server':
            # 伺服器端開發中
            return jsonify({
                'status': 'development',
                'message': '伺服器端正在開發中，敬請期待！',
                'estimated_release': '2024年第二季度'
            }), 200
            
        elif file_type == 'mobile':
            # 移動端開發中
            return jsonify({
                'status': 'development',
                'message': '移動端正在開發中，敬請期待！',
                'platforms': ['Android', 'iOS'],
                'estimated_release': '2024年第三季度'
            }), 200
            
        elif file_type == 'web':
            # Web 端開發中
            return jsonify({
                'status': 'development',
                'message': 'Web 端正在開發中，敬請期待！',
                'features': ['瀏覽器直接運行', '無需安裝', '跨平台支持'],
                'estimated_release': '2024年第四季度'
            }), 200
            
        elif file_type == 'vpn-config':
            # VPN 配置檔案
            vpn_data = {
                'client_ip': request.remote_addr,
                'client_name': username
            }
            
            result, error = call_vpn_service('/vpn/create_client', 'POST', vpn_data)
            
            if error:
                return jsonify({'error': error}), 500
            
            # 創建臨時配置檔案
            import tempfile
            with tempfile.NamedTemporaryFile(mode='w', suffix='.conf', delete=False) as f:
                f.write(result.get('config', ''))
                temp_path = f.name
            
            try:
                return send_file(
                    temp_path,
                    as_attachment=True,
                    download_name=f'{username}_wireguard.conf',
                    mimetype='text/plain'
                )
            finally:
                # 下載完成後刪除臨時檔案
                try:
                    os.unlink(temp_path)
                except:
                    pass
        else:
            return jsonify({'error': '不支援的檔案類型'}), 404
            
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
@app.route('/balance')
def balance_page():
    return render_template('balance.html')

@app.route('/sponsor')
def sponsor_page():
    return render_template('sponsor.html')

@app.route('/terms')
def terms_page():
    return render_template('terms.html')

@app.route('/privacy')
def privacy_page():
    return render_template('privacy.html')

if __name__ == '__main__':
    # 從環境變數讀取運行配置
    host = os.getenv('FLASK_HOST', '0.0.0.0')
    port = int(os.getenv('FLASK_PORT', '80'))
    debug = os.getenv('FLASK_DEBUG', 'False').lower() == 'true'
    
    print(f"啟動 Flask 應用程式:")
    print(f"  - 主機: {host}")
    print(f"  - 端口: {port}")
    print(f"  - 調試模式: {debug}")
    print(f"  - VPN 服務 URL: {VPN_SERVICE_URL}")
    print(f"  - 限流設置: {RATE_LIMIT_SECONDS} 秒")
    
    app.run(debug=debug, host=host, port=5000)