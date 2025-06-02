# node_pool/master_node_service.py
import grpc
import logging
import redis
import time
import nodepool_pb2
import nodepool_pb2_grpc
import threading
import json

# 從正確的位置導入 NodeManager
from node_manager import NodeManager

logging.basicConfig(level=logging.INFO, format='%(asctime)s - %(levelname)s - [%(threadName)s] - %(message)s')

# --- TaskManager 類保持不變 ---
class TaskManager:
    def __init__(self):
        # decode_responses=True for easier handling
        self.redis_client = redis.Redis(host='localhost', port=6379, db=0, decode_responses=True)
        try:
            self.redis_client.ping()
            logging.info("TaskManager: Redis 連線成功")
        except redis.RedisError as e:
            logging.error(f"TaskManager: Redis 連線失敗: {e}")
            raise

    def store_task(self, task_id, task_zip, memory_gb, cpu_score, gpu_score, gpu_memory_gb, location, gpu_name, user_id, cpt_cost=None):
        """存儲任務信息，包括用戶ID與cpt_cost"""
        task_key = f"task:{task_id}"
        try:
            # 記錄詳細的輸入參數
            logging.info(f"存儲任務 {task_id} 的詳細信息: memory_gb={memory_gb}, cpu_score={cpu_score}, "
                         f"gpu_score={gpu_score}, gpu_memory_gb={gpu_memory_gb}, user_id={user_id}")
            
            # 存儲二進制數據時需要特別處理，其他存為字符串
            task_info = {
                "memory_gb": str(memory_gb),
                "cpu_score": str(cpu_score),
                "gpu_score": str(gpu_score),
                "gpu_memory_gb": str(gpu_memory_gb),
                "location": location,
                "gpu_name": gpu_name,
                "status": "PENDING",
                "output": "",
                "assigned_node": "",
                "user_id": str(user_id),
                "created_at": str(time.time()),
                "updated_at": str(time.time())
            }
            
            # 添加 cpt_cost 字段
            if cpt_cost is not None:
                task_info["cpt_cost"] = str(cpt_cost)
            
            # 先存儲非二進制數據
            self.redis_client.hset(task_key, mapping=task_info)
            
            # 單獨存儲二進制數據，使用不會自動解碼的客戶端
            if task_zip:
                temp_client_no_decode = redis.Redis(host='localhost', port=6379, db=0, decode_responses=False)
                temp_client_no_decode.hset(task_key, "task_zip", task_zip)
                temp_client_no_decode.close()
                logging.debug(f"任務 {task_id} 的二進制ZIP數據已存儲 ({len(task_zip)} bytes)")
            
            # 將任務ID添加到用戶的任務集合中
            if user_id:
                user_tasks_key = f"user:{user_id}:tasks"
                self.redis_client.sadd(user_tasks_key, task_id)
                logging.info(f"已將任務 {task_id} 關聯到用戶 {user_id}")
            else:
                logging.warning(f"任務 {task_id} 無用戶ID，未能關聯到用戶")

            # 驗證存儲是否成功
            stored_user_id = self.redis_client.hget(task_key, "user_id")
            logging.info(f"任務 {task_id} 存儲後驗證: 用戶ID = {stored_user_id}")

            logging.info(f"任務 {task_id} 已存儲 (用戶ID: {user_id}, 需求 GPU: '{gpu_name}')")
            return True
        except redis.RedisError as e:
            logging.error(f"Redis 錯誤，任務 {task_id} 存儲失敗: {e}")
            return False
        except Exception as e:
            logging.error(f"存儲任務 {task_id} 時發生未知錯誤: {e}", exc_info=True)
            return False

    def store_output(self, task_id, output):
        """存儲任務的中途輸出 (追加方式)"""
        task_key = f"task:{task_id}"
        try:
            current_output = self.redis_client.hget(task_key, "output") or ""
            new_output = current_output + output + "\n"
            max_output_len = 1024 * 10
            if len(new_output) > max_output_len:
                new_output = new_output[-max_output_len:]

            self.redis_client.hset(task_key, "output", new_output)
            logging.debug(f"任務 {task_id} 中途輸出已更新")
            return True
        except redis.RedisError as e:
            logging.error(f"Redis 錯誤，任務 {task_id} 中途輸出存儲失敗: {e}")
            return False
        except Exception as e:
            logging.error(f"存儲任務 {task_id} 中途輸出時發生未知錯誤: {e}", exc_info=True)
            return False

    def store_result(self, task_id, result_zip):
        """存儲任務的最終結果 ZIP"""
        task_key = f"task:{task_id}"
        try:
            update_data = {
                "result_zip": result_zip,
                "status": "COMPLETED"
            }
            self.redis_client.hset(task_key, mapping=update_data)
            logging.info(f"任務 {task_id} 結果已存儲，狀態更新為 COMPLETED")
            return True
        except redis.RedisError as e:
            logging.error(f"Redis 錯誤，任務 {task_id} 結果存儲失敗: {e}")
            return False
        except Exception as e:
            logging.error(f"存儲任務 {task_id} 結果時發生未知錯誤: {e}", exc_info=True)
            return False

    def update_task_status(self, task_id, status, assigned_node=None):
        """更新任務狀態，可選地更新分配節點"""
        task_key = f"task:{task_id}"
        try:
            update_data = {"status": status}
            if assigned_node is not None:
                update_data["assigned_node"] = assigned_node

            if not self.redis_client.exists(task_key):
                logging.warning(f"嘗試更新狀態失敗：任務 {task_id} 不存在")
                return False

            self.redis_client.hset(task_key, mapping=update_data)
            log_msg = f"任務 {task_id} 狀態更新為: {status}"
            if assigned_node:
                log_msg += f", 分配給節點: {assigned_node}"
            logging.info(log_msg)
            return True
        except redis.RedisError as e:
            logging.error(f"Redis 錯誤，更新任務 {task_id} 狀態失敗: {e}")
            return False
        except Exception as e:
            logging.error(f"更新任務 {task_id} 狀態時發生未知錯誤: {e}", exc_info=True)
            return False

    def get_task_info(self, task_id, include_zip=False):
        """獲取任務信息，區分字符串和二進制字段"""
        task_key = f"task:{task_id}"
        task_info = {}
        string_keys = [
            "memory_gb", "cpu_score", "gpu_score", "gpu_memory_gb",
            "location", "gpu_name", "status", "output", "assigned_node",
            "user_id", "created_at", "updated_at"  # 確保包含user_id字段
        ]
        binary_keys = ["task_zip", "result_zip"]

        try:
            if not self.redis_client.exists(task_key):
                logging.warning(f"獲取任務信息失敗：任務 {task_id} 不存在")
                return None

            string_values = self.redis_client.hmget(task_key, string_keys)
            for key, value in zip(string_keys, string_values):
                task_info[key] = value if value is not None else ""
                
            # 輸出獲取到的用戶ID，用於調試
            if "user_id" in task_info:
                logging.debug(f"任務 {task_id} 的用戶ID: {task_info['user_id']}")
            else:
                logging.warning(f"任務 {task_id} 沒有獲取到用戶ID")

            if include_zip:
                temp_client_no_decode = redis.Redis(host='localhost', port=6379, db=0, decode_responses=False)
                for key in binary_keys:
                    binary_value = temp_client_no_decode.hget(task_key, key)
                    task_info[key] = binary_value if binary_value else b""
                temp_client_no_decode.close()
            else:
                for key in binary_keys:
                    field_exists = self.redis_client.hexists(task_key, key)
                    task_info[key] = "<binary data>" if field_exists else "<empty>"

            logging.debug(f"成功獲取任務 {task_id} 的信息 (include_zip={include_zip})")
            return task_info

        except redis.RedisError as e:
            logging.error(f"Redis 錯誤，獲取任務 {task_id} 信息失敗: {e}")
            return None
        except Exception as e:
            logging.error(f"獲取任務 {task_id} 信息時發生未知錯誤: {e}", exc_info=True)
            return None

    def get_task_result_zip(self, task_id):
        """專門獲取結果 ZIP (確保獲取 bytes)"""
        task_key = f"task:{task_id}"
        try:
            temp_client_no_decode = redis.Redis(host='localhost', port=6379, db=0, decode_responses=False)
            result_zip = temp_client_no_decode.hget(task_key, "result_zip")
            status_bytes = temp_client_no_decode.hget(task_key, "status")
            temp_client_no_decode.close()

            status_str = status_bytes.decode() if status_bytes else "UNKNOWN"

            if status_str == "COMPLETED":
                if result_zip:
                    return result_zip, status_str
                else:
                    logging.warning(f"任務 {task_id} 狀態為 COMPLETED 但結果 ZIP 為空")
                    return b"", status_str
            else:
                return b"", status_str

        except redis.RedisError as e:
            logging.error(f"Redis 錯誤，獲取任務 {task_id} 結果 ZIP 失敗: {e}")
            return b"", "UNKNOWN"
        except Exception as e:
            logging.error(f"獲取任務 {task_id} 結果 ZIP 時發生未知錯誤: {e}", exc_info=True)
            return b"", "UNKNOWN"

    def get_tasks_by_status(self, status):
        """根據狀態獲取任務列表"""
        tasks = []
        try:
            task_keys = self.redis_client.keys("task:*")
            for key in task_keys:
                task_id = key.split(":", 1)[1]
                task_status = self.redis_client.hget(key, "status")
                if task_status == status:
                    user_id = self.redis_client.hget(key, "user_id")
                    memory_gb = self.redis_client.hget(key, "memory_gb")
                    cpu_score = self.redis_client.hget(key, "cpu_score")
                    gpu_score = self.redis_client.hget(key, "gpu_score")
                    gpu_memory_gb = self.redis_client.hget(key, "gpu_memory_gb")
                    location = self.redis_client.hget(key, "location") or "Any"
                    gpu_name = self.redis_client.hget(key, "gpu_name") or ""
                    tasks.append({
                        "task_id": task_id,
                        "user_id": user_id,
                        "requirements": {
                            "memory_gb": int(memory_gb or 0),
                            "cpu_score": int(cpu_score or 0),
                            "gpu_score": int(gpu_score or 0),
                            "gpu_memory_gb": int(gpu_memory_gb or 0),
                            "location": location,
                            "gpu_name": gpu_name,
                        }
                    })
            return tasks
        except Exception as e:
            logging.error(f"get_tasks_by_status({status}) error: {e}", exc_info=True)
            return []

    def get_pending_tasks(self):
        """獲取所有 PENDING 狀態的任務"""
        return self.get_tasks_by_status("PENDING")

    def get_running_tasks(self):
        """獲取所有 RUNNING 狀態的任務"""
        return self.get_tasks_by_status("RUNNING")

    def get_completed_tasks_without_reward(self):
        """
        獲取所有 COMPLETED 狀態且尚未發放獎勵的任務
        這裡假設有一個欄位 reward_distributed 標記是否已發獎勵，若無則只回傳 COMPLETED
        """
        tasks = []
        try:
            task_keys = self.redis_client.keys("task:*")
            for key in task_keys:
                task_id = key.split(":", 1)[1]
                status = self.redis_client.hget(key, "status")
                reward_flag = self.redis_client.hget(key, "reward_distributed")
                if status == "COMPLETED" and not reward_flag:
                    user_id = self.redis_client.hget(key, "user_id")
                    tasks.append({
                        "task_id": task_id,
                        "user_id": user_id
                    })
            return tasks
        except Exception as e:
            logging.error(f"get_completed_tasks_without_reward error: {e}", exc_info=True)
            return []

