# node_pool/node_manager.py
import grpc
import logging
import redis
import time
import nodepool_pb2
# nodepool_pb2_grpc 的導入在這個文件中不再需要，因為 Servicer 被移除了

logging.basicConfig(level=logging.INFO, format='%(asctime)s - %(levelname)s - %(message)s')

class NodeManager:
    def __init__(self):
        # 假設 Redis 運行在 localhost:6379, db=0
        # decode_responses=True 讓 Redis 返回字符串而不是 bytes
        self.redis_client = redis.Redis(host='localhost', port=6379, db=0, decode_responses=True)
        try:
            self.redis_client.ping()
            logging.info("Redis 連線成功")
        except redis.RedisError as e:
            logging.error(f"Redis 連線失敗: {e}")
            raise # 啟動時連不上 Redis 就拋出異常

    def register_worker_node(self, node_id, hostname, cpu_cores, memory_gb, cpu_score, gpu_score, gpu_memory_gb, location, port, gpu_name, ip=None):
        """註冊或更新工作節點資訊，新增 ip 欄位"""
        node_key = f"node:{node_id}"
        try:
            # 获取现有节点信息
            existing_info = self.redis_client.hgetall(node_key)
            
            # 准备新的节点信息
            node_info = {
                "hostname": hostname,
                "cpu_cores": str(cpu_cores),
                "memory_gb": str(memory_gb),
                "cpu_score": str(cpu_score),
                "gpu_score": str(gpu_score),
                "gpu_memory_gb": str(gpu_memory_gb),
                "location": location,
                "port": str(port),
                "gpu_name": gpu_name,
                "ip": ip or "",  # 新增
                "status": "Idle",
                "last_heartbeat": str(int(time.time())),
                "is_registered": "True"
            }
            
            # 如果是重新注册，保留原有的CPT余额
            if existing_info and "cpt_balance" in existing_info:
                node_info["cpt_balance"] = existing_info["cpt_balance"]
            else:
                node_info["cpt_balance"] = "0"  # 新节点初始余额为0
            
            # 更新节点信息
            for field, value in node_info.items():
                self.redis_client.hset(node_key, field, value)
                
            logging.info(f"節點 {node_id} (GPU: {gpu_name}) 註冊/更新成功，當前餘額: {node_info['cpt_balance']}")
            return True, f"節點 {node_id} 註冊成功"
        except redis.RedisError as e:
            logging.error(f"Redis 錯誤，節點 {node_id} 註冊失敗: {e}")
            return False, f"Redis 錯誤: {e}"
        except Exception as e:
            logging.error(f"註冊節點 {node_id} 時發生未知錯誤: {e}", exc_info=True)
            return False, f"未知伺服器錯誤: {e}"

    # --- 修改 report_status ---
    def report_status(self, node_id, status_message):
        """節點報告狀態並更新心跳"""
        node_key = f"node:{node_id}"
        try:
            # 使用 exists 檢查鍵是否存在
            if not self.redis_client.exists(node_key):
                logging.warning(f"嘗試報告狀態失敗: 節點 {node_id} 不存在")
                return False, f"節點 {node_id} 不存在"

            # 获取当前节点信息
            node_info = self.redis_client.hgetall(node_key)
            if not node_info:
                logging.warning(f"節點 {node_id} 信息不完整")
                return False, f"節點 {node_id} 信息不完整"

            # 检查节点是否已注册
            if node_info.get("is_registered") != "True":
                logging.warning(f"節點 {node_id} 未註冊")
                return False, f"節點 {node_id} 未註冊"

            # 更新狀態和心跳
            update_data = {
                "status": status_message,
                "last_heartbeat": str(int(time.time()))
            }

            # 如果状态包含余额信息，更新余额
            if "CPT Balance:" in status_message:
                try:
                    balance = int(status_message.split("CPT Balance:")[1].strip().split()[0])
                    update_data["cpt_balance"] = str(balance)
                except (ValueError, IndexError):
                    logging.warning(f"無法從狀態消息中解析餘額: {status_message}")

            # 更新节点信息
            for field, value in update_data.items():
                self.redis_client.hset(node_key, field, value)

            logging.debug(f"節點 {node_id} 狀態更新: {status_message}")
            return True, f"節點 {node_id} 狀態更新成功"
        except redis.RedisError as e:
            logging.error(f"Redis 錯誤，節點 {node_id} 狀態報告失敗: {e}")
            return False, f"Redis 錯誤: {e}"
        except Exception as e:
            logging.error(f"更新節點 {node_id} 狀態時發生未知錯誤: {e}", exc_info=True)
            return False, f"未知伺服器錯誤: {e}"

    # --- 修改 get_node_list ---
    def get_node_list(self):
        """獲取所有註冊節點的資訊列表"""
        try:
            nodes = []
            node_keys = self.redis_client.keys("node:*")
            logging.info(f"從 Redis 發現 {len(node_keys)} 個節點 key")
            if not node_keys:
                logging.info("Redis 中未找到節點資訊")
                return nodes

            current_time = time.time()
            for key in node_keys:
                node_info = self.redis_client.hgetall(key) # 得到字串字典
                node_id_str = key.split(":", 1)[1] # 從 key 獲取 node_id

                # 檢查核心字段
                essential_keys = ["hostname", "status", "port", "last_heartbeat", "is_registered"]
                missing_essentials = [k for k in essential_keys if k not in node_info or not node_info[k]]

                if missing_essentials:
                    logging.warning(f"節點 {node_id_str} 核心資訊不完整 (缺少或為空: {missing_essentials})，跳過: {node_info}")
                    continue

                # 檢查節點心跳時間，如果超過 30 秒沒有心跳，認為節點不可用
                try:
                    last_heartbeat = float(node_info.get("last_heartbeat", 0))
                    if current_time - last_heartbeat > 30:
                        logging.warning(f"節點 {node_id_str} 心跳超時，最後心跳時間: {last_heartbeat}")
                        continue
                except (ValueError, TypeError):
                    logging.warning(f"節點 {node_id_str} 心跳時間格式錯誤: {node_info.get('last_heartbeat')}")
                    continue

                # 檢查節點註冊狀態
                if node_info.get("is_registered") != "True":
                    logging.debug(f"節點 {node_id_str} 未註冊")
                    continue

                # 檢查節點狀態
                if node_info.get("status") != "Idle":
                    logging.debug(f"節點 {node_id_str} 狀態不是 Idle，當前狀態: {node_info.get('status')}")
                    continue

                # 嘗試轉換並添加到列表
                try:
                    nodes.append(nodepool_pb2.WorkerNodeInfo(
                        node_id=node_id_str,
                        hostname=node_info.get("hostname", "N/A"),
                        cpu_cores=int(node_info.get("cpu_cores", 0)),
                        memory_gb=int(node_info.get("memory_gb", 0)),
                        status=node_info.get("status", "Unknown"),
                        last_heartbeat=float(node_info.get("last_heartbeat", 0.0)),
                        cpu_score=int(node_info.get("cpu_score", 0)),
                        gpu_score=int(node_info.get("gpu_score", 0)),
                        gpu_memory_gb=int(node_info.get("gpu_memory_gb", 0)),
                        location=node_info.get("location", "N/A"),
                        port=int(node_info.get("port", 0)),
                        gpu_name=node_info.get("gpu_name", "N/A")
                        # ip 欄位移除，proto 沒有 ip 欄位
                    ))
                except (ValueError, TypeError) as conv_err:
                    logging.warning(f"轉換節點 {node_id_str} 數值時出錯: {conv_err}, data: {node_info}")
                    continue

            logging.info(f"成功獲取並處理 {len(nodes)} 個可用節點")
            return nodes
        except redis.RedisError as e:
            logging.error(f"Redis 錯誤，獲取節點列表失敗: {e}")
            return []
        except Exception as e:
            logging.error(f"獲取節點列表時發生未知錯誤: {e}", exc_info=True)
            return []

    def get_available_nodes(self, memory_gb_req, cpu_score_req, gpu_score_req, gpu_memory_gb_req, location_req, gpu_name_req):
        """根據需求查找可用的空閒節點"""
        try:
            available_nodes_info = []
            all_nodes = self.get_node_list() # 直接復用 get_node_list 獲取已處理好的節點列表

            for node in all_nodes:
                # 檢查狀態、位置和資源是否滿足要求
                is_status_ok = (node.status == "Idle") # 確保使用 Idle
                is_location_ok = (location_req == "Any" or node.location == location_req)
                is_resource_ok = (
                    node.memory_gb >= memory_gb_req and
                    node.cpu_score >= cpu_score_req and
                    node.gpu_score >= gpu_score_req and
                    node.gpu_memory_gb >= gpu_memory_gb_req
                )
                # 檢查 GPU 名稱，如果請求為空或 "Any"，則不限制；否則要求精確匹配
                is_gpu_name_ok = (not gpu_name_req or gpu_name_req.lower() == "any" or node.gpu_name == gpu_name_req)

                if is_status_ok and is_location_ok and is_resource_ok and is_gpu_name_ok:
                    available_nodes_info.append(node) # 添加符合條件的節點信息對象

            logging.info(f"根據需求找到 {len(available_nodes_info)} 個可用節點")
            return available_nodes_info # 返回 WorkerNodeInfo 對象列表
        except Exception as e:
            logging.error(f"查找可用節點時發生錯誤: {e}", exc_info=True)
            return []

    def health_check(self):
        """执行 Redis ping 作为健康检查"""
        try:
            if self.redis_client.ping():
                return True, "Redis connection healthy"
            else:
                # 理論上 ping() 失敗會拋異常，但以防萬一
                return False, "Redis ping failed"
        except redis.RedisError as e:
            return False, f"Redis connection error: {e}"
        except Exception as e:
             return False, f"Health check unexpected error: {e}"
    
    def get_node_info(self, node_id):
        """取得單一節點資訊（dict）"""
        node_key = f"node:{node_id}"
        try:
            node_info = self.redis_client.hgetall(node_key)
            if not node_info:
                return None
            return {
                "node_id": node_id,
                "ip": node_info.get("ip", "127.0.0.1"),
                "port": int(node_info.get("port", 0)),
                # ...可擴充其他欄位...
            }
        except Exception as e:
            logging.error(f"取得節點 {node_id} 資訊失敗: {e}")
            return None
