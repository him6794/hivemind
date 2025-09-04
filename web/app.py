from flask import Flask, request, jsonify, render_template, send_file, redirect
from datetime import datetime, timedelta
import uuid
import os
import sys
import time
from collections import defaultdict
import requests
import bcrypt
import ipaddress
import markdown
from pathlib import Path

# 添加節點池模組路徑
sys.path.append(os.path.abspath(os.path.join(os.path.dirname(__file__), '..', 'vpn')))
sys.path.append(os.path.abspath(os.path.join(os.path.dirname(__file__), '..')))
from node_pool import user_service
from node_pool.config import Config
# 添加調試信息來檢查配置載入狀態
print("🔧 檢查配置載入狀態:")
print(f"  - SECRET_KEY: {'已設定' if Config.SECRET_KEY != 'dev-secret-key-change-in-production' else '使用預設值'}")
print(f"  - TURNSTILE_SECRET_KEY: {'已設定' if Config.TURNSTILE_SECRET_KEY else '未設定'}")
print(f"  - RESEND_API_KEY: {'已設定' if Config.RESEND_API_KEY else '未設定'}")
print(f"  - BASE_URL: {Config.BASE_URL}")
print(f"  - 開發模式: {Config.is_development()}")

# 驗證必要配置
missing_configs = Config.validate_config()
if missing_configs:
    print("⚠️  警告：以下配置缺失或使用預設值:")
    for config in missing_configs:
        print(f"    - {config}")

app = Flask(__name__)
app.config['SECRET_KEY'] = Config.SECRET_KEY

@app.after_request
def add_security_headers(response):
    """添加安全頭部"""
    # 僅在非開發模式下添加安全頭部
    if not Config.is_development():
        # 內容安全政策 - 放寬限制以允許常用的 CDN
        response.headers['Content-Security-Policy'] = (
            "default-src 'self'; "
            "script-src 'self' 'unsafe-inline' 'unsafe-eval' "
            "https://challenges.cloudflare.com "
            "https://static.cloudflareinsights.com "
            "https://cdn.jsdelivr.net "
            "https://cdnjs.cloudflare.com; "
            "style-src 'self' 'unsafe-inline' "
            "https://cdn.jsdelivr.net "
            "https://cdnjs.cloudflare.com "
            "https://fonts.googleapis.com; "
            "font-src 'self' "
            "https://cdn.jsdelivr.net "
            "https://cdnjs.cloudflare.com "
            "https://fonts.gstatic.com; "
            "img-src 'self' data: https:; "
            "connect-src 'self' https:; "
            "frame-src https://challenges.cloudflare.com;"
        )
        
        # 其他安全頭部
        response.headers['X-Content-Type-Options'] = 'nosniff'
        response.headers['X-Frame-Options'] = 'DENY'
        response.headers['X-XSS-Protection'] = '1; mode=block'
        
        # 在 HTTPS 環境下添加 HSTS
        if request.is_secure or request.headers.get('X-Forwarded-Proto') == 'https':
            response.headers['Strict-Transport-Security'] = 'max-age=31536000; includeSubDomains'
    
    # 移除伺服器資訊
    response.headers.pop('Server', None)
    
    return response

# 使用 Config 的配置
VPN_SERVICE_URL = f"http://{Config.VPN_SERVICE_HOST}:{Config.VPN_SERVICE_PORT}"

SECURITY_CONFIG = {
    'rate_limit_seconds': Config.RATE_LIMIT_SECONDS,
    'max_clients_per_user': Config.MAX_CLIENTS_PER_USER,
    'jwt_secret': Config.JWT_SECRET_KEY,
    'token_expiration_hours': Config.TOKEN_EXPIRATION_HOURS
}

STORAGE_CONFIG = {
    'log_dir': Config.LOG_DIR,
    'upload_dir': Config.UPLOAD_DIR,
    'max_file_size': Config.MAX_FILE_SIZE
}

# 初始化服務
user_service_obj = user_service.UserServiceServicer()

# 文檔路徑配置
DOCS_BASE_PATH = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', 'documentation'))