# --- MasterNodeServiceServicer 類 ---
class MasterNodeServiceServicer(nodepool_pb2_grpc.MasterNodeServiceServicer):
    def __init__(self):
        self.task_manager = TaskManager()
        self.node_manager = NodeManager()
        self._stop_event = threading.Event()
        self.dispatch_interval = 10
        self.dispatcher_thread = None
        self.health_check_interval = 10
        self.reward_interval = 60
        self.task_health = {}  # {task_id: {"fail_count": 0, "last_check": 0, ...}}
        self.pending_rewards = []  # [(user_id, worker_node_id, amount, task_id)]
        self.start_task_dispatcher()
        self.start_health_checker()
        self.start_reward_scheduler()
        self.task_logs = {}  # 儲存任務日誌
        self.logs_lock = threading.Lock()

    def __del__(self):
        """確保在對象銷毀時停止後台線程"""
        self.stop_task_dispatcher()

    def start_task_dispatcher(self):
        """啟動後台線程來分發待處理任務"""
        if self.dispatcher_thread is None or not self.dispatcher_thread.is_alive():
            self._stop_event.clear()
            self.dispatcher_thread = threading.Thread(target=self._dispatch_pending_tasks_loop, name="TaskDispatcher", daemon=True)
            self.dispatcher_thread.start()
            logging.info("任務分發後台線程已啟動")

    def stop_task_dispatcher(self):
        """停止後台任務分發線程"""
        if self.dispatcher_thread and self.dispatcher_thread.is_alive():
             logging.info("正在停止任務分發後台線程...")
             self._stop_event.set()
             self.dispatcher_thread.join(timeout=self.dispatch_interval + 2) # 等待線程結束
             if self.dispatcher_thread.is_alive():
                  logging.warning("任務分發線程未能在超時內優雅停止")
             else:
                  logging.info("任務分發後台線程已停止")
        self.dispatcher_thread = None

    def start_health_checker(self):
        def health_check_loop():
            while not self._stop_event.is_set():
                # 檢查所有 RUNNING 任務
                running_tasks = self.task_manager.get_running_tasks()  # 你需要實作這個方法，回傳所有狀態為 RUNNING 的任務
                for task in running_tasks:
                    task_id = task["task_id"]
                    assigned_node = task.get("assigned_node")
                    user_id = task.get("user_id")
                    if not assigned_node:
                        continue
                    node_info = self.node_manager.get_node_info(assigned_node)  # 你需要實作這個方法
                    if not node_info:
                        continue
                    # 呼叫工作端健康檢查
                    try:
                        worker_channel = grpc.insecure_channel(f"{node_info['ip']}:{node_info['port']}")
                        stub = nodepool_pb2_grpc.WorkerNodeServiceStub(worker_channel)
                        resp = stub.HealthCheck(nodepool_pb2.HealthCheckRequest(), timeout=5)
                        if resp.success:
                            # 健康，重置 fail_count
                            self.task_health.setdefault(task_id, {})["fail_count"] = 0
                        else:
                            self.task_health.setdefault(task_id, {})["fail_count"] = self.task_health.get(task_id, {}).get("fail_count", 0) + 1
                    except Exception:
                        self.task_health.setdefault(task_id, {})["fail_count"] = self.task_health.get(task_id, {}).get("fail_count", 0) + 1
                    finally:
                        if worker_channel:
                            worker_channel.close()
                    # 如果失敗超過3次，重派任務
                    if self.task_health[task_id]["fail_count"] >= 3:
                        self._reassign_task(task_id, user_id)
                time.sleep(self.health_check_interval)
        threading.Thread(target=health_check_loop, daemon=True).start()

    def _reassign_task(self, task_id, user_id):
        # 1. 更新任務狀態為PENDING
        self.task_manager.update_task_status(task_id, "PENDING", assigned_node=None)
        # 2. 日誌
        logging.warning(f"任務 {task_id} 因健康檢查失敗，已重派為PENDING")
        # 3. 可選：通知主控端

    def start_reward_scheduler(self):
        def reward_loop():
            while not self._stop_event.is_set():
                # 統一處理所有已完成且尚未發獎勵的任務
                completed_tasks = self.task_manager.get_completed_tasks_without_reward()  # 你需要實作這個方法
                for task in completed_tasks:
                    user_id = task["user_id"]
                    assigned_node_id = task.get("assigned_node")
                    cpt_cost = int(task.get("cpt_cost", 1))
                    # 查 assigned_node_id 對應的 username
                    worker_username = assigned_node_id
                    # 如果 node_id 是 user_id，則查 username
                    try:
                        from user_manager import UserManager
                        user_manager = UserManager()
                        # 如果 node_id 是 user_id（數字），查 username
                        if assigned_node_id and assigned_node_id.isdigit():
                            user_row = user_manager.query_one("SELECT username FROM users WHERE id = ?", (int(assigned_node_id),))
                            if user_row:
                                worker_username = user_row["username"]
                    except Exception as e:
                        logging.warning(f"查詢 worker username 失敗: {e}")
                    # 1. 檢查 user_id 餘額
                    from user_manager import UserManager
                    user_manager = UserManager()
                    balance = user_manager.get_user_balance(user_id)
                    if balance < cpt_cost:
                        # 餘額不足，強制停止任務
                        self.task_manager.force_stop_task(task["task_id"])
                        logging.warning(f"主控端帳戶 {user_id} 餘額不足，任務 {task['task_id']} 已強制停止")
                        continue
                    # 2. 轉帳
                    success, msg = user_manager.transfer_tokens(user_id, worker_username, cpt_cost)
                    if success:
                        self.task_manager.mark_reward_given(task["task_id"])
                        logging.info(f"任務 {task['task_id']} 獎勵 {cpt_cost} 已由主控端 {user_id} 轉給 {worker_username}")
                    else:
                        logging.error(f"任務 {task['task_id']} 發獎勵失敗: {msg}")
                time.sleep(self.reward_interval)
        threading.Thread(target=reward_loop, daemon=True).start()

    def _dispatch_pending_tasks_loop(self):
        """後台線程循環，定期檢查並分發任務"""
        while not self._stop_event.is_set():
            logging.debug("開始新一輪任務分發檢查...")
            try:
                # 1. 獲取待處理任務列表
                pending_tasks = self.task_manager.get_pending_tasks()
                if not pending_tasks:
                    logging.debug("當前無待處理任務")
                    self._stop_event.wait(self.dispatch_interval) # 等待指定時間
                    continue

                # 2. 獲取所有可用節點列表 (一次性獲取)
                all_idle_nodes = [node for node in self.node_manager.get_node_list() if node.status == "Idle"]

                # 移除沒有心跳或IP的節點
                valid_idle_nodes = []
                for node in all_idle_nodes:
                    node_info = self.node_manager.get_node_info(node.node_id)
                    ip = node_info.get("ip") if node_info else None
                    last_heartbeat = None
                    try:
                        last_heartbeat = float(node.last_heartbeat)
                    except Exception:
                        last_heartbeat = 0
                    # 心跳超過30秒或沒有IP就不列入
                    if not ip or ip == "" or (time.time() - last_heartbeat > 30):
                        logging.warning(f"移除無效節點: {node.node_id} (ip={ip}, last_heartbeat={last_heartbeat})")
                        # 直接從redis刪除該節點資訊
                        try:
                            self.node_manager.redis_client.delete(f"node:{node.node_id}")
                        except Exception as e:
                            logging.error(f"刪除無效節點 {node.node_id} 失敗: {e}")
                        continue
                    valid_idle_nodes.append(node)

                if not valid_idle_nodes:
                     logging.info("無有效空閒節點可用於分發，等待下一輪")
                     self._stop_event.wait(self.dispatch_interval)
                     continue

                logging.info(f"待處理任務: {len(pending_tasks)}, 空閒節點: {len(valid_idle_nodes)}")

                dispatched_task_ids = set()
                assigned_node_ids = set()

                for task_data in pending_tasks:
                    if self._stop_event.is_set(): break
                    task_id = task_data["task_id"]
                    reqs = task_data["requirements"]
                    user_id = task_data.get("user_id")
                    
                    # 確保user_id不為None
                    if not user_id:
                        # 嘗試直接從Redis獲取任務信息來獲取user_id
                        task_info = self.task_manager.get_task_info(task_id, include_zip=False)
                        if task_info and task_info.get("user_id"):
                            user_id = task_info.get("user_id")
                            logging.info(f"從任務信息重新獲取到用戶ID: {user_id}")
                        else:
                            logging.error(f"無法確定任務 {task_id} 的用戶ID，跳過分發")
                            continue

                    # 檢查用戶餘額
                    from user_manager import UserManager
                    user_manager = UserManager()
                    user_balance_result = user_manager.get_user_balance(user_id)
                    if isinstance(user_balance_result, tuple):
                        balance = user_balance_result[-1]
                    else:
                        balance = user_balance_result
                    
                    # 計算任務所需的 CPT 代币
                    memory_gb_val = float(reqs["memory_gb"])
                    cpu_score_val = float(reqs["cpu_score"])
                    gpu_score_val = float(reqs["gpu_score"])
                    gpu_memory_gb_val = float(reqs["gpu_memory_gb"])
                    
                    cpt_cost = (
                        memory_gb_val +  # 内存需求
                        cpu_score_val / 100 +  # CPU 分数
                        gpu_score_val / 100 +  # GPU 分数
                        gpu_memory_gb_val  # GPU 显存
                    )
                    
                    # 最低消費為1個代幣
                    if cpt_cost < 1.0:
                        cpt_cost = 1.0
                    
                    logging.info(f"任務 {task_id} 資源需求: 內存={memory_gb_val}GB, CPU分數={cpu_score_val}, " +
                               f"GPU分數={gpu_score_val}, GPU顯存={gpu_memory_gb_val}GB, " +
                               f"計算所需代幣: {cpt_cost:.2f} CPT")
                    
                    if balance < cpt_cost:
                        logging.warning(f"用戶 {user_id} 餘額不足，無法分發任務 {task_id} (需要 {cpt_cost:.2f} CPT，目前 {balance:.2f} CPT)")
                        continue

                    candidate_nodes = [
                        node for node in valid_idle_nodes
                        if node.node_id not in assigned_node_ids and
                           node.memory_gb >= reqs["memory_gb"] and
                           node.cpu_score >= reqs["cpu_score"] and
                           node.gpu_score >= reqs["gpu_score"] and
                           node.gpu_memory_gb >= reqs["gpu_memory_gb"] and
                           (reqs["location"] == "Any" or node.location == reqs["location"]) and
                           (not reqs["gpu_name"] or reqs["gpu_name"].lower() == "any" or node.gpu_name == reqs["gpu_name"])
                    ]

                    # 嘗試分發到所有候選節點，直到成功
                    dispatched = False
                    for target_node in sorted(candidate_nodes, key=lambda n: n.cpu_score, reverse=True):
                        logging.info(f"嘗試將任務 {task_id} 分發給節點 {target_node.node_id} (Port: {target_node.port})...")
                        if self._try_dispatch_task_to_node(task_id, target_node):
                            dispatched_task_ids.add(task_id)
                            assigned_node_ids.add(target_node.node_id)
                            dispatched = True
                            break
                        else:
                            logging.warning(f"分發任務 {task_id} 給節點 {target_node.node_id} 失敗")
                    if not dispatched:
                        logging.warning(f"任務 {task_id} 無法分發給任何可用節點")

                logging.debug(f"本輪任務分發結束，成功分發 {len(dispatched_task_ids)} 個任務")

            except Exception as e:
                logging.error(f"任務分發循環出錯: {e}", exc_info=True)

            self._stop_event.wait(self.dispatch_interval)

        logging.info("任務分發後台線程循環結束")

    def _try_dispatch_task_to_node(self, task_id, target_node):
        """
        嘗試將指定任務分發給目標節點。

        Args:
            task_id (str): 要分發的任務 ID。
            target_node (nodepool_pb2.WorkerNodeInfo): 目標工作節點的信息對象。

        Returns:
            bool: 如果任務成功發送給 Worker 並被接受，返回 True，否則返回 False。
        """
        # 1. 檢查目標節點端口
        if not target_node.port or target_node.port <= 0:
            logging.error(f"節點 {target_node.node_id} 端口無效 ({target_node.port})，無法分發任務 {task_id}")
            return False

        # 取得節點 IP（從 Redis 查）
        node_info = self.node_manager.get_node_info(target_node.node_id)
        ip = node_info.get("ip") if node_info else None
        if not ip or ip == "":
            logging.error(f"節點 {target_node.node_id} 註冊時未提供 IP，無法分發任務 {task_id}")
            return False

        worker_address = f'{ip}:{target_node.port}'  # 用查到的 IP
        worker_channel = None
        logging.info(f"準備分發任務 {task_id} 到節點 {target_node.node_id} ({worker_address})")

        try:
            # 2. 獲取任務的詳細信息，包括 ZIP 二進制數據
            #    調用修改後的 get_task_info，確保 include_zip=True
            task_info = self.task_manager.get_task_info(task_id, include_zip=True)

            # 檢查是否成功獲取任務信息以及 task_zip 是否存在
            if not task_info:
                 logging.error(f"無法獲取任務 {task_id} 的信息 (可能任務不存在?)，分發失敗")
                 return False
            if "task_zip" not in task_info:
                logging.error(f"任務 {task_id} 的信息中缺少 'task_zip' 字段，分發失敗")
                return False

            # 3. 驗證 task_zip 是 bytes 類型
            task_zip_bytes = task_info.get("task_zip")
            if not isinstance(task_zip_bytes, bytes):
                 logging.error(f"任務 {task_id} 的 task_zip 數據類型錯誤 (應為 bytes，實際為 {type(task_zip_bytes)})，分發失敗")
                 # 處理可能存在的標記，例如 "<binary data>"
                 if isinstance(task_zip_bytes, str) and task_zip_bytes == "<binary data>":
                      logging.error(f"任務 {task_id} 的 task_zip 數據未正確獲取 (得到標記)，請檢查 get_task_info 邏輯，分發失敗")
                 return False
            if not task_zip_bytes: # 檢查是否為空 bytes
                 logging.warning(f"任務 {task_id} 的 task_zip 數據為空，仍嘗試分發...")
                 # 或者根據業務邏輯決定是否允許空 zip 分發
                 # return False

            # 4. 設置 gRPC 連接和調用超時
            connect_timeout = 3 # 連接 Worker 的超時時間 (秒)
            call_timeout = 30   # 調用 Worker ExecuteTask 的超時時間 (秒) - 可能需要根據任務準備時間調整

            # 5. 建立到 Worker 的 gRPC 連接
            logging.debug(f"正在連接到 Worker {target_node.node_id} at {worker_address}...")
            worker_channel = grpc.insecure_channel(worker_address)
            try:
                 # 嘗試快速檢查連接是否可用，帶超時
                 grpc.channel_ready_future(worker_channel).result(timeout=connect_timeout)
                 logging.debug(f"成功連接到 Worker {target_node.node_id}")
            except grpc.FutureTimeoutError:
                 logging.warning(f"連接到 Worker {target_node.node_id} ({worker_address}) 超時 ({connect_timeout}s)")
                 # 連接超時，關閉 channel 並返回失敗
                 if worker_channel: worker_channel.close()
                 return False

            # 6. 發送 ExecuteTask 請求給 Worker
            logging.debug(f"向 Worker {target_node.node_id} 發送 ExecuteTask 請求 (Task ID: {task_id})")
            worker_stub = nodepool_pb2_grpc.WorkerNodeServiceStub(worker_channel)
            execute_request = nodepool_pb2.ExecuteTaskRequest(
                node_id=target_node.node_id, # 可選，用於 Worker 端驗證
                task_id=task_id,
                task_zip=task_zip_bytes # 傳遞驗證過的 bytes
            )

            # 設置調用超時
            response = worker_stub.ExecuteTask(execute_request, timeout=call_timeout)

            # 7. 處理 Worker 的響應
            if response.success:
                logging.info(f"成功將任務 {task_id} 分發給節點 {target_node.node_id}，Worker 已接受")
                # 更新任務狀態為 RUNNING 並記錄分配的節點
                update_ok = self.task_manager.update_task_status(task_id, "RUNNING", target_node.node_id)
                if not update_ok:
                     logging.error(f"任務 {task_id} 已被 Worker 接受，但在 Redis 中更新狀態失敗！")
                     # 這是一個潛在的狀態不一致問題，需要關注
                # 可選：更新 Master 端記錄的節點狀態（雖然 Worker 也會報告）
                # self.node_manager.report_status(target_node.node_id, "Executing Task")
                return True # 分發成功
            else:
                # Worker 明確拒絕執行任務
                logging.warning(f"節點 {target_node.node_id} 拒絕執行任務 {task_id}: {response.message}")
                return False # 分發失敗

        except grpc.RpcError as e:
            # 處理 gRPC 通信錯誤 (例如 DEADLINE_EXCEEDED, UNAVAILABLE, etc.)
            error_code = e.code()
            error_details = e.details()
            log_level = logging.WARNING # 默認警告級別
            if error_code == grpc.StatusCode.DEADLINE_EXCEEDED:
                 log_level = logging.WARNING # 超時可能是暫時的
                 log_message = f"調用 Worker {target_node.node_id} ({worker_address}) ExecuteTask 超時 ({call_timeout}s)"
            elif error_code == grpc.StatusCode.UNAVAILABLE:
                 log_level = logging.WARNING # Worker 可能暫時不可達
                 log_message = f"Worker {target_node.node_id} ({worker_address}) 不可用"
            else:
                 log_level = logging.ERROR # 其他 gRPC 錯誤可能是更嚴重的問題
                 log_message = f"與 Worker {target_node.node_id} ({worker_address}) 通信時 gRPC 錯誤: {error_code} - {error_details}"
            logging.log(log_level, log_message)
            return False # 分發失敗
        except Exception as e:
            # 捕獲其他任何意外錯誤
            logging.error(f"分發任務 {task_id} 給節點 {target_node.node_id} ({worker_address}) 時發生未知錯誤: {e}", exc_info=True)
            return False # 分發失敗
        finally:
            # 無論成功或失敗，都要確保關閉 gRPC channel
            if worker_channel:
                worker_channel.close()
                logging.debug(f"已關閉到 Worker {target_node.node_id} 的 gRPC channel")
    # --- gRPC 服務方法實現 ---
    def UploadTask(self, request, context):
        """處理來自主控端的任務上傳請求，記錄 user_id"""
        logging.info(f"收到任務上傳請求: 任務 {request.task_id}")
        try:
            user_id = request.user_id
            if not user_id:
                context.set_code(grpc.StatusCode.INVALID_ARGUMENT)
                context.set_details("缺少用戶ID")
                return nodepool_pb2.UploadTaskResponse(success=False, message="缺少用戶ID")
            # 計算任務所需 CPT
            cpt_cost = self._calculate_cpt_cost(request)
            # 存儲任務，記錄 user_id 及 cpt_cost
            success = self.task_manager.store_task(
                request.task_id,
                request.task_zip,
                request.memory_gb,
                request.cpu_score,
                request.gpu_score,
                request.gpu_memory_gb,
                request.location,
                request.gpu_name,
                user_id,
                cpt_cost=cpt_cost  # 你需要在 store_task 增加這個欄位
            )
            if not success:
                context.set_code(grpc.StatusCode.INTERNAL)
                context.set_details("任務存儲失敗")
                return nodepool_pb2.UploadTaskResponse(success=False, message="任務存儲失敗")
            return nodepool_pb2.UploadTaskResponse(success=True, message="任務已上傳")
        except Exception as e:
            logging.error(f"處理任務 {request.task_id} 上傳時發生未知錯誤: {e}", exc_info=True)
            context.set_code(grpc.StatusCode.INTERNAL)
            context.set_details("服務器內部錯誤")
            return nodepool_pb2.UploadTaskResponse(success=False, message="服務器內部錯誤")

    def _calculate_cpt_cost(self, request):
        # 根據需求計算 CPT 花費
        memory_gb_val = float(request.memory_gb)
        cpu_score_val = float(request.cpu_score)
        gpu_score_val = float(request.gpu_score)
        gpu_memory_gb_val = float(request.gpu_memory_gb)
        cpt_cost = memory_gb_val + cpu_score_val / 100 + gpu_score_val / 100 + gpu_memory_gb_val
        if cpt_cost < 1.0:
            cpt_cost = 1.0
        return int(cpt_cost)

    def PollTaskStatus(self, request, context):
        """輪詢任務狀態"""
        task_id = request.task_id
        logging.debug(f"處理任務狀態輪詢請求: {task_id}")
        try:
            task_info = self.task_manager.get_task_info(task_id, include_zip=False) # 不獲取 zip
            if task_info:
                status = task_info.get("status", "UNKNOWN")
                # 獲取輸出，如果為空則返回空列表
                output_str = task_info.get("output", "")
                output_lines = output_str.splitlines() if output_str else []
                message = f"狀態查詢成功: {status}"
                logging.debug(f"任務 {task_id} 狀態: {status}, Output lines: {len(output_lines)}")
                return nodepool_pb2.PollTaskStatusResponse(
                    task_id=task_id,
                    status=status,
                    output=output_lines, # 返回列表
                    message=message
                )
            else:
                logging.warning(f"輪詢任務 {task_id} 失敗：任務不存在")
                context.set_code(grpc.StatusCode.NOT_FOUND)
                context.set_details("任務不存在")
                return nodepool_pb2.PollTaskStatusResponse(
                    task_id=task_id,
                    status="UNKNOWN",
                    output=[],
                    message="任務不存在"
                )
        except Exception as e:
            logging.error(f"輪詢任務 {task_id} 狀態時發生錯誤: {e}", exc_info=True)
            context.set_code(grpc.StatusCode.INTERNAL)
            context.set_details("服務器內部錯誤")
            return nodepool_pb2.PollTaskStatusResponse(
                task_id=task_id,
                status="ERROR",
                output=[],
                message=f"狀態查詢失敗: {str(e)}"
            )

    def StoreOutput(self, request, context):
        """存儲來自 Worker 的中途輸出"""
        task_id = request.task_id
        output = request.output
        logging.debug(f"收到任務 {task_id} 的中途輸出")
        success = self.task_manager.store_output(task_id, output)
        if success:
            return nodepool_pb2.StatusResponse(success=True, message="中途輸出存儲成功")
        else:
            # 考慮是否要設置更具體的錯誤碼
            context.set_code(grpc.StatusCode.INTERNAL)
            context.set_details("中途輸出存儲失敗 (內部錯誤)")
            return nodepool_pb2.StatusResponse(success=False, message="中途輸出存儲失敗")

    def StoreResult(self, request, context):
        """存儲來自 Worker 的最終結果"""
        task_id = request.task_id
        result_zip = request.result_zip
        logging.info(f"收到任務 {task_id} 的最終結果 (大小: {len(result_zip)} bytes)")
        success = self.task_manager.store_result(task_id, result_zip)
        if success:
            return nodepool_pb2.StatusResponse(success=True, message="結果存儲成功")
        else:
            context.set_code(grpc.StatusCode.INTERNAL)
            context.set_details("結果存儲失敗 (內部錯誤)")
            return nodepool_pb2.StatusResponse(success=False, message="結果存儲失敗")

    def GetTaskResult(self, request, context):
        """客戶端獲取任務結果"""
        task_id = request.task_id
        logging.info(f"處理獲取任務 {task_id} 結果的請求")
        try:
            result_zip, status = self.task_manager.get_task_result_zip(task_id)

            if status == "COMPLETED":
                if result_zip:
                     logging.info(f"成功返回任務 {task_id} 的結果 (大小: {len(result_zip)} bytes)")
                     return nodepool_pb2.GetTaskResultResponse(
                        success=True,
                        message="獲取結果成功",
                        result_zip=result_zip
                     )
                else:
                     logging.warning(f"任務 {task_id} 狀態為 COMPLETED 但結果 ZIP 為空")
                     context.set_code(grpc.StatusCode.INTERNAL) # 狀態和數據不一致，算內部錯誤
                     context.set_details("任務已完成但結果數據丟失")
                     return nodepool_pb2.GetTaskResultResponse(
                        success=False,
                        message="任務已完成但結果數據丟失",
                        result_zip=b""
                     )
            elif status == "UNKNOWN":
                 logging.warning(f"獲取任務 {task_id} 結果失敗：任務不存在")
                 context.set_code(grpc.StatusCode.NOT_FOUND)
                 context.set_details("任務不存在")
                 return nodepool_pb2.GetTaskResultResponse(
                     success=False,
                     message="任務不存在",
                     result_zip=b""
                 )
            else: # PENDING, RUNNING, FAILED, ERROR 等狀態
                logging.info(f"獲取任務 {task_id} 結果請求被拒絕：任務狀態為 {status}")
                context.set_code(grpc.StatusCode.FAILED_PRECONDITION) # 狀態不滿足獲取結果的條件
                context.set_details(f"任務未完成或已失敗，當前狀態: {status}")
                return nodepool_pb2.GetTaskResultResponse(
                    success=False,
                    message=f"任務未完成或已失敗，當前狀態: {status}",
                    result_zip=b""
                )
        except Exception as e:
            logging.error(f"獲取任務 {task_id} 結果時發生未知錯誤: {e}", exc_info=True)
            context.set_code(grpc.StatusCode.INTERNAL)
            context.set_details("服務器內部錯誤")
            return nodepool_pb2.GetTaskResultResponse(
                success=False,
                message=f"獲取結果失敗: {str(e)}",
                result_zip=b""
            )

    def TaskCompleted(self, request, context):
        """處理來自 Worker 的任務完成通知"""
        task_id = request.task_id
        node_id = request.node_id
        success = request.success
        
        # 添加詳細日誌
        logging.info(f"開始處理來自節點 {node_id} 的任務 {task_id} 完成通知")
        
        try:
            status = "COMPLETED" if success else "FAILED"
            
            # 記錄任務狀態更新
            logging.info(f"嘗試將任務 {task_id} 狀態更新為 {status}")
            update_success = self.task_manager.update_task_status(task_id, status)
            logging.info(f"任務狀態更新{'成功' if update_success else '失敗'}")
            
            if update_success:
                # 記錄節點狀態更新
                logging.info(f"嘗試將節點 {node_id} 狀態更新為 Idle")
                node_update_success, msg = self.node_manager.report_status(node_id, "Idle")
                logging.info(f"節點狀態更新{'成功' if node_update_success else '失敗'}: {msg}")
                
                return nodepool_pb2.StatusResponse(
                    success=True,
                    message=f"任務狀態更新成功: {status}"
                )
            else:
                error_msg = f"更新任務 {task_id} 狀態失敗"
                logging.error(error_msg)
                context.set_code(grpc.StatusCode.INTERNAL)
                context.set_details(error_msg)
                return nodepool_pb2.StatusResponse(
                    success=False,
                    message=error_msg
                )
        except Exception as e:
            error_msg = f"處理任務 {task_id} 完成通知時發生錯誤: {str(e)}"
            logging.error(error_msg, exc_info=True)
            context.set_code(grpc.StatusCode.INTERNAL)
            context.set_details(error_msg)
            return nodepool_pb2.StatusResponse(
                success=False,
                message=error_msg
            )

    def UpdateTaskStatus(self, request, context):
        """處理來自工作節點的任務狀態更新請求"""
        task_id = request.task_id
        status = request.status
        message = request.message
        
        logging.info(f"收到任務 {task_id} 的狀態更新請求: {status}, 訊息: {message}")
        
        try:
            # 更新任務狀態
            update_success = self.task_manager.update_task_status(task_id, status)
            
            if update_success:
                return nodepool_pb2.StatusResponse(
                    success=True,
                    message=f"任務狀態成功更新為: {status}"
                )
            else:
                error_msg = f"更新任務 {task_id} 狀態失敗"
                logging.error(error_msg)
                context.set_code(grpc.StatusCode.INTERNAL)
                context.set_details(error_msg)
                return nodepool_pb2.StatusResponse(
                    success=False,
                    message=error_msg
                )
                
        except Exception as e:
            error_msg = f"處理任務 {task_id} 狀態更新時發生錯誤: {str(e)}"
            logging.error(error_msg, exc_info=True)
            context.set_code(grpc.StatusCode.INTERNAL)
            context.set_details(error_msg)
            return nodepool_pb2.StatusResponse(
                success=False,
                message=error_msg
            )

    def StoreLogs(self, request, context):
        """存儲工作節點回報的日誌"""
        try:
            with self.logs_lock:
                task_id = request.task_id
                if task_id not in self.task_logs:
                    self.task_logs[task_id] = []
                
                new_logs = json.loads(request.logs)
                self.task_logs[task_id].extend(new_logs)
                
                # 清理超過6小時的日誌
                cutoff = time.time() - 21600
                self.task_logs[task_id] = [
                    log for log in self.task_logs[task_id]
                    if log['timestamp'] > cutoff
                ]
            
            return nodepool_pb2.StatusResponse(success=True)
        except Exception as e:
            logging.error(f"存儲日誌失敗: {e}")
            return nodepool_pb2.StatusResponse(success=False, message=str(e))

    def GetTaskLogs(self, request, context):
        """獲取任務的日誌"""
        try:
            # 驗證token並獲取用戶ID
            user_id = self._verify_token(request.token)
            if not user_id:
                context.set_code(grpc.StatusCode.UNAUTHENTICATED)
                context.set_details("無效的令牌")
                return nodepool_pb2.GetTaskLogsResponse(
                    success=False,
                    message="無效的令牌"
                )
            
            task_id = request.task_id
            
            # 檢查任務是否存在
            task_info = self.task_manager.get_task_info(task_id, include_zip=False)
            if not task_info:
                context.set_code(grpc.StatusCode.NOT_FOUND)
                context.set_details("找不到該任務")
                return nodepool_pb2.GetTaskLogsResponse(
                    success=False,
                    message="找不到該任務"
                )
            
            # 檢查任務是否屬於該用戶
            if str(task_info.get("user_id", "")) != str(user_id):
                context.set_code(grpc.StatusCode.PERMISSION_DENIED)
                context.set_details("無權訪問此任務的日誌")
                return nodepool_pb2.GetTaskLogsResponse(
                    success=False,
                    message="無權訪問此任務的日誌"
                )
            
            with self.logs_lock:
                if task_id not in self.task_logs:
                    return nodepool_pb2.GetTaskLogsResponse(
                        success=False,
                        message="找不到該任務的日誌"
                    )
                
                return nodepool_pb2.GetTaskLogsResponse(
                    success=True,
                    message="成功獲取任務日誌",
                    logs=json.dumps(self.task_logs[task_id])
                )
        except Exception as e:
            logging.error(f"獲取日誌失敗: {e}")
            context.set_code(grpc.StatusCode.INTERNAL)
            context.set_details(f"服務器內部錯誤: {str(e)}")
            return nodepool_pb2.GetTaskLogsResponse(
                success=False,
                message=f"獲取日誌失敗: {str(e)}"
            )

    def GetUserTasks(self, request, context):
        """獲取用戶的所有任務"""
        try:
            # 驗證token並獲取用戶ID
            user_id = self._verify_token(request.token)
            if not user_id:
                context.set_code(grpc.StatusCode.UNAUTHENTICATED)
                context.set_details("Invalid token")
                return nodepool_pb2.GetUserTasksResponse(success=False, message="Invalid token")

            # 獲取用戶的任務
            tasks = self.task_manager.get_user_tasks(user_id)
            
            # 轉換為proto消息
            task_statuses = []
            for task in tasks:
                task_statuses.append(nodepool_pb2.TaskStatus(
                    task_id=task["task_id"],
                    status=task["status"],
                    created_at=task["created_at"],
                    updated_at=task["updated_at"]
                ))

            return nodepool_pb2.GetUserTasksResponse(
                success=True,
                message="Success",
                tasks=task_statuses
            )
        except Exception as e:
            logging.error(f"獲取用戶任務失敗: {e}", exc_info=True)
            context.set_code(grpc.StatusCode.INTERNAL)
            context.set_details(str(e))
            return nodepool_pb2.GetUserTasksResponse(success=False, message=str(e))

    def GetTaskStatus(self, request, context):
        """獲取特定任務的狀態"""
        try:
            # 驗證token
            user_id = self._verify_token(request.token)
            if not user_id:
                context.set_code(grpc.StatusCode.UNAUTHENTICATED)
                context.set_details("Invalid token")
                return nodepool_pb2.GetTaskStatusResponse(success=False, message="Invalid token")

            # 獲取任務狀態
            task_info = self.task_manager.get_task_status(request.task_id)
            if not task_info:
                return nodepool_pb2.GetTaskStatusResponse(
                    success=False,
                    message="Task not found"
                )

            # 檢查任務是否屬於該用戶
            if str(task_info["user_id"]) != str(user_id):
                context.set_code(grpc.StatusCode.PERMISSION_DENIED)
                context.set_details("Task does not belong to user")
                return nodepool_pb2.GetTaskStatusResponse(
                    success=False,
                    message="Task does not belong to user"
                )

            return nodepool_pb2.GetTaskStatusResponse(
                success=True,
                message="Success",
                task=nodepool_pb2.TaskStatus(
                    task_id=task_info["task_id"],
                    status=task_info["status"],
                    created_at=task_info["created_at"],
                    updated_at=task_info["updated_at"]
                )
            )
        except Exception as e:
            logging.error(f"獲取任務狀態失敗: {e}", exc_info=True)
            context.set_code(grpc.StatusCode.INTERNAL)
            context.set_details(str(e))
            return nodepool_pb2.GetTaskStatusResponse(success=False, message=str(e))

    def GetAllTasks(self, request, context):
        """獲取所有任務列表，用於主控端同步"""
        try:
            # 驗證token
            user_id = self._verify_token(request.token)
            if not user_id:
                context.set_code(grpc.StatusCode.UNAUTHENTICATED)
                context.set_details("Invalid token")
                return nodepool_pb2.GetAllTasksResponse(success=False, message="Invalid token")

            # 從Redis中查詢所有任務鍵
            task_keys = self.task_manager.redis_client.keys("task:*")
            all_tasks = []
            
            for key in task_keys:
                task_id = key.split(":", 1)[1]
                task_info = self.task_manager.get_task_info(task_id, include_zip=False)
                
                if task_info:
                    # 只返回屬於當前用戶的任務
                    if task_info.get("user_id") and str(task_info.get("user_id")) == str(user_id):
                        # 確保時間格式正確
                        created_at = ""
                        updated_at = ""
                        
                        try:
                            # 如果時間戳是數字，轉化為字符串格式
                            if task_info.get("created_at"):
                                created_timestamp = float(task_info.get("created_at", 0))
                                created_at = time.strftime("%Y-%m-%d %H:%M:%S", time.localtime(created_timestamp))
                            
                            if task_info.get("updated_at"):
                                updated_timestamp = float(task_info.get("updated_at", 0))
                                updated_at = time.strftime("%Y-%m-%d %H:%M:%S", time.localtime(updated_timestamp))
                        except (ValueError, TypeError) as e:
                            logging.warning(f"任務 {task_id} 時間戳格式錯誤: {e}")
                        
                        # 創建TaskStatus對象
                        task_status = nodepool_pb2.TaskStatus(
                            task_id=task_id,
                            status=task_info.get("status", "UNKNOWN"),
                            created_at=created_at,
                            updated_at=updated_at,
                            assigned_node=task_info.get("assigned_node", "")
                        )
                        all_tasks.append(task_status)
            
            logging.info(f"返回用戶 {user_id} 的 {len(all_tasks)} 個任務狀態")
            return nodepool_pb2.GetAllTasksResponse(
                success=True,
                message=f"成功獲取用戶 {user_id} 的任務列表",
                tasks=all_tasks
            )
        except Exception as e:
            error_msg = f"獲取用戶任務列表失敗: {str(e)}"
            logging.error(error_msg, exc_info=True)
            context.set_code(grpc.StatusCode.INTERNAL)
            context.set_details(error_msg)
            return nodepool_pb2.GetAllTasksResponse(
                success=False,
                message=error_msg
            )

    def _verify_token(self, token):
        """驗證token並返回用戶ID"""
        try:
            # 這裡需要實現token驗證邏輯
            # 返回用戶ID或None
            from user_manager import UserManager
            user_manager = UserManager()
            return user_manager.verify_token(token)
        except Exception as e:
            logging.error(f"Token驗證失敗: {e}", exc_info=True)
            return None