from flask import Flask, request, jsonify, send_from_directory
import grpc
import nodepool_pb2
import nodepool_pb2_grpc
import os

app = Flask(__name__)

def get_grpc_client():
    options = [
        ('grpc.keepalive_time_ms', 10000),
        ('grpc.keepalive_timeout_ms', 5000),
        ('grpc.keepalive_permit_without_calls', True),
        ('grpc.http2.max_pings_without_data', 0),
        ('grpc.http2.min_time_between_pings_ms', 10000),
        ('grpc.http2.min_ping_interval_without_data_ms', 300000)
    ]
    channel = grpc.insecure_channel('localhost:50051', options=options)
    return nodepool_pb2_grpc.UserServiceStub(channel)

@app.route('/login', methods=['POST'])
def login():
    data = request.json
    username = data.get('username')
    password = data.get('password')
    print(f"登入嘗試: 用戶名={username}")
    try:
        grpc_client = get_grpc_client()
        print("gRPC 客戶端已創建")
        response = grpc_client.Login(
            nodepool_pb2.LoginRequest(username=username, password=password),
            timeout=5.0
        )
        print(f"gRPC 響應: success={response.success}, message={response.message}")
        if response.success:
            return jsonify({"message": "Login successful", "token": response.token})
        else:
            return jsonify({"message": "帳號或密碼錯誤"}), 401
    except grpc.RpcError as e:
        print(f"gRPC 錯誤: {e.code()}, {e.details()}")
        return jsonify({"message": f"gRPC 錯誤: {e.details()}"}), 500
    except Exception as e:
        print(f"一般錯誤: {str(e)}")
        return jsonify({"message": f"登入失敗: {str(e)}"}), 500

@app.route('/register', methods=['POST'])
def register():
    data = request.json
    username = data.get('username')
    password = data.get('password')
    return jsonify({"message": "Registration successful"})

@app.route('/balance', methods=['GET'])
def get_balance():
    username = request.args.get('user_id')
    token = request.args.get('token', '')
    grpc_client = get_grpc_client()
    response = grpc_client.GetBalance(nodepool_pb2.GetBalanceRequest(username=username, token=token))
    return jsonify({"balance": response.balance})

@app.route('/transfer', methods=['POST'])
def transfer():
    data = request.json
    token = data.get('token', '')
    receiver_username = data.get('to_user_id')
    amount = int(data.get('amount', 0))
    grpc_client = get_grpc_client()
    response = grpc_client.Transfer(nodepool_pb2.TransferRequest(
        token=token,
        receiver_username=receiver_username,
        amount=amount
    ))
    return jsonify({"message": response.message})

@app.route('/static/<path:filename>')
def static_files(filename):
    return send_from_directory(os.path.join(os.path.dirname(__file__), 'static'), filename)

@app.route('/')
def index():
    return send_from_directory(os.path.dirname(__file__), 'index.html')

@app.route('/login.html')
def login_html():
    return send_from_directory(os.path.dirname(__file__), 'login.html')

@app.route('/register.html')
def register_html():
    return send_from_directory(os.path.dirname(__file__), 'register.html')

@app.route('/dashboard.html')
def dashboard_html():
    return send_from_directory(os.path.dirname(__file__), 'dashboard.html')

@app.route('/download.html')
def download_html():
    return send_from_directory(os.path.dirname(__file__), 'download.html')

if __name__ == '__main__':
    app.run(debug=False, host='0.0.0.0', port=5000)