def load_markdown_file(file_path):
    """載入並轉換 Markdown 檔案為 HTML"""
    try:
        full_path = os.path.join(DOCS_BASE_PATH, file_path)
        if not os.path.exists(full_path):
            return None, f"檔案不存在: {file_path}"
        
        with open(full_path, 'r', encoding='utf-8') as f:
            content = f.read()
        
        # 配置 markdown 擴展
        md = markdown.Markdown(extensions=[
            'markdown.extensions.codehilite',
            'markdown.extensions.fenced_code',
            'markdown.extensions.tables',
            'markdown.extensions.toc',
            'markdown.extensions.meta'
        ])
        
        html_content = md.convert(content)
        
        # 獲取元數據（如果有的話）
        meta = getattr(md, 'Meta', {})
        
        return {
            'content': html_content,
            'meta': meta,
            'toc': md.toc if hasattr(md, 'toc') else ''
        }, None
        
    except Exception as e:
        return None, f"讀取檔案錯誤: {str(e)}"

def get_docs_navigation():
    """獲取文檔導航結構"""
    navigation = {
        'zh-tw': {
            'title': '中文文檔',
            'items': [
                {'name': 'README', 'path': 'zh-tw/README.md', 'title': '概述'},
                {'name': 'api', 'path': 'zh-tw/api.md', 'title': 'API 文檔'},
                {'name': 'deployment', 'path': 'zh-tw/deployment.md', 'title': '部署指南'},
                {'name': 'developer', 'path': 'zh-tw/developer.md', 'title': '開發者指南'},
                {'name': 'troubleshooting', 'path': 'zh-tw/troubleshooting.md', 'title': '故障排除'},
                {
                    'name': 'modules',
                    'title': '模組文檔',
                    'children': [
                        {'name': 'README', 'path': 'zh-tw/modules/README.md', 'title': '模組概述'},
                        {'name': 'node-pool', 'path': 'zh-tw/modules/node-pool.md', 'title': '節點池'},
                        {'name': 'master-node', 'path': 'zh-tw/modules/master-node.md', 'title': 'Master 節點'},
                        {'name': 'worker-node', 'path': 'zh-tw/modules/worker-node.md', 'title': 'Worker 節點'},
                        {'name': 'web', 'path': 'zh-tw/modules/web.md', 'title': 'Web 介面'},
                        {'name': 'taskworker', 'path': 'zh-tw/modules/taskworker.md', 'title': 'TaskWorker'},
                        {'name': 'ai', 'path': 'zh-tw/modules/ai.md', 'title': 'AI 模組'},
                        {'name': 'bt', 'path': 'zh-tw/modules/bt.md', 'title': 'BT 模組'},
                    ]
                }
            ]
        },
        'en': {
            'title': 'English Documentation',
            'items': [
                {'name': 'README', 'path': 'en/README.md', 'title': 'Overview'},
                {'name': 'api', 'path': 'en/api.md', 'title': 'API Documentation'},
                {'name': 'deployment', 'path': 'en/deployment.md', 'title': 'Deployment Guide'},
                {'name': 'developer', 'path': 'en/developer.md', 'title': 'Developer Guide'},
                {
                    'name': 'modules',
                    'title': 'Module Documentation',
                    'children': [
                        {'name': 'README', 'path': 'en/modules/README.md', 'title': 'Module Overview'},
                        {'name': 'node-pool', 'path': 'en/modules/node-pool.md', 'title': 'Node Pool'},
                        {'name': 'master-node', 'path': 'en/modules/master-node.md', 'title': 'Master Node'},
                        {'name': 'worker-node', 'path': 'en/modules/worker-node.md', 'title': 'Worker Node'},
                        {'name': 'web', 'path': 'en/modules/web.md', 'title': 'Web Interface'},
                        {'name': 'taskworker', 'path': 'en/modules/taskworker.md', 'title': 'TaskWorker'},
                        {'name': 'ai', 'path': 'en/modules/ai.md', 'title': 'AI Module'},
                        {'name': 'bt', 'path': 'en/modules/bt.md', 'title': 'BT Module'},
                    ]
                }
            ]
        }
    }
    return navigation

