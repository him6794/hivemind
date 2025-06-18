import grpc
import os
import time
import logging
import zipfile
import datetime
from concurrent.futures import ThreadPoolExecutor, as_completed
import nodepool_pb2
import nodepool_pb2_grpc
import threading
from flask import Flask, jsonify, request, render_template, redirect, url_for, flash
import io
from functools import wraps
import uuid

# --- Configuration ---
GRPC_SERVER_ADDRESS = os.environ.get('GRPC_SERVER_ADDRESS', '127.0.0.1:50051')
FLASK_SECRET_KEY = os.environ.get('FLASK_SECRET_KEY', 'a-default-master-secret-key')
MASTER_USERNAME = os.environ.get('MASTER_USERNAME', 'test')
MASTER_PASSWORD = os.environ.get('MASTER_PASSWORD', 'password')
UI_HOST = '0.0.0.0'
UI_PORT = 5001
logging.basicConfig(level=logging.INFO, format='%(asctime)s - %(levelname)s - [%(threadName)s] - %(message)s')

class MasterNodeUI:
    def __init__(self, username, password, grpc_address):
        self.username = username
        self.password = password
        self.grpc_address = grpc_address
        self.channel = None
        self.user_stub = None
        self.master_stub = None
        self.node_stub = None
        self.token = None
        self._stop_event = threading.Event()
        self.task_status_cache = {}
        self.task_cache_lock = threading.Lock()

        self.app = Flask(__name__, template_folder="templates_master", static_folder="static_master")
        self.app.secret_key = FLASK_SECRET_KEY
        self.setup_flask_routes()
        
        # 用戶會話管理
        self.user_list = []  # [{username, token, cpt_balance, login_time}]
        self.user_list_lock = threading.Lock()

    def add_or_update_user(self, username, token):
        with self.user_list_lock:
            for user in self.user_list:
                if user['username'] == username:
                    user['token'] = token
                    user['login_time'] = datetime.datetime.now()
                    return
            self.user_list.append({
                'username': username,
                'token': token,
                'cpt_balance': 0,
                'login_time': datetime.datetime.now()
            })

    def get_user(self, username):
        with self.user_list_lock:
            for user in self.user_list:
                if user['username'] == username:
                    return user
        return None

    def remove_user(self, username):
        with self.user_list_lock:
            self.user_list = [u for u in self.user_list if u['username'] != username]

    def _connect_grpc(self):
        try:
            self.channel = grpc.insecure_channel(self.grpc_address)
            grpc.channel_ready_future(self.channel).result(timeout=10)
            self.user_stub = nodepool_pb2_grpc.UserServiceStub(self.channel)
            self.master_stub = nodepool_pb2_grpc.MasterNodeServiceStub(self.channel)
            self.node_stub = nodepool_pb2_grpc.NodeManagerServiceStub(self.channel)
            logging.info(f"Successfully connected to gRPC server at {self.grpc_address}")
            return True
        except Exception as e:
            logging.error(f"Failed to connect to gRPC server: {e}")
            return False

    def login(self, username=None, password=None):
        username = username or self.username
        password = password or self.password
        if not self.channel or not self.user_stub:
            logging.error("gRPC connection not established. Cannot login.")
            return False

        request = nodepool_pb2.LoginRequest(username=username, password=password)
        try:
            response = self.user_stub.Login(request, timeout=15)
            if response.success and response.token:
                self.add_or_update_user(username, response.token)
                if username == self.username:
                    self.token = response.token
                logging.info(f"User {username} logged in successfully")
                return True
            else:
                return False
        except Exception as e:
            logging.error(f"Login error: {e}")
            return False

    def get_balance(self, username):
        user = self.get_user(username)
        if not user:
            return 0
        try:
            req = nodepool_pb2.GetBalanceRequest(username=username, token=user['token'])
            resp = self.user_stub.GetBalance(req, timeout=30)
            if resp.success:
                user['cpt_balance'] = resp.balance
                return resp.balance
            else:
                return 0
        except Exception:
            return 0

    def get_tasks(self, username):
        user = self.get_user(username)
        if not user:
            return []
        try:
            req = nodepool_pb2.GetAllTasksRequest(token=user['token'])
            resp = self.master_stub.GetAllTasks(req, timeout=30)
            if resp.success:
                return resp.tasks
            else:
                return []
        except Exception:
            return []

    def upload_task_with_user(self, username, task_id, task_zip_bytes, requirements):
        user = self.get_user(username)
        if not user:
            logging.error(f"找不到用戶 {username}，無法上傳任務")
            return task_id, False
        token = user['token']

        try:
            # 檢查用戶餘額
            balance_request = nodepool_pb2.GetBalanceRequest(username=username, token=token)
            balance_response = self.user_stub.GetBalance(balance_request, timeout=30)
            if balance_response.success:
                user['cpt_balance'] = balance_response.balance
                # 簡化成本計算
                memory_gb_val = float(requirements.get("memory_gb", 0))
                cpu_score_val = float(requirements.get("cpu_score", 0))
                gpu_score_val = float(requirements.get("gpu_score", 0))
                gpu_memory_gb_val = float(requirements.get("gpu_memory_gb", 0))
                cpt_cost = max(1, int(memory_gb_val + cpu_score_val / 100 + gpu_score_val / 100 + gpu_memory_gb_val))
                
                if balance_response.balance < cpt_cost:
                    logging.error(f"用戶 {username} 餘額不足: 需要 {cpt_cost} CPT，但只有 {balance_response.balance} CPT")
                    return task_id, False
            else:
                logging.error(f"無法獲取用戶 {username} 餘額")
                return task_id, False

            request = nodepool_pb2.UploadTaskRequest(
                task_id=task_id,
                task_zip=task_zip_bytes,
                memory_gb=int(requirements.get("memory_gb", 0)),
                cpu_score=int(requirements.get("cpu_score", 0)),
                gpu_score=int(requirements.get("gpu_score", 0)),
                gpu_memory_gb=int(requirements.get("gpu_memory_gb", 0)),
                location=requirements.get("location", "Any"),
                gpu_name=requirements.get("gpu_name", ""),
                user_id=username
            )
            metadata = [('authorization', f'Bearer {token}')]
            response = self.master_stub.UploadTask(request, metadata=metadata, timeout=60)
            
            if response.success:
                logging.info(f"Task {task_id} uploaded successfully")
                with self.task_cache_lock:
                    self.task_status_cache[task_id] = {
                        "task_id": task_id,
                        "status": "PENDING",
                        "message": "Task submitted",
                        "last_polled": time.time()
                    }
                return task_id, True
            else:
                logging.error(f"Task {task_id} upload failed: {response.message}")
                return task_id, False
        except Exception as e:
            logging.error(f"Task {task_id} upload error: {e}")
            return task_id, False

    def setup_flask_routes(self):
        @self.app.route('/login', methods=['GET', 'POST'])
        def login():
            if request.method == 'POST':
                username = request.form.get('username')
                password = request.form.get('password')
                if not self.channel or not self.user_stub:
                    flash('主控端未連接到節點池，請稍後再試。', 'error')
                    return render_template('login.html')
                if self.login(username, password):
                    flash('Login successful!', 'success')
                    return redirect(url_for('index') + f"?user={username}")
                else:
                    flash('Invalid username or password', 'error')
            return render_template('login.html')

        @self.app.route('/logout')
        def logout():
            username = request.args.get('user')
            if username:
                self.remove_user(username)
            flash('You have been logged out.', 'success')
            return redirect(url_for('login'))

        @self.app.route('/')
        def index():
            username = request.args.get('user')
            if not username or not self.get_user(username):
                return redirect(url_for('login'))
            return render_template('master_dashboard.html', username=username)

        @self.app.route('/api/balance')
        def api_balance():
            username = request.args.get('user')
            if not username or not self.get_user(username):
                return jsonify({"error": "請先登入", "cpt_balance": 0}), 401
            balance = self.get_balance(username)
            return jsonify({"cpt_balance": balance})

        @self.app.route('/api/tasks')
        def api_tasks():
            username = request.args.get('user')
            if not username or not self.get_user(username):
                return jsonify({"error": "請先登入", "tasks": []}), 401
            tasks = self.get_tasks(username)
            task_list = []
            for task in tasks:
                created_time = ""
                if getattr(task, "created_at", None):
                    try:
                        created_timestamp = float(task.created_at)
                        created_time = time.strftime('%H:%M:%S', time.localtime(created_timestamp))
                    except:
                        created_time = "未知"
                task_list.append({
                    "task_id": task.task_id,
                    "status": task.status,
                    "progress": "100%" if task.status == "COMPLETED" else "50%" if task.status == "RUNNING" else "0%",
                    "message": f"狀態: {task.status}",
                    "last_update": created_time
                })
            return jsonify({"tasks": task_list})

        @self.app.route('/api/nodes')
        def api_nodes():
            username = request.args.get('user')
            if not username or not self.get_user(username):
                return jsonify({"error": "請先登入", "nodes": []}), 401
            if not self.node_stub:
                return jsonify({"error": "Not connected to gRPC server", "nodes": []}), 200
            try:
                grpc_request = nodepool_pb2.GetNodeListRequest()
                response = self.node_stub.GetNodeList(grpc_request, timeout=30)
                if response.success:
                    nodes_list = []
                    for node in response.nodes:
                        status = "ONLINE" if node.status else "OFFLINE"
                        nodes_list.append({
                            "node_id": node.node_id,
                            "status": status,
                            "cpu_cores": node.cpu_cores,
                            "memory_gb": node.memory_gb,
                            "cpu_score": node.cpu_score,
                            "gpu_score": node.gpu_score,
                            "last_heartbeat": time.strftime('%H:%M:%S', time.localtime(int(node.last_heartbeat))) if node.last_heartbeat else 'N/A',
                        })
                    return jsonify({"nodes": nodes_list})
                else:
                    return jsonify({"error": f"Failed to get node list: {response.message}", "nodes": []}), 200
            except Exception as e:
                logging.error(f"API GetNodeList error: {e}")
                return jsonify({"error": "獲取節點列表失敗", "nodes": []}), 200

        @self.app.route('/upload', methods=['GET', 'POST'])
        def upload_task_ui():
            username = request.args.get('user')
            if not username:
                flash('需要用戶參數', 'error')
                return redirect(url_for('login'))
            
            user = self.get_user(username)
            if not user:
                flash('用戶未登入，請重新登入', 'error')
                return redirect(url_for('login'))
            
            if request.method == 'POST':
                if 'task_zip' not in request.files:
                    flash('請選擇 ZIP 檔案', 'error')
                    return redirect(request.url)
                    
                file = request.files['task_zip']
                if file.filename == '':
                    flash('未選擇檔案', 'error')
                    return redirect(request.url)
                    
                if file and file.filename.endswith('.zip'):
                    # 生成任務ID
                    timestamp = datetime.datetime.now().strftime("%Y%m%d_%H%M%S")
                    task_uuid = str(uuid.uuid4())[:8]
                    task_id = f"task_{timestamp}_{task_uuid}"
                    
                    # 獲取需求參數
                    requirements = {
                        "memory_gb": request.form.get('memory_gb', 0),
                        "cpu_score": request.form.get('cpu_score', 0),
                        "gpu_score": request.form.get('gpu_score', 0),
                        "gpu_memory_gb": request.form.get('gpu_memory_gb', 0),
                        "location": request.form.get('location', 'Any'),
                        "gpu_name": request.form.get('gpu_name', '')
                    }
                    
                    # 讀取檔案並上傳
                    task_zip_bytes = file.read()
                    _, success = self.upload_task_with_user(username, task_id, task_zip_bytes, requirements)
                    
                    if success:
                        flash(f'任務 "{task_id}" 上傳成功！', 'success')
                    else:
                        flash(f'任務 "{task_id}" 上傳失敗。', 'error')
                    
                    return redirect(url_for('index') + f"?user={username}")
                else:
                    flash('檔案格式無效，請上傳 .zip 檔案', 'error')
                    return redirect(request.url)
            
            return render_template('master_upload.html', username=username)

        @self.app.route('/api/stop_task/<task_id>', methods=['POST'])
        def api_stop_task(task_id):
            username = request.args.get('user')
            user = self.get_user(username)
            if not user:
                return jsonify({"success": False, "error": "用戶未登入"}), 401
            
            try:
                logging.info(f"用戶 {username} 請求停止任務 {task_id}")
                
                req = nodepool_pb2.StopTaskRequest(task_id=task_id, token=user['token'])
                # 適當的超時時間，節點池會處理狀態更新
                response = self.master_stub.StopTask(req, timeout=60)
                
                if response.success:
                    with self.task_cache_lock:
                        if task_id in self.task_status_cache:
                            self.task_status_cache[task_id].update({
                                "status": "STOPPED",
                                "last_polled": time.time()
                            })
                    
                    logging.info(f"任務 {task_id} 停止成功")
                    return jsonify({
                        "success": True,
                        "message": f"任務 {task_id} 已成功停止，工作端正在打包部分結果",
                        "note": "停止的任務仍會打包部分結果供下載"
                    })
                else:
                    logging.warning(f"節點池拒絕停止任務 {task_id}: {response.message}")
                    return jsonify({
                        "success": False,
                        "error": f"停止任務失敗: {response.message}"
                    }), 400
                    
            except grpc.RpcError as e:
                logging.error(f"停止任務 {task_id} gRPC 錯誤: {e.code()} - {e.details()}")
                return jsonify({
                    "success": False,
                    "error": f"通信錯誤: {e.details()}"
                }), 500
            except Exception as e:
                logging.error(f"停止任務 {task_id} 失敗: {e}")
                return jsonify({"success": False, "error": f"內部錯誤: {str(e)}"}), 500

        @self.app.route('/api/task_logs/<task_id>')
        def api_task_logs(task_id):
            username = request.args.get('user')
            user = self.get_user(username)
            if not user:
                return jsonify({"error": "用戶未登入"}), 401
            
            try:
                req = nodepool_pb2.GetTaskLogsRequest(task_id=task_id, token=user['token'])
                response = self.master_stub.GetTaskLogs(req, timeout=10)
                
                if response.success:
                    formatted_logs = []
                    if response.logs:
                        for line in response.logs.split('\n'):
                            if line.strip():
                                timestamp, content, level = self._parse_log_line(line)
                                formatted_logs.append({
                                    "timestamp": timestamp,
                                    "content": content,
                                    "level": level
                                })
                    
                    # 獲取任務狀態
                    status_request = nodepool_pb2.PollTaskStatusRequest(task_id=task_id)
                    status_response = self.master_stub.PollTaskStatus(status_request, timeout=10)
                    
                    return jsonify({
                        "task_id": task_id,
                        "status": status_response.status if status_response else "UNKNOWN",
                        "message": response.message,
                        "logs": formatted_logs,
                        "total_logs": len(formatted_logs)
                    })
                else:
                    return jsonify({"error": response.message, "logs": []}), 404
            except Exception as e:
                logging.error(f"獲取任務日誌失敗: {e}")
                return jsonify({"error": f"獲取日誌失敗: {str(e)}"}), 500

        @self.app.route('/api/download_result/<task_id>')
        def api_download_result(task_id):
            username = request.args.get('user')
            if not username:
                return jsonify({"error": "缺少用戶參數"}), 400
                
            user = self.get_user(username)
            if not user:
                return jsonify({"error": "用戶未登入或會話已過期"}), 401
            
            try:
                logging.info(f"用戶 {username} 請求下載任務 {task_id} 的結果")
                
                req = nodepool_pb2.GetTaskResultRequest(task_id=task_id, token=user['token'])
                response = self.master_stub.GetTaskResult(req, timeout=60)
                
                if response.success and response.result_zip:
                    from flask import Response
                    
                    # 檢查結果是否為空
                    if len(response.result_zip) == 0:
                        return jsonify({"error": "任務結果為空"}), 404
                    
                    def generate():
                        yield response.result_zip
                    
                    filename = f"{task_id}_result.zip"
                    logging.info(f"開始下載任務 {task_id} 結果，檔案大小: {len(response.result_zip)} bytes")
                    
                    return Response(
                        generate(),
                        mimetype='application/zip',
                        headers={
                            'Content-Disposition': f'attachment; filename="{filename}"',
                            'Content-Length': str(len(response.result_zip)),
                            'Cache-Control': 'no-cache'
                        }
                    )
                else:
                    error_msg = response.message if hasattr(response, 'message') else "無法獲取任務結果"
                    logging.warning(f"下載任務 {task_id} 失敗: {error_msg}")
                    return jsonify({"error": error_msg}), 404
                    
            except grpc.RpcError as e:
                logging.error(f"下載任務結果 gRPC 錯誤: {e.code()} - {e.details()}")
                return jsonify({"error": f"伺服器通信錯誤: {e.details()}"}), 500
            except Exception as e:
                logging.error(f"下載任務結果失敗: {e}", exc_info=True)
                return jsonify({"error": f"下載失敗: {str(e)}"}), 500

    def _parse_log_line(self, line):
        """簡化的日誌解析"""
        import re
        timestamp_pattern = r'\[(\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2})\]'
        timestamp_match = re.search(timestamp_pattern, line)
        
        if timestamp_match:
            timestamp = timestamp_match.group(1)
            content = re.sub(timestamp_pattern, '', line, count=1).strip()
        else:
            timestamp = time.strftime('%Y-%m-%d %H:%M:%S')
            content = line
        
        # 簡化級別檢測
        level = "info"
        content_upper = content.upper()
        if "ERROR" in content_upper or "FAILED" in content_upper:
            level = "error"
        elif "WARNING" in content_upper or "WARN" in content_upper:
            level = "warning"
        
        return timestamp, content, level

    def run(self):
        if not self._connect_grpc():
            logging.error("無法連接到節點池，退出")
            return

        if not self.login():
            logging.error("主控端登錄失敗，退出")
            return

        try:
            logging.info(f"主控端啟動在 http://{UI_HOST}:{UI_PORT}")
            self.app.run(host=UI_HOST, port=UI_PORT, debug=False)
        except KeyboardInterrupt:
            logging.info("收到中斷信號，正在關閉...")
        finally:
            if self.channel:
                self.channel.close()

if __name__ == "__main__":
    master_ui = MasterNodeUI(MASTER_USERNAME, MASTER_PASSWORD, GRPC_SERVER_ADDRESS)
    try:
        master_ui.run()
    except KeyboardInterrupt:
        logging.info("收到中斷信號，正在關閉...")
    except Exception as e:
        logging.error(f"主控端運行時發生錯誤: {e}")
    finally:
        if master_ui.channel:
            master_ui.channel.close()
        logging.info("主控端已關閉")
