from flask import Flask, request, jsonify, render_template, send_file
from flask_cors import CORS
from datetime import datetime, timedelta
import uuid
import os
import sys
sys.path.append(os.path.abspath(os.path.join(os.path.dirname(__file__), '..')))
from 節點池 import user_service
from dotenv import load_dotenv

load_dotenv()

app = Flask(__name__)
app.config['SECRET_KEY'] = os.getenv('SECRET_KEY', 'your-secret-key-here')
CORS(app)

# 初始化 user_service 物件
user_service_obj = user_service.UserServiceServicer()

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
    if not user_service_obj:
        return jsonify({'error': '用戶服務不可用'}), 500
    
    try:
        auth_header = request.headers.get('Authorization', '')
        if not auth_header.startswith('Bearer '):
            return jsonify({'error': '無效的認證令牌'}), 401
        
        token = auth_header[7:]
        
        # 驗證用戶token
        user_info = user_service_obj.verify_token(token)
        if not user_info:
            return jsonify({'error': '無效的認證令牌'}), 401
        
        user_id = user_info['user_id']
        
        # 生成WireGuard配置
        vpn_config = f"""[Interface]
PrivateKey = {str(uuid.uuid4()).replace('-', '')[:44]}=
Address = 10.0.0.{user_id}/24
DNS = 8.8.8.8, 1.1.1.1

[Peer]
PublicKey = SERVER_PUBLIC_KEY_PLACEHOLDER
Endpoint = vpn.hivemind.com:51820
AllowedIPs = 0.0.0.0/0
PersistentKeepalive = 25
"""
        
        return jsonify({
            'message': 'VPN配置生成成功',
            'config': vpn_config
        }), 200
        
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
        
        # 驗證用戶token
        user_info = user_service_obj.verify_token(token)
        if not user_info:
            return jsonify({'error': '無效的認證令牌'}), 401
        
        if file_type == 'client':
            return jsonify({
                'download_url': '/static/downloads/hivemind-client.zip',
                'version': '1.0.0',
                'size': '25.6 MB'
            }), 200
        elif file_type == 'vpn-config':
            user_id = user_info['user_id']
            
            # 生成VPN配置
            vpn_config = f"""[Interface]
PrivateKey = {str(uuid.uuid4()).replace('-', '')[:44]}=
Address = 10.0.0.{user_id}/24
DNS = 8.8.8.8, 1.1.1.1

[Peer]
PublicKey = SERVER_PUBLIC_KEY_PLACEHOLDER
Endpoint = vpn.hivemind.com:51820
AllowedIPs = 0.0.0.0/0
PersistentKeepalive = 25
"""
            return jsonify({
                'config': vpn_config
            }), 200
        else:
            return jsonify({'error': '文件類型不存在'}), 404
            
    except Exception as e:
        return jsonify({'error': str(e)}), 500

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
    app.run(debug=True, host='0.0.0.0', port=5000)