# IP 限流字典
ip_rate_limit = defaultdict(float)
RATE_LIMIT_SECONDS = Config.RATE_LIMIT_SECONDS

def check_rate_limit(ip_address):
    """檢查 IP 是否在限流範圍內"""
    current_time = time.time()
    last_request_time = ip_rate_limit[ip_address]
    
    if current_time - last_request_time < RATE_LIMIT_SECONDS:
        return False
    
    ip_rate_limit[ip_address] = current_time
    return True

# 添加郵件發送限流字典
email_rate_limit = defaultdict(float)
EMAIL_RATE_LIMIT_SECONDS = 60  # 60秒限制（針對單個用戶）

def check_email_rate_limit(user_id):
    """檢查郵件發送是否在限流範圍內（針對單個用戶）"""
    current_time = time.time()
    last_request_time = email_rate_limit[user_id]
    
    if current_time - last_request_time < EMAIL_RATE_LIMIT_SECONDS:
        return False, int(EMAIL_RATE_LIMIT_SECONDS - (current_time - last_request_time))
    
    return True, 0

def update_email_rate_limit(user_id):
    """更新郵件發送時間（只有成功發送才調用）"""
    email_rate_limit[user_id] = time.time()

# 添加忘記密碼限流字典
forgot_password_rate_limit = defaultdict(float)
FORGOT_PASSWORD_RATE_LIMIT_SECONDS = 60  # 60秒限制

def check_forgot_password_rate_limit(email):
    """檢查忘記密碼請求是否在限流範圍內"""
    current_time = time.time()
    last_request_time = forgot_password_rate_limit[email]
    
    if current_time - last_request_time < FORGOT_PASSWORD_RATE_LIMIT_SECONDS:
        return False, int(FORGOT_PASSWORD_RATE_LIMIT_SECONDS - (current_time - last_request_time))
    
    return True, 0

def update_forgot_password_rate_limit(email):
    """更新忘記密碼請求時間（只有成功發送才調用）"""
    forgot_password_rate_limit[email] = time.time()

def call_vpn_service(endpoint, method='GET', data=None):
    """調用 VPN 服務的通用方法"""
    try:
        url = f"{VPN_SERVICE_URL}{endpoint}"
        
        if method == 'GET':
            response = requests.get(url, timeout=Config.VPN_SERVICE_TIMEOUT)
        elif method == 'POST':
            response = requests.post(url, json=data, timeout=Config.VPN_SERVICE_TIMEOUT)
        elif method == 'DELETE':
            response = requests.delete(url, timeout=Config.VPN_SERVICE_TIMEOUT)
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

def get_real_client_ip(request):
    """安全地獲取客戶端真實 IP（支援 Cloudflare + Nginx）"""
    
    # 從配置獲取信任的代理 IP 列表
    TRUSTED_PROXIES = [ip.strip() for ip in Config.TRUSTED_PROXIES if ip.strip()]
    
    # 獲取 Cloudflare IP 範圍
    CLOUDFLARE_IP_RANGES = []
    if Config.TRUST_CLOUDFLARE and Config.CLOUDFLARE_IPS:
        for ip_range in Config.CLOUDFLARE_IPS:
            ip_range = ip_range.strip()
            if ip_range:
                try:
                    CLOUDFLARE_IP_RANGES.append(ipaddress.ip_network(ip_range, strict=False))
                except ValueError:
                    continue
    
    remote_addr = request.environ.get('REMOTE_ADDR', request.remote_addr)
    
    def is_trusted_proxy(ip_str):
        """檢查 IP 是否為信任的代理"""
        if ip_str in TRUSTED_PROXIES:
            return True
        
        # 檢查是否為 Cloudflare IP
        if Config.TRUST_CLOUDFLARE:
            try:
                ip_obj = ipaddress.ip_address(ip_str)
                for cf_range in CLOUDFLARE_IP_RANGES:
                    if ip_obj in cf_range:
                        return True
            except ValueError:
                pass
        
        return False
    
    # 只有當請求來自信任的代理時，才檢查轉發頭部
    if is_trusted_proxy(remote_addr):
        # Cloudflare 提供的真實 IP 頭部（優先級順序）
        forwarded_headers = [
            'CF-Connecting-IP',      # Cloudflare 提供的真實客戶端 IP
            'X-Forwarded-For',       # 標準轉發頭部
            'X-Real-IP',             # Nginx 常用頭部
        ]
        
        for header in forwarded_headers:
            forwarded_ip = request.headers.get(header)
            if forwarded_ip:
                # 處理可能的多個 IP（取第一個）
                client_ip = forwarded_ip.split(',')[0].strip()
                # 驗證 IP 格式
                try:
                    ipaddress.ip_address(client_ip)
                    print(f"🌐 使用 {header} 獲取客戶端 IP: {client_ip}")
                    return client_ip
                except ValueError:
                    continue
    
    print(f"🔒 使用直連 IP: {remote_addr}")
    return remote_addr

