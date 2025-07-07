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
import requests  # 新增

# --- Configuration ---
GRPC_SERVER_ADDRESS = os.environ.get('GRPC_SERVER_ADDRESS', '10.0.0.1:50051')
FLASK_SECRET_KEY = os.environ.get('FLASK_SECRET_KEY', 'a-default-master-secret-key')
# 移除預設的用戶名和密碼
MASTER_USERNAME = os.environ.get('MASTER_USERNAME')  # 不設默認值
MASTER_PASSWORD = os.environ.get('MASTER_PASSWORD')  # 不設默認值
UI_HOST = '0.0.0.0'
UI_PORT = 5001
logging.basicConfig(level=logging.INFO, format='%(asctime)s - %(levelname)s - [%(threadName)s] - %(message)s')

class MasterNodeUI:
    def __init__(self, grpc_address):  # 移除預設用戶名密碼參數
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

    def login(self, username, password):  # 移除默認參數
        if not self.channel or not self.user_stub:
            logging.error("gRPC connection not established. Cannot login.")
            return False

        request = nodepool_pb2.LoginRequest(username=username, password=password)
        try:
            response = self.user_stub.Login(request, timeout=15)
            if response.success and response.token:
                self.add_or_update_user(username, response.token)
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
                logging.info(f"收到用戶 {username} 的檔案上傳請求")
                logging.info(f"請求的 files 鍵: {list(request.files.keys())}")
                logging.info(f"請求的 form 鍵: {list(request.form.keys())}")
                
                if 'task_zip' not in request.files:
                    logging.warning("請求中沒有 task_zip 檔案欄位")
                    logging.warning(f"可用的檔案欄位: {list(request.files.keys())}")
                    flash('請選擇 ZIP 檔案', 'error')
                    return render_template('master_upload.html', username=username)
                    
                file = request.files['task_zip']
                logging.info(f"接收到檔案物件: {file}")
                logging.info(f"檔案名稱: {file.filename}")
                logging.info(f"檔案內容類型: {file.content_type}")
                
                if not file.filename or file.filename == '':
                    logging.warning("檔案名稱為空")
                    flash('未選擇檔案，請選擇一個 ZIP 檔案', 'error')
                    return render_template('master_upload.html', username=username)
                
                if not file.filename.lower().endswith('.zip'):
                    logging.warning(f"檔案格式錯誤: {file.filename}")
                    flash('檔案格式無效，請上傳 .zip 檔案', 'error')
                    return render_template('master_upload.html', username=username)
                
                try:
                    file_content = file.read()
                    logging.info(f"成功讀取檔案內容，大小: {len(file_content)} bytes")
                    
                    if len(file_content) == 0:
                        logging.warning("檔案內容為空")
                        flash('上傳的檔案為空，請選擇有效的 ZIP 檔案', 'error')
                        return render_template('master_upload.html', username=username)
                    
                    max_size = 50 * 1024 * 1024
                    if len(file_content) > max_size:
                        logging.warning(f"檔案太大: {len(file_content)} bytes")
                        flash('檔案大小超過 50MB 限制', 'error')
                        return render_template('master_upload.html', username=username)
                    
                    try:
                        zip_buffer = io.BytesIO(file_content)
                        with zipfile.ZipFile(zip_buffer, 'r') as zip_file:
                            zip_file.testzip()
                            file_list = zip_file.namelist()
                            logging.info(f"ZIP 檔案驗證成功，包含 {len(file_list)} 個檔案")
                            if len(file_list) > 0:
                                logging.info(f"ZIP 內容範例: {file_list[:5]}")
                    except zipfile.BadZipFile:
                        logging.warning("無效的 ZIP 檔案")
                        flash('無效的 ZIP 檔案，請確認檔案沒有損壞', 'error')
                        return render_template('master_upload.html', username=username)
                    except Exception as e:
                        logging.error(f"ZIP 檔案驗證錯誤: {e}")
                        flash('檔案驗證失敗，請嘗試重新上傳', 'error')
                        return render_template('master_upload.html', username=username)
                    
                    logging.info(f"檔案驗證通過: {file.filename}, 大小: {len(file_content)} bytes")
                    
                    # 獲取重複次數
                    try:
                        repeat_count = int(request.form.get('repeat_count', 1))
                        if repeat_count < 1 or repeat_count > 100:
                            flash('重複次數必須在 1 到 100 之間', 'error')
                            return render_template('master_upload.html', username=username)
                    except ValueError:
                        flash('無效的重複次數，請輸入數字', 'error')
                        return render_template('master_upload.html', username=username)
                    
                    requirements = {
                        "memory_gb": request.form.get('memory_gb', 0),
                        "cpu_score": request.form.get('cpu_score', 0),
                        "gpu_score": request.form.get('gpu_score', 0),
                        "gpu_memory_gb": request.form.get('gpu_memory_gb', 0),
                        "location": request.form.get('location', 'Any'),
                        "gpu_name": request.form.get('gpu_name', '')
                    }
                    
                    success_count = 0
                    task_ids = []
                    timestamp = datetime.datetime.now().strftime("%Y%m%d_%H%M%S")
                    
                    for i in range(repeat_count):
                        task_uuid = str(uuid.uuid4())[:8]
                        task_id = f"task_{timestamp}_{task_uuid}_{i+1}"
                        logging.info(f"準備上傳任務 {task_id}，需求: {requirements}")
                        task_id, success = self.upload_task_with_user(username, task_id, file_content, requirements)
                        if success:
                            success_count += 1
                            task_ids.append(task_id)
                        else:
                            logging.error(f"任務 {task_id} 上傳失敗")
                    
                    if success_count == repeat_count:
                        flash(f'成功上傳 {success_count}/{repeat_count} 個任務: {", ".join(task_ids)}', 'success')
                        logging.info(f"成功上傳 {success_count}/{repeat_count} 個任務")
                    else:
                        flash(f'僅成功上傳 {success_count}/{repeat_count} 個任務: {", ".join(task_ids)}', 'warning')
                        logging.warning(f"僅成功上傳 {success_count}/{repeat_count} 個任務")
                    
                    return redirect(url_for('index') + f"?user={username}")
                        
                except Exception as e:
                    logging.error(f"處理上傳檔案時發生錯誤: {e}", exc_info=True)
                    flash('處理檔案時發生錯誤，請稍後再試', 'error')
                    return render_template('master_upload.html', username=username)
            
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

    def auto_join_vpn(self):
        """
        主控端自動請求 /api/vpn/join 取得 WireGuard 配置並嘗試連線 VPN。
        若自動連線失敗，提示用戶手動連線。
        """
        try:
            api_url = "https://hivemind.justin0711.com/api/vpn/join"
            nodename = os.environ.get("COMPUTERNAME", "master")
            client_name = f"master-{nodename}-{os.getpid()}"
            resp = requests.post(api_url, json={"client_name": client_name}, timeout=15, verify=True)
            try:
                resp_json = resp.json()
            except Exception:
                resp_json = {}
            if resp.status_code == 200 and resp_json.get("success"):
                config_content = resp_json.get("config")
                config_path = os.path.join(os.path.dirname(os.path.abspath(__file__)), "wg0.conf")
                try:
                    with open(config_path, "w") as f:
                        f.write(config_content)
                    logging.info(f"自動取得 WireGuard 配置並寫入 {config_path}")
                except Exception as e:
                    logging.warning(f"寫入 WireGuard 配置失敗: {e}")
                    return
                # 嘗試自動啟動 VPN
                result = os.system(f"wg-quick down {config_path} 2>/dev/null; wg-quick up {config_path}")
                if result == 0:
                    logging.info("WireGuard VPN 啟動成功")
                else:
                    logging.warning("WireGuard VPN 啟動失敗，請檢查權限與配置")
                    self.prompt_manual_vpn(config_path)
            else:
                error_msg = resp_json.get("error") if resp_json else resp.text
                logging.warning(f"自動取得 WireGuard 配置失敗: {error_msg}")
                if error_msg and "VPN 服務不可用" in error_msg:
                    logging.warning("請確認主控端 Flask 啟動時有正確初始化 WireGuardServer，且 /api/vpn/join 可用")
                self.prompt_manual_vpn()
        except Exception as e:
            logging.warning(f"自動請求 /api/vpn/join 失敗: {e}")
            self.prompt_manual_vpn()

    def prompt_manual_vpn(self, config_path=None):
        """提示用戶手動連線 WireGuard"""
        msg = (
            "\n[提示] 主控端自動連線 WireGuard 失敗，請手動連線 VPN：\n"
            "1. 請找到您的設定檔(wg0.conf)。\n"
            "2. 手動打開wireguard客戶端導入配置\n"
            "3. 如遇權限問題請用管理員/Root 權限執行。\n"
        )
        print(msg)
        print('如果您已經連線好請按y')
        a = input()
        if a == 'y':
            logging.info("用戶已確認主控端手動連線 WireGuard")

    def run(self):
        # 先自動連線 VPN
        self.auto_join_vpn()
        if not self._connect_grpc():
            logging.error("無法連接到節點池，退出")
            return

        # 移除自動登錄邏輯，要求用戶手動登錄
        try:
            logging.info(f"主控端啟動在 http://{UI_HOST}:{UI_PORT}")
            logging.info("請通過 Web 界面登錄以使用主控台功能")
            self.app.run(host=UI_HOST, port=UI_PORT, debug=False)
        except KeyboardInterrupt:
            logging.info("收到中斷信號，正在關閉...")
        finally:
            if self.channel:
                self.channel.close()

if __name__ == "__main__":
    # 移除預設用戶名密碼的傳遞
    master_ui = MasterNodeUI(GRPC_SERVER_ADDRESS)
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