def is_rate_limit_bypassed(ip_address):
    """檢查 IP 是否嘗試繞過限流（安全檢查）"""
    # 如果未啟用嚴格驗證，跳過檢查
    if not Config.STRICT_IP_VALIDATION:
        return False
    
    # 在 Cloudflare 環境下，允許某些私有 IP（如內部服務通信）
    if Config.TRUST_CLOUDFLARE:
        # 允許的內部 IP 模式（如果需要）
        allowed_internal_ips = ['127.0.0.1', '::1']
        if ip_address in allowed_internal_ips:
            return False
    
    # 檢查是否為可疑的 IP 模式
    suspicious_patterns = [
        'localhost',
        '10.0.0.',      # 私有 IP 段 A
        '192.168.',     # 私有 IP 段 C
        '172.16.',      # 私有 IP 段 B (部分)
        '172.17.',      # Docker 默認網段
        '172.18.',
        '172.19.',
        '172.20.',
    ]
    
    for pattern in suspicious_patterns:
        if ip_address.startswith(pattern):
            # 記錄可疑活動
            print(f"⚠️  檢測到可疑 IP 嘗試繞過限流: {ip_address}")
            return True
    
    return False

def verify_turnstile(token, ip_address):
    """驗證 Cloudflare Turnstile token"""
    try:
        # 僅在開發模式下自動通過，不再檢查 IP
        if Config.is_development():
            print("🚧 開發模式，自動通過 Turnstile 驗證")
            return True
        
        # 如果沒有提供 token，直接返回 False
        if not token:
            print("❌ Turnstile: 沒有提供 token")
            return False

        if not Config.TURNSTILE_SECRET_KEY:
            print("❌ Turnstile: 沒有設定 TURNSTILE_SECRET_KEY 環境變數")
            return False

        print(f"🔑 Turnstile: 使用密鑰 {Config.TURNSTILE_SECRET_KEY[:10]}...")

        response = requests.post('https://challenges.cloudflare.com/turnstile/v0/siteverify', {
            'secret': Config.TURNSTILE_SECRET_KEY,
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
        return jsonify({'error': 'User service unavailable'}), 500
    
    try:
        data = request.get_json()
        username = data.get('username')
        password = data.get('password')
        turnstile_response = data.get('cf-turnstile-response')
        
        # 獲取客戶端 IP（安全方式）
        client_ip = get_real_client_ip(request)
        
        # 驗證人機驗證
        if not turnstile_response:
            return jsonify({'error': 'Please complete human verification'}), 400
            
        if not verify_turnstile(turnstile_response, client_ip):
            return jsonify({'error': 'Human verification failed, please try again'}), 400
        
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
                    'message': 'Registration successful',
                    'access_token': login_response.token,
                    'user': {
                        'username': username,
                        'balance': 0.0
                    }
                }), 201
            else:
                return jsonify({'error': 'Registration successful but login failed'}), 500
        else:
            return jsonify({'error': response.message}), 400
            
    except Exception as e:
        return jsonify({'error': str(e)}), 500

@app.route('/api/login', methods=['POST'])
def login():
    if not user_service_obj:
        return jsonify({'error': 'User service unavailable'}), 500
    
    try:
        data = request.get_json()
        username = data.get('username')
        password = data.get('password')
        turnstile_response = data.get('cf-turnstile-response')
        
        # 獲取客戶端 IP（安全方式）
        client_ip = get_real_client_ip(request)
        
        # 驗證人機驗證
        if not turnstile_response:
            return jsonify({'error': 'Please complete human verification'}), 400
            
        if not verify_turnstile(turnstile_response, client_ip):
            return jsonify({'error': 'Human verification failed, please try again'}), 400
        
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
                'message': 'Login successful',
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
        return jsonify({'error': 'User service unavailable'}), 500
    
    try:
        auth_header = request.headers.get('Authorization', '')
        if not auth_header.startswith('Bearer '):
            return jsonify({'error': 'Invalid authentication token'}), 401
        
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
                    'error': 'Token expired',
                    'error_code': 'TOKEN_EXPIRED'
                }), 401
            else:
                return jsonify({'error': response.message}), 400
            
    except Exception as e:
        return jsonify({'error': str(e)}), 500

@app.route('/api/transfer', methods=['POST'])
def transfer():
    if not user_service_obj:
        return jsonify({'error': 'User service unavailable'}), 500
    
    try:
        auth_header = request.headers.get('Authorization', '')
        if not auth_header.startswith('Bearer '):
            return jsonify({'error': 'Invalid authentication token'}), 401
        
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
                'message': 'Transfer successful',
                'new_balance': balance_response.balance if balance_response.success else 0.0
            }), 200
        else:
            return jsonify({'error': response.message}), 400
            
    except Exception as e:
        return jsonify({'error': str(e)}), 500

@app.route('/api/vpn/join', methods=['POST'])
def join_vpn():
    # 獲取客戶端真實 IP（安全方式）
    client_ip = get_real_client_ip(request)
    
    # 檢查是否嘗試繞過限流
    if is_rate_limit_bypassed(client_ip):
        return jsonify({
            'error': '檢測到可疑請求，請使用有效的公共 IP 地址'
        }), 403
    
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
@app.route('/dashboard')
def dashboard_page():
    return render_template('dashboard.html')

@app.route('/sponsor')
def sponsor_page():
    return render_template('sponsor.html')

@app.route('/terms')
def terms_page():
    return render_template('reset_password.html')

@app.route('/forgot-password')
def forgot_password_page():
    """忘記密碼頁面"""
    return render_template('forgot_password.html')

# 文檔路由
@app.route('/docs')
def docs_index():
    """文檔首頁 - 重定向到主要文檔"""
    return redirect('/docs/md/zh-tw/README')

@app.route('/docs/')
def docs_index_slash():
    """文檔首頁 - 重定向到主要文檔"""
    return redirect('/docs/md/zh-tw/README')

@app.route('/docs/zh/')
def docs_zh_index():
    """中文文檔首頁 - 重定向到主要文檔"""
    return redirect('/docs/md/zh-tw/README')

@app.route('/docs/test')
def docs_test():
    """文檔測試頁面"""
    return render_template('docs/test_markdown.html')

@app.route('/docs/md/<path:doc_path>')
def serve_markdown_doc(doc_path):
    """動態載入 Markdown 文檔"""
    try:
        # 確保路徑安全，防止目錄遍歷攻擊
        if '..' in doc_path or doc_path.startswith('/'):
            return jsonify({'error': '無效的文檔路徑'}), 400
        
        # 添加 .md 擴展名（如果沒有的話）
        if not doc_path.endswith('.md'):
            doc_path += '.md'
        
        # 載入 Markdown 檔案
        result, error = load_markdown_file(doc_path)
        
        if error:
            return jsonify({'error': error}), 404
        
        # 獲取導航結構
        navigation_data = get_docs_navigation()
        
        # 確定語言
        lang = 'zh-tw' if doc_path.startswith('zh-tw/') else 'en'
        
        # 添加麵包屑導航
        breadcrumbs = generate_breadcrumbs(doc_path)
        
        # 確保傳遞正確的導航數據
        nav_data = navigation_data.get(lang, {})
        
        # 調試信息
        print(f"Debug: doc_path={doc_path}, lang={lang}")
        print(f"Debug: nav_data type={type(nav_data)}")
        print(f"Debug: nav_data keys={list(nav_data.keys()) if isinstance(nav_data, dict) else 'Not a dict'}")
        if isinstance(nav_data, dict) and 'items' in nav_data:
            print(f"Debug: nav_data.items type={type(nav_data['items'])}")
        
        return render_template('docs/markdown_viewer.html', 
                             content=result['content'],
                             meta=result.get('meta', {}),
                             toc=result.get('toc', ''),
                             doc_path=doc_path,
                             navigation=nav_data,
                             current_lang=lang,
                             breadcrumbs=breadcrumbs)
    
    except Exception as e:
        print(f"Error in serve_markdown_doc: {e}")
        import traceback
        traceback.print_exc()
        return jsonify({
            'error': f'服務器錯誤: {str(e)}',
            'doc_path': doc_path
        }), 500

# 生成麵包屑導航
def generate_breadcrumbs(doc_path):
    """生成麵包屑導航"""
    try:
        parts = doc_path.replace('.md', '').split('/')
        breadcrumbs = []
        
        for i, part in enumerate(parts):
            if i == 0:  # 語言
                breadcrumbs.append({
                    'name': '中文文檔' if part == 'zh-tw' else 'English Docs',
                    'url': f'/docs/md/{part}/README'
                })
            elif part == 'modules':
                breadcrumbs.append({
                    'name': '模組文檔' if parts[0] == 'zh-tw' else 'Modules',
                    'url': f'/docs/md/{"/".join(parts[:i+1])}/README'
                })
            else:
                breadcrumbs.append({
                    'name': part.replace('-', ' ').title(),
                    'url': f'/docs/md/{"/".join(parts[:i+1])}'
                })
        
        return breadcrumbs
    except Exception as e:
        print(f"Error generating breadcrumbs: {e}")
        return []

def search_documentation(query, lang='zh-tw'):
    """搜索文檔內容"""
    results = []
    try:
        search_paths = [
            os.path.join(DOCS_BASE_PATH, lang),
        ]
        
        for search_path in search_paths:
            if not os.path.exists(search_path):
                continue
                
            for root, dirs, files in os.walk(search_path):
                for file in files:
                    if file.endswith('.md'):
                        file_path = os.path.join(root, file)
                        try:
                            with open(file_path, 'r', encoding='utf-8') as f:
                                content = f.read().lower()
                                
                            if query.lower() in content:
                                rel_path = os.path.relpath(file_path, DOCS_BASE_PATH)
                                
                                # 提取標題
                                lines = content.split('\n')
                                title = file.replace('.md', '').replace('-', ' ').title()
                                for line in lines[:10]:
                                    if line.startswith('# '):
                                        title = line[2:].strip()
                                        break
                                
                                # 提取摘要
                                excerpt = ''
                                content_lines = content.split('\n')
                                for line in content_lines:
                                    if query.lower() in line.lower() and len(line.strip()) > 10:
                                        excerpt = line.strip()[:200] + '...'
                                        break
                                
                                results.append({
                                    'title': title,
                                    'path': rel_path.replace('\\', '/'),
                                    'url': f'/docs/md/{rel_path.replace("\\", "/")}',
                                    'excerpt': excerpt
                                })
                                
                        except Exception:
                            continue
    except Exception as e:
        print(f"Error in search_documentation: {e}")
    
    return results[:10]  # 限制結果數量

# ...existing code...

@app.route('/api/user/profile', methods=['GET'])
def get_user_profile():
    """獲取用戶資料"""
    if not user_service_obj:
        return jsonify({'error': 'User service unavailable'}), 500
    
    try:
        auth_header = request.headers.get('Authorization', '')
        if not auth_header.startswith('Bearer '):
            return jsonify({'error': 'Invalid authentication token'}), 401
        
        token = auth_header[7:]
        
        # 驗證 token 並獲取用戶信息
        user_info = user_service_obj.verify_token(token)
        if not user_info:
            return jsonify({'error': 'Invalid authentication token'}), 401
        
        # 從數據庫獲取完整用戶資料
        user_id = user_info.get('user_id')
        user_data = user_service_obj.user_manager.db_manager.get_user_by_id(user_id)
        
        if user_data:
            user_dict = dict(user_data) if user_data else {}
            
            return jsonify({
                'username': user_dict.get('username', ''),
                'email': user_dict.get('email'),
                'email_verified': bool(user_dict.get('email_verified', 0)),
                'tokens': user_dict.get('tokens', 0),
                'credit_score': user_dict.get('credit_score', 100),
                'created_at': user_dict.get('created_at')
            }), 200
        else:
            return jsonify({'error': 'User not found'}), 404
            
    except Exception as e:
        print(f"Error in get_user_profile: {e}")  # 添加調試信息
        return jsonify({'error': str(e)}), 500

@app.route('/api/user/update-email', methods=['POST'])
def update_user_email():
    """更新用戶電子郵件"""
    if not user_service_obj:
        return jsonify({'error': 'User service unavailable'}), 500
    
    try:
        auth_header = request.headers.get('Authorization', '')
        if not auth_header.startswith('Bearer '):
            return jsonify({'error': 'Invalid authentication token'}), 401
        
        token = auth_header[7:]
        data = request.get_json()
        email = data.get('email', '').strip()
        
        if not email:
            return jsonify({'error': 'Please enter a valid email address'}), 400
        
        # 驗證 token 並獲取用戶信息
        user_info = user_service_obj.verify_token(token)
        if not user_info:
            return jsonify({'error': 'Invalid authentication token'}), 401
        
        user_id = user_info.get('user_id')
        
        # 獲取用戶當前的電子郵件
        current_user = user_service_obj.user_manager.db_manager.get_user_by_id(user_id)
        if not current_user:
            return jsonify({'error': 'User not found'}), 404
        
        current_user_dict = dict(current_user)
        current_email = current_user_dict.get('email')
        
        # 檢查新電子郵件是否與當前電子郵件相同
        if current_email and current_email.lower() == email.lower():
            return jsonify({'error': 'The new email address is the same as your current email address'}), 400
        
        # 檢查郵件發送限流（針對單個用戶）
        can_send, wait_time = check_email_rate_limit(user_id)
        if not can_send:
            return jsonify({
                'error': f'Please wait {wait_time} seconds before sending another verification email'
            }), 429
        
        # 檢查電子郵件是否已被其他用戶使用
        existing_user = user_service_obj.user_manager.db_manager.get_user_by_email(email)
        if existing_user and existing_user['id'] != user_id:
            return jsonify({'error': 'This email is already used by another user'}), 400
        
        # 更新電子郵件並發送驗證郵件
        success, message = user_service_obj.user_manager.db_manager.update_user_email_and_send_verification(user_id, email)
        
        if success:
            # 只有成功發送郵件才更新限流時間
            update_email_rate_limit(user_id)
            return jsonify({'message': message}), 200
        else:
            # 發送失敗不計入限流，用戶可以立即重試
            return jsonify({'error': message}), 400
            
    except Exception as e:
        print(f"Error in update_user_email: {e}")  # 添加調試信息
        return jsonify({'error': str(e)}), 500

@app.route('/api/user/change-password', methods=['POST'])
def change_user_password():
    """變更用戶密碼"""
    if not user_service_obj:
        return jsonify({'error': 'User service unavailable'}), 500
    
    try:
        auth_header = request.headers.get('Authorization', '')
        if not auth_header.startswith('Bearer '):
            return jsonify({'error': 'Invalid authentication token'}), 401
        
        token = auth_header[7:]
        data = request.get_json()
        current_password = data.get('current_password', '')
        new_password = data.get('new_password', '')
        
        if not current_password or not new_password:
            return jsonify({'error': 'Please enter current password and new password'}), 400
        
        if len(new_password) < 6:
            return jsonify({'error': 'Password must be at least 6 characters long'}), 400
        
        # 驗證 token 並獲取用戶信息
        user_info = user_service_obj.verify_token(token)
        if not user_info:
            return jsonify({'error': 'Invalid authentication token'}), 401
        
        user_id = user_info.get('user_id')
        
        # 驗證目前密碼
        user_data = user_service_obj.user_manager.db_manager.get_user_by_id(user_id)
        if not user_data:
            return jsonify({'error': 'User not found'}), 404
            
        # 將 sqlite3.Row 轉換為字典
        user_dict = dict(user_data)
        
        if not bcrypt.checkpw(current_password.encode(), user_dict['password'].encode()):
            return jsonify({'error': 'Current password is incorrect'}), 400
        
        # 更新密碼
        success, message = user_service_obj.user_manager.update_user_password(user_id, new_password)
        
        if success:
            return jsonify({'message': 'Password updated successfully'}), 200
        else:
            return jsonify({'error': message}), 400
            
    except Exception as e:
        print(f"Error in change_user_password: {e}")  # 添加調試信息
        return jsonify({'error': str(e)}), 500

@app.route('/api/user/verify-email/<token>', methods=['GET'])
def verify_email(token):
    """驗證電子郵件"""
    if not user_service_obj:
        return jsonify({'error': 'User service unavailable'}), 500
    
    try:
        success, message = user_service_obj.user_manager.db_manager.verify_user_email(token)
        
        if success:
            return jsonify({'message': message}), 200
        else:
            return jsonify({'error': message}), 400
            
    except Exception as e:
        return jsonify({'error': str(e)}), 500

@app.route('/api/user/forgot-password', methods=['POST'])
def forgot_password():
    """發送密碼重設郵件"""
    if not user_service_obj:
        return jsonify({'error': 'User service unavailable'}), 500
    
    try:
        data = request.get_json()
        email = data.get('email', '').strip()
        
        if not email:
            return jsonify({'error': 'Please enter your email address'}), 400
        
        # 檢查忘記密碼限流（針對單個郵件地址）
        can_send, wait_time = check_forgot_password_rate_limit(email)
        if not can_send:
            return jsonify({
                'error': f'Please wait {wait_time} seconds before requesting another password reset email'
            }), 429
        
        # 創建密碼重設 token 並發送郵件
        success, message = user_service_obj.user_manager.db_manager.create_password_reset_token(email)
        
        if success:
            # 只有成功發送郵件才更新限流時間
            update_forgot_password_rate_limit(email)
            return jsonify({'message': message}), 200
        else:
            # 發送失敗不計入限流，用戶可以立即重試
            return jsonify({'error': message}), 400
            
    except Exception as e:
        print(f"Error in forgot_password: {e}")
        return jsonify({'error': str(e)}), 500

@app.route('/api/user/reset-password', methods=['POST'])
def reset_password():
    """使用 token 重設密碼"""
    if not user_service_obj:
        return jsonify({'error': 'User service unavailable'}), 500
    
    try:
        data = request.get_json()
        token = data.get('token', '')
        new_password = data.get('new_password', '')
        confirm_password = data.get('confirm_password', '')
        
        if not token or not new_password or not confirm_password:
            return jsonify({'error': 'All fields are required'}), 400
        
        if new_password != confirm_password:
            return jsonify({'error': 'Passwords do not match'}), 400
        
        if len(new_password) < 6:
            return jsonify({'error': 'Password must be at least 6 characters long'}), 400
        
        # 重設密碼
        success, message = user_service_obj.user_manager.db_manager.reset_password_with_token(token, new_password)
        
        if success:
            return jsonify({'message': message}), 200
        else:
            return jsonify({'error': message}), 400
            
    except Exception as e:
        print(f"Error in reset_password: {e}")
        return jsonify({'error': str(e)}), 500

if __name__ == '__main__':
    print(f"啟動 Flask 應用程式:")
    print(f"  - 主機: {Config.FLASK_HOST}")
    print(f"  - 端口: {Config.FLASK_PORT}")
    print(f"  - 調試模式: {Config.FLASK_DEBUG}")
    print(f"  - 環境: {Config.FLASK_ENV}")
    print(f"  - VPN 服務 URL: {VPN_SERVICE_URL}")
    print(f"  - 限流設置: {Config.RATE_LIMIT_SECONDS} 秒")
    
    app.run(
        debug=Config.FLASK_DEBUG, 
        host=Config.FLASK_HOST, 
        port=Config.FLASK_PORT
    )