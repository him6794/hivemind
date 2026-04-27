# node_pool/node_manager.py
import grpc
import logging
import redis
import time
import nodepool_pb2
from user_manager import UserManager
from config import Config

logging.basicConfig(level=logging.INFO, format='%(asctime)s - %(levelname)s - %(message)s')

class NodeManager:
    def __init__(self, redis_client=None):
        if redis_client is not None:
            self.redis_client = redis_client
            return

        self.redis_client = redis.Redis(
            host=getattr(Config, 'REDIS_HOST', 'localhost'),
            port=int(getattr(Config, 'REDIS_PORT', 6379)),
            db=int(getattr(Config, 'REDIS_DB', 0)),
            decode_responses=True,
        )
        try:
            self.redis_client.ping()
            logging.info("Redis 連線成功")
        except redis.RedisError as e:
            logging.error(f"Redis 連線失敗: {e}")
            raise # 啟動時連不上 Redis 就拋出異常

    def cleanup_offline_nodes(self, offline_threshold: int = 300) -> int:
        """清理超過 offline_threshold 秒未心跳的節點

        - 掃描 Redis 中所有 node:* keys
        - 如果 now - last_heartbeat > threshold，則刪除該節點 key
        - 返回被清理的節點數量

        備註：為安全起見，僅刪除節點主 key（node:<id>）。
        """
        try:
            now = time.time()
            cleaned = 0
            keys = self.redis_client.keys("node:*")
            for key in keys:
                try:
                    info = self.redis_client.hgetall(key)
                    last_hb = float(info.get("last_heartbeat", 0))
                    if last_hb == 0 or (now - last_hb) > offline_threshold:
                        node_id = key.split(":", 1)[1]
                        self.redis_client.delete(key)
                        cleaned += 1
                        logging.info(f"清理離線節點 {node_id} (last_heartbeat={last_hb})")
                except Exception as ie:
                    logging.warning(f"檢查/清理 {key} 發生例外: {ie}")
            logging.info(f"離線節點清理完成，共清理 {cleaned} 個")
            return cleaned
        except Exception as e:
            logging.error(f"清理離線節點失敗: {e}", exc_info=True)
            return 0

    def register_worker_node(self, node_id, hostname, cpu_cores, memory_gb, cpu_score, gpu_score, gpu_memory_gb, location, port, gpu_name, docker_status="unknown"):
        """註冊或更新工作節點資訊 - 增加Docker狀態和資源管理"""
        node_key = f"node:{node_id}"
        try:
            # 獲取現有節點信息
            existing_info = self.redis_client.hgetall(node_key)
            
            # 查詢該節點的信用評分（假設 node_id 就是用戶名）
            user_manager = UserManager()
            credit_score = user_manager.get_user_credit_score(node_id)
            
            # 確定信任等級 - 無Docker的強制歸類為低信任
            if docker_status == "disabled":
                trust_level = 'low'
                logging.info(f"節點 {node_id} 無Docker服務，強制歸類為低信任等級")
            elif credit_score >= 100:
                trust_level = 'high'
            elif credit_score >= 50:
                trust_level = 'normal'
            else:
                trust_level = 'low'
            
            # 準備新的節點信息 - 包含資源追蹤
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
                "docker_status": docker_status,
                "status": "Idle",
                "last_heartbeat": str(int(time.time())),
                "is_registered": "True",
                "credit_score": str(credit_score),
                "trust_level": trust_level,
                # 資源追蹤 - 總資源
                "total_cpu_score": str(cpu_score),
                "total_memory_gb": str(memory_gb), 
                "total_gpu_score": str(gpu_score),
                "total_gpu_memory_gb": str(gpu_memory_gb),
                # 可用資源 (初始時等於總資源)
                "available_cpu_score": str(cpu_score),
                "available_memory_gb": str(memory_gb),
                "available_gpu_score": str(gpu_score), 
                "available_gpu_memory_gb": str(gpu_memory_gb),
                # 任務管理
                "current_tasks": "0",
                "running_task_ids": ""  # 逗號分隔的任務ID列表
            }
            
            # 如果是重新注册，保留原有的CPT余额和運行中的任務信息
            if existing_info:
                if "cpt_balance" in existing_info:
                    node_info["cpt_balance"] = existing_info["cpt_balance"]
                if "current_tasks" in existing_info:
                    node_info["current_tasks"] = existing_info["current_tasks"]
                if "running_task_ids" in existing_info:
                    node_info["running_task_ids"] = existing_info["running_task_ids"]
                
                # 若有任務在跑，保留原有 status（不強制重設為 Idle）
                current_task_count = int(existing_info.get("current_tasks", 0) or 0)
                if current_task_count > 0 and "status" in existing_info:
                    node_info["status"] = existing_info["status"]
                
                # 關鍵邏輯:如果沒有任務在跑,直接用新的總資源覆蓋可用資源
                
                if current_task_count == 0:
                    # 沒有任務在跑,直接覆蓋為新的總資源
                    node_info["available_cpu_score"] = str(cpu_score)
                    node_info["available_memory_gb"] = str(memory_gb)
                    node_info["available_gpu_score"] = str(gpu_score)
                    node_info["available_gpu_memory_gb"] = str(gpu_memory_gb)
                    logging.info(f"節點 {node_id} 無任務運行,可用資源更新為總資源: CPU={cpu_score}, Mem={memory_gb}GB, GPU={gpu_score}, GPU_Mem={gpu_memory_gb}GB")
                else:
                    # 有任務在跑,保留已分配的資源狀態(但要考慮總資源變化)
                    old_total_cpu = float(existing_info.get("total_cpu_score", 0) or 0)
                    new_total_cpu = float(cpu_score)
                    
                    if old_total_cpu > 0 and abs(new_total_cpu - old_total_cpu) > 0.1:
                        # 總資源改變了，按比例調整可用資源
                        old_avail_cpu = float(existing_info.get("available_cpu_score", 0) or 0)
                        usage_ratio = (old_total_cpu - old_avail_cpu) / old_total_cpu if old_total_cpu > 0 else 0
                        new_avail_cpu = new_total_cpu * (1 - usage_ratio)
                        node_info["available_cpu_score"] = str(int(round(new_avail_cpu)))
                        logging.info(
                            f"節點 {node_id} 總資源從 {old_total_cpu} 變更為 {new_total_cpu}，"
                            f"可用資源從 {old_avail_cpu} 調整為 {int(round(new_avail_cpu))}"
                        )
                    elif "available_cpu_score" in existing_info:
                        # 總資源沒變，保留原有可用資源
                        node_info["available_cpu_score"] = existing_info["available_cpu_score"]
                    
                    # GPU 和記憶體同樣處理
                    old_total_gpu = float(existing_info.get("total_gpu_score", 0) or 0)
                    new_total_gpu = float(gpu_score)
                    if old_total_gpu > 0 and abs(new_total_gpu - old_total_gpu) > 0.1:
                        old_avail_gpu = float(existing_info.get("available_gpu_score", 0) or 0)
                        usage_ratio = (old_total_gpu - old_avail_gpu) / old_total_gpu if old_total_gpu > 0 else 0
                        new_avail_gpu = new_total_gpu * (1 - usage_ratio)
                        node_info["available_gpu_score"] = str(int(round(new_avail_gpu)))
                    elif "available_gpu_score" in existing_info:
                        node_info["available_gpu_score"] = existing_info["available_gpu_score"]
                    
                    old_total_mem = float(existing_info.get("total_memory_gb", 0) or 0)
                    new_total_mem = float(memory_gb)
                    if old_total_mem > 0 and abs(new_total_mem - old_total_mem) > 0.1:
                        old_avail_mem = float(existing_info.get("available_memory_gb", 0) or 0)
                        usage_ratio = (old_total_mem - old_avail_mem) / old_total_mem if old_total_mem > 0 else 0
                        new_avail_mem = new_total_mem * (1 - usage_ratio)
                        node_info["available_memory_gb"] = str(round(new_avail_mem, 1))
                    elif "available_memory_gb" in existing_info:
                        node_info["available_memory_gb"] = existing_info["available_memory_gb"]
                    
                    old_total_gpu_mem = float(existing_info.get("total_gpu_memory_gb", 0) or 0)
                    new_total_gpu_mem = float(gpu_memory_gb)
                    if old_total_gpu_mem > 0 and abs(new_total_gpu_mem - old_total_gpu_mem) > 0.1:
                        old_avail_gpu_mem = float(existing_info.get("available_gpu_memory_gb", 0) or 0)
                        usage_ratio = (old_total_gpu_mem - old_avail_gpu_mem) / old_total_gpu_mem if old_total_gpu_mem > 0 else 0
                        new_avail_gpu_mem = new_total_gpu_mem * (1 - usage_ratio)
                        node_info["available_gpu_memory_gb"] = str(round(new_avail_gpu_mem, 1))
                    elif "available_gpu_memory_gb" in existing_info:
                        node_info["available_gpu_memory_gb"] = existing_info["available_gpu_memory_gb"]
            else:
                node_info["cpt_balance"] = "0"
            
            # 夾限：確保 available 不超過 total 且不小於 0
            try:
                for pair in (
                    ("available_cpu_score", "total_cpu_score"),
                    ("available_memory_gb", "total_memory_gb"),
                    ("available_gpu_score", "total_gpu_score"),
                    ("available_gpu_memory_gb", "total_gpu_memory_gb"),
                ):
                    a_key, t_key = pair
                    a_val = float(node_info.get(a_key, 0) or 0)
                    t_val = float(node_info.get(t_key, 0) or 0)
                    a_val = max(0.0, min(a_val, t_val))
                    # 寫回為字串（保持整體一致）
                    if a_key.endswith("_memory_gb"):
                        node_info[a_key] = str(a_val)
                    else:
                        node_info[a_key] = str(int(round(a_val)))
            except Exception:
                pass

            # 更新节点信息
            for field, value in node_info.items():
                self.redis_client.hset(node_key, field, value)
            
            docker_note = f"Docker: {docker_status}"
            if docker_status == "disabled":
                docker_note += " (低信任群組)"
                
            logging.info(f"節點 {node_id} (GPU: {gpu_name}, {docker_note}, 信用評分: {credit_score}, 信任等級: {trust_level}) 註冊/更新成功")
            return True, f"節點 {node_id} 註冊成功 (信任等級: {trust_level})"
        except redis.RedisError as e:
            logging.error(f"Redis 錯誤，節點 {node_id} 註冊失敗: {e}")
            return False, f"Redis 錯誤: {e}"
        except Exception as e:
            logging.error(f"註冊節點 {node_id} 時發生未知錯誤: {e}", exc_info=True)
            return False, f"未知伺服器錯誤: {e}"

    # --- 修改 report_status ---
    def report_status(self, request, context=None):
        """處理節點狀態報告"""
        # 修正：正確處理參數類型
        if isinstance(request, str):
            # 如果傳入的是字符串，說明是內部調用
            node_id = request
            status_message = context if isinstance(context, str) else "Idle"
            is_grpc_call = False
        else:
            # 如果是 gRPC 請求對象，按正常方式處理
            node_id = request.node_id
            # RunningStatusRequest 使用欄位名 'status'
            status_message = getattr(request, 'status', None) or getattr(request, 'status_message', 'Idle')
            is_grpc_call = True

        # 檢查節點是否存在
        if not self.redis_client.exists(f"node:{node_id}"):
            error_msg = f"節點 {node_id} 不存在，請先註冊"
            logging.warning(error_msg)
            if is_grpc_call:
                return nodepool_pb2.StatusResponse(success=False, message=error_msg)
            else:
                return False, error_msg

        try:
            # 獲取現有節點信息
            node_info = self.get_node_info(node_id)
            if not node_info:
                error_msg = f"無法獲取節點 {node_id} 的信息"
                logging.error(error_msg)
                if is_grpc_call:
                    return nodepool_pb2.StatusResponse(success=False, message=error_msg)
                else:
                    return False, error_msg

            # 更新狀態和心跳時間
            current_time = time.time()
            
            # 保持原有的 IP 和其他信息，只更新狀態和心跳
            update_data = {
                "status": status_message,
                "last_heartbeat": str(current_time),
                "updated_at": str(current_time)
            }
            
            # 更新 Redis 中的節點信息
            self.redis_client.hset(f"node:{node_id}", mapping=update_data)
            
            logging.debug(f"節點 {node_id} 狀態已更新: {status_message}，心跳時間: {current_time}")
            
            if is_grpc_call:
                # 與 proto 對應：ReportStatus 回傳 RunningStatusResponse
                return nodepool_pb2.RunningStatusResponse(success=True, message="狀態報告成功")
            else:
                return True, "狀態更新成功"

        except Exception as e:
            error_msg = f"更新節點 {node_id} 狀態失敗: {e}"
            logging.error(error_msg, exc_info=True)
            if is_grpc_call:
                return nodepool_pb2.RunningStatusResponse(success=False, message=error_msg)
            else:
                return False, error_msg

    def update_node_usage(self, node_id: str, cpu_usage: int | float = 0, memory_usage: int | float = 0,
                          gpu_usage: int | float = 0, gpu_memory_usage: int | float = 0) -> tuple[bool, str]:
        """更新節點即時資源使用狀態

        - 寫入 Redis 欄位：current_cpu_usage、current_memory_usage、current_gpu_usage、current_gpu_memory_usage
        - 更新 last_heartbeat 與 updated_at
        - 夾限 CPU/GPU 百分比於 [0,100]，記憶體使用量不為負
        """
        try:
            node_key = f"node:{node_id}"
            if not self.redis_client.exists(node_key):
                return False, f"節點 {node_id} 不存在"

            # 數值處理與夾限
            try:
                cpu_val = max(0.0, min(100.0, float(cpu_usage)))
            except Exception:
                cpu_val = 0.0
            try:
                mem_val = max(0.0, float(memory_usage))
            except Exception:
                mem_val = 0.0
            try:
                gpu_val = max(0.0, min(100.0, float(gpu_usage)))
            except Exception:
                gpu_val = 0.0
            try:
                gpumem_val = max(0.0, float(gpu_memory_usage))
            except Exception:
                gpumem_val = 0.0

            now = time.time()
            mapping = {
                "current_cpu_usage": str(int(round(cpu_val))),
                "current_memory_usage": str(int(round(mem_val))),
                "current_gpu_usage": str(int(round(gpu_val))),
                "current_gpu_memory_usage": str(int(round(gpumem_val))),
                "last_heartbeat": str(now),
                "updated_at": str(now)
            }
            self.redis_client.hset(node_key, mapping=mapping)
            return True, "節點使用狀態已更新"
        except Exception as e:
            logging.error(f"更新節點 {node_id} 使用狀態失敗: {e}", exc_info=True)
            return False, str(e)

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
            offline_threshold = getattr(Config, 'HEARTBEAT_ONLINE_THRESHOLD_SECONDS', 180)
            for key in node_keys:
                node_info = self.redis_client.hgetall(key) # 得到字串字典
                node_id_str = key.split(":", 1)[1] # 從 key 獲取 node_id

                # 檢查核心字段
                essential_keys = ["hostname", "status", "port", "last_heartbeat", "is_registered"]
                missing_essentials = [k for k in essential_keys if k not in node_info or not node_info[k]]

                if missing_essentials:
                    logging.warning(f"節點 {node_id_str} 核心資訊不完整 (缺少或為空: {missing_essentials})，跳過: {node_info}")
                    continue

                # 檢查節點心跳時間，如果超過閾值沒有心跳，認為節點不可用
                try:
                    last_heartbeat = float(node_info.get("last_heartbeat", 0))
                    if current_time - last_heartbeat > offline_threshold:
                        logging.info(f"節點 {node_id_str} 心跳超時（>{offline_threshold}s），最後心跳時間: {last_heartbeat}")
                        continue
                except (ValueError, TypeError):
                    logging.warning(f"節點 {node_id_str} 心跳時間格式錯誤: {node_info.get('last_heartbeat')}")
                    continue

                # 檢查節點註冊狀態
                if node_info.get("is_registered") != "True":
                    logging.debug(f"節點 {node_id_str} 未註冊")
                    continue

                # 嘗試轉換並添加到列表
                try:
                    # 創建節點資訊字典而不是 protobuf 對象
                    node_data = {
                        'node_id': node_id_str,
                        'hostname': node_info.get("hostname", "N/A"),
                        'cpu_cores': int(node_info.get("cpu_cores", 0)),
                        'memory_gb': float(node_info.get("memory_gb", 0)),
                        'status': node_info.get("status", "Unknown"),
                        'last_heartbeat': float(node_info.get("last_heartbeat", 0.0)),
                        'cpu_score': int(node_info.get("cpu_score", 0)),
                        'gpu_score': int(node_info.get("gpu_score", 0)),
                        'gpu_memory_gb': float(node_info.get("gpu_memory_gb", 0)),
                        'location': node_info.get("location", "N/A"),
                        'port': int(node_info.get("port", 0)),
                        'gpu_name': node_info.get("gpu_name", "N/A"),
                        # 前端常用的附加欄位
                        'is_online': True,  # 走到這裡代表已通過心跳門檻
                        'docker_status': node_info.get("docker_status", "unknown"),
                        'trust_level': node_info.get("trust_level", "low"),
                        'credit_score': int(node_info.get("credit_score", 0)),
                        'current_tasks': int(node_info.get("current_tasks", 0)),
                        # 即時使用率（若有由 Worker 回報）
                        'current_cpu_usage': int(float(node_info.get("current_cpu_usage", 0) or 0)),
                        'current_memory_usage': int(float(node_info.get("current_memory_usage", 0) or 0)),
                        'current_gpu_usage': int(float(node_info.get("current_gpu_usage", 0) or 0)),
                        'current_gpu_memory_usage': int(float(node_info.get("current_gpu_memory_usage", 0) or 0)),
                        # 資源總量/可用量（若無則以舊欄位回退）
                        'total_cpu_score': int(node_info.get("total_cpu_score", node_info.get("cpu_score", 0))),
                        'total_memory_gb': float(node_info.get("total_memory_gb", node_info.get("memory_gb", 0))),
                        'total_gpu_score': int(node_info.get("total_gpu_score", node_info.get("gpu_score", 0))),
                        'available_cpu_score': int(node_info.get("available_cpu_score", node_info.get("cpu_score", 0))),
                        'available_memory_gb': float(node_info.get("available_memory_gb", node_info.get("memory_gb", 0))),
                        'available_gpu_score': int(node_info.get("available_gpu_score", node_info.get("gpu_score", 0)))
                    }
                    # 夾限 available <= total，避免前端顯示負百分比
                    node_data['available_cpu_score'] = max(0, min(node_data['available_cpu_score'], node_data['total_cpu_score']))
                    node_data['available_memory_gb'] = max(0.0, min(node_data['available_memory_gb'], node_data['total_memory_gb']))
                    node_data['available_gpu_score'] = max(0, min(node_data['available_gpu_score'], node_data['total_gpu_score']))
                    nodes.append(node_data)
                except (ValueError, TypeError) as conv_err:
                    logging.warning(f"轉換節點 {node_id_str} 數值時出錯: {conv_err}, data: {node_info}")
                    continue

            logging.info(f"成功獲取並處理 {len(nodes)} 個可用節點")
            return nodes
        except redis.RedisError as e:
            logging.error(f"Redis 錯誤，獲取節點列表失敗: {e}")
            return []

    def get_cluster_health_status(self, offline_threshold: int | None = None) -> dict:
        """計算並返回集群健康狀態統計

        回傳內容包含：
        - online_nodes, offline_nodes, busy_nodes, idle_nodes, total_tasks
        - total_resources/available_resources（cpu_score/memory_gb/gpu_score）
        - resource_utilization（百分比）
        - timestamp
        """
        try:
            # 讀取預設閾值
            if offline_threshold is None:
                offline_threshold = getattr(Config, 'HEARTBEAT_ONLINE_THRESHOLD_SECONDS', 180)

            stats = {
                'online_nodes': 0,
                'offline_nodes': 0,
                'busy_nodes': 0,
                'idle_nodes': 0,
                'total_tasks': 0,
                'total_resources': {
                    'cpu_score': 0,
                    'memory_gb': 0,
                    'gpu_score': 0
                },
                'available_resources': {
                    'cpu_score': 0,
                    'memory_gb': 0,
                    'gpu_score': 0
                },
                'resource_utilization': {
                    'cpu_percent': 0,
                    'memory_percent': 0,
                    'gpu_percent': 0
                },
                'timestamp': int(time.time())
            }

            now = time.time()
            keys = self.redis_client.keys("node:*")
            for key in keys:
                info = self.redis_client.hgetall(key)
                node_id = key.split(":", 1)[1]
                try:
                    last_hb = float(info.get("last_heartbeat", 0))
                except (TypeError, ValueError):
                    last_hb = 0.0

                is_online = (now - last_hb) < offline_threshold and info.get("is_registered", "True") == "True"

                if is_online:
                    stats['online_nodes'] += 1
                    current_tasks = int(info.get("current_tasks", 0) or 0)
                    if current_tasks > 0:
                        stats['busy_nodes'] += 1
                    else:
                        stats['idle_nodes'] += 1
                    stats['total_tasks'] += current_tasks
                else:
                    stats['offline_nodes'] += 1

                # 累計資源（若缺少欄位則以0處理）
                try:
                    stats['total_resources']['cpu_score'] += int(info.get('total_cpu_score', info.get('cpu_score', 0)) or 0)
                    stats['total_resources']['memory_gb'] += float(info.get('total_memory_gb', info.get('memory_gb', 0)) or 0)
                    stats['total_resources']['gpu_score'] += int(info.get('total_gpu_score', info.get('gpu_score', 0)) or 0)

                    stats['available_resources']['cpu_score'] += int(info.get('available_cpu_score', info.get('cpu_score', 0)) or 0)
                    stats['available_resources']['memory_gb'] += float(info.get('available_memory_gb', info.get('memory_gb', 0)) or 0)
                    stats['available_resources']['gpu_score'] += int(info.get('available_gpu_score', info.get('gpu_score', 0)) or 0)
                except Exception:
                    # 某些欄位轉型失敗不應中斷統計
                    pass

            # 計算利用率（夾限 available <= total 並將百分比夾到 [0,100]）
            total_cpu = stats['total_resources']['cpu_score']
            if total_cpu > 0:
                avail = min(max(stats['available_resources']['cpu_score'], 0), total_cpu)
                util = (1 - (avail / total_cpu)) * 100
                stats['resource_utilization']['cpu_percent'] = round(max(0, min(100, util)), 2)

            total_mem = stats['total_resources']['memory_gb']
            if total_mem > 0:
                avail = min(max(stats['available_resources']['memory_gb'], 0.0), float(total_mem))
                util = (1 - (avail / total_mem)) * 100
                stats['resource_utilization']['memory_percent'] = round(max(0, min(100, util)), 2)

            total_gpu = stats['total_resources']['gpu_score']
            if total_gpu > 0:
                avail = min(max(stats['available_resources']['gpu_score'], 0), total_gpu)
                util = (1 - (avail / total_gpu)) * 100
                stats['resource_utilization']['gpu_percent'] = round(max(0, min(100, util)), 2)

            return stats
        except Exception as e:
            logging.error(f"計算集群健康狀態失敗: {e}", exc_info=True)
            return {}
        except Exception as e:
            logging.error(f"獲取節點列表時發生未知錯誤: {e}", exc_info=True)
            return []

    def get_available_nodes(self):
        """獲取所有可用節點的列表"""
        try:
            keys = self.redis_client.keys("node:*")
            available_nodes = []
            for key in keys:
                node_info = self.redis_client.hgetall(key)
                # 注意：status 可能被寫成「Running N tasks - Light/Medium/...」，
                # 因此這裡不以 status == "Idle" 作為唯一可用判定。
                load_status = (node_info.get("load_status") or "").strip()
                if load_status == "Heavy Load":
                    continue

                # 若有心跳資訊且已過期，可視為不可用；缺少心跳(0)則不強制排除，避免測試/開發資料被過濾
                try:
                    last_hb = float(node_info.get("last_heartbeat", 0) or 0)
                except Exception:
                    last_hb = 0.0
                if last_hb > 0:
                    try:
                        threshold = int(getattr(Config, 'HEARTBEAT_ONLINE_THRESHOLD_SECONDS', 180))
                        if (time.time() - last_hb) > threshold:
                            continue
                    except Exception:
                        pass

                try:
                    current_tasks = int(node_info.get("current_tasks", 0) or 0)
                except Exception:
                    current_tasks = 0

                try:
                    last_assigned_at = float(node_info.get("last_assigned_at", 0) or 0)
                except Exception:
                    last_assigned_at = 0.0

                available_nodes.append({
                    "node_id": key.split(":")[1],
                    "hostname": node_info.get("hostname"),
                    "port": node_info.get("port"),
                    "current_tasks": current_tasks,
                    "last_assigned_at": last_assigned_at,
                    "available_cpu_score": int(float(node_info.get("available_cpu_score", 0) or 0)),
                    "available_memory_gb": float(node_info.get("available_memory_gb", 0) or 0),
                    "available_gpu_score": int(float(node_info.get("available_gpu_score", 0) or 0)),
                    "available_gpu_memory_gb": float(node_info.get("available_gpu_memory_gb", 0) or 0)
                })
            # 負載平衡：
            # 1) 優先挑 current_tasks 少
            # 2) 若任務數相同，優先選「更久沒被分配」的節點（避免一直打到同一台效能好的）
            # 3) 最後才用 available_cpu_score 當 tie-breaker，避免選到資源太弱的節點
            available_nodes.sort(key=lambda x: (x["current_tasks"], x["last_assigned_at"], -x["available_cpu_score"]))
            return available_nodes
        except Exception as e:
            logging.error(f"獲取可用節點失敗: {e}", exc_info=True)
            return []

    def assign_task_to_node(self, node, task_id, task_zip):
        """分配任務到指定節點"""
        try:
            node_key = f"node:{node['node_id']}"
            # 更新節點的任務信息
            current_tasks = self.redis_client.hget(node_key, "running_task_ids") or ""
            task_list = current_tasks.split(",") if current_tasks else []
            task_list.append(task_id)
            self.redis_client.hset(node_key, "running_task_ids", ",".join(task_list))
            self.redis_client.hincrby(node_key, "current_tasks", 1)

            # 標記為 Running，避免面板/監控誤判為閒置（實際狀態可能會被 worker 回報覆寫）
            try:
                self.redis_client.hset(node_key, "status", "Running")
            except Exception:
                pass

            # 記錄最後一次分配時間，用於同負載節點的輪轉
            try:
                self.redis_client.hset(node_key, "last_assigned_at", str(time.time()))
            except Exception:
                pass

            # 模擬任務分發（實際應該是通過 gRPC 或其他方式通知節點）
            logging.info(f"任務 {task_id} 已分配到節點 {node['node_id']}")
        except Exception as e:
            logging.error(f"分配任務到節點 {node['node_id']} 時發生錯誤: {e}", exc_info=True)

    def update_node_heartbeat(self, node_id, status, running_tasks, resource_info):
        """?�新節點�?跳�??��??�?�信??"""
        try:
            # 檢查節點是?��???
            if not self.redis_client.exists(f"node:{node_id}"):
                logging.warning(f"節�?{node_id} 不�??��??��??�新心跳")
                return False

            current_time = time.time()
            
            # ?�新節點�??��?心跳?��?
            update_data = {
                "status": status,
                "last_heartbeat": str(current_time),
                "updated_at": str(current_time),
                "running_tasks": str(running_tasks),
                "current_cpu_usage": str(resource_info.get('cpu_usage', 0)),
                "current_memory_usage": str(resource_info.get('memory_usage', 0)),
                "current_gpu_usage": str(resource_info.get('gpu_usage', 0)),
                "current_gpu_memory_usage": str(resource_info.get('gpu_memory_usage', 0)),
                "docker_status": resource_info.get('docker_status', 'unknown')
            }
            
            # ?�新 Redis 中�?節點信??
            self.redis_client.hset(f"node:{node_id}", mapping=update_data)
            
            logging.debug(f"節�?{node_id} ?�?�已?�新: {status}，�?行任?�數: {running_tasks}")
            return True

        except Exception as e:
            logging.error(f"?�新節�?{node_id} 心跳失�?: {e}", exc_info=True)
            return False

    def get_node_info(self, node_id):
        """取得單一節點資訊（dict）- 包含資源追蹤"""
        node_key = f"node:{node_id}"
        try:
            node_info = self.redis_client.hgetall(node_key)
            if not node_info:
                return None
            return {
                "node_id": node_id,
                "hostname": node_info.get("hostname", "127.0.0.1"),
                "port": int(node_info.get("port", 0)),
                "status": node_info.get("status", "Unknown"),
                "last_heartbeat": float(node_info.get("last_heartbeat", 0)),
                "cpu_cores": int(node_info.get("cpu_cores", 0)),
                "memory_gb": float(node_info.get("memory_gb", 0)),
                "cpu_score": int(node_info.get("cpu_score", 0)),
                "gpu_score": int(node_info.get("gpu_score", 0)),
                "gpu_memory_gb": float(node_info.get("gpu_memory_gb", 0)),
                "location": node_info.get("location", "Unknown"),
                "gpu_name": node_info.get("gpu_name", "Unknown"),
                "docker_status": node_info.get("docker_status", "unknown"),
                "cpt_balance": float(node_info.get("cpt_balance", 0)),
                "credit_score": int(node_info.get("credit_score", 100)),
                "trust_level": node_info.get("trust_level", "low"),
                "current_tasks": int(node_info.get("current_tasks", 0)),
                "running_task_ids": node_info.get("running_task_ids", ""),
                # 總資源
                "total_cpu_score": int(node_info.get("total_cpu_score", 0)),
                "total_memory_gb": float(node_info.get("total_memory_gb", 0)),
                "total_gpu_score": int(node_info.get("total_gpu_score", 0)),
                "total_gpu_memory_gb": float(node_info.get("total_gpu_memory_gb", 0)),
                # 可用資源
                "available_cpu_score": max(0, min(int(node_info.get("available_cpu_score", 0)), int(node_info.get("total_cpu_score", 0) or 0))),
                "available_memory_gb": max(0.0, min(float(node_info.get("available_memory_gb", 0)), float(node_info.get("total_memory_gb", 0) or 0.0))),
                "available_gpu_score": max(0, min(int(node_info.get("available_gpu_score", 0)), int(node_info.get("total_gpu_score", 0) or 0))),
                "available_gpu_memory_gb": max(0.0, min(float(node_info.get("available_gpu_memory_gb", 0)), float(node_info.get("total_gpu_memory_gb", 0) or 0.0)))
            }
        except Exception as e:
            logging.error(f"取得節點 {node_id} 資訊失敗: {e}")
            return None

    def get_available_nodes_by_trust_group(self, memory_gb_req, cpu_score_req, gpu_score_req, gpu_memory_gb_req, location_req, gpu_name_req, user_trust_score=0):
        """按信任群組分層查找可用節點"""
        try:
            logging.info(f"按信任群組搜尋節點，用戶信任分數: {user_trust_score}")
            
            all_nodes = self.get_node_list()
            logging.info(f"獲取到 {len(all_nodes)} 個節點進行篩選")
            
            # 按信任等級分組收集節點
            high_trust_nodes = []    # >= 100分且有Docker
            normal_trust_nodes = []  # 50-99分且有Docker  
            low_trust_nodes = []     # < 50分或無Docker
            
            for node_data in all_nodes:
                node_id = node_data['node_id']
                logging.info(f"檢查節點 {node_id}")
                
                node_info = self.get_node_info(node_id)
                if not node_info:
                    logging.warning(f"無法獲取節點 {node_id} 的詳細信息")
                    continue
                
                docker_status = node_info.get("docker_status", "unknown")
                credit_score = int(node_info.get("credit_score", 100))
                trust_level = node_info.get("trust_level", "low")
                current_tasks = int(node_info.get("current_tasks", 0))
                max_concurrent = int(node_info.get("max_concurrent_tasks", 1))
                
                logging.info(f"節點 {node_id}: docker={docker_status}, credit={credit_score}, trust={trust_level}, tasks={current_tasks}")
                
                # 獲取節點的總資源（而不是可用資源，用於基本資源檢查）
                total_cpu = int(node_info.get("total_cpu_score", node_info.get("cpu_score", 0)))
                total_memory = int(node_info.get("total_memory_gb", node_info.get("memory_gb", 0)))
                total_gpu = int(node_info.get("total_gpu_score", node_info.get("gpu_score", 0)))
                total_gpu_memory = int(node_info.get("total_gpu_memory_gb", node_info.get("gpu_memory_gb", 0)))
                
                # 檢查是否有足夠的總資源來執行任務
                has_enough_resources = (
                    total_memory >= memory_gb_req and
                    total_cpu >= cpu_score_req and
                    total_gpu >= gpu_score_req and
                    total_gpu_memory >= gpu_memory_gb_req
                )
                
                logging.info(f"節點 {node_id} 資源檢查: 需求 CPU:{cpu_score_req}, Memory:{memory_gb_req}GB, GPU:{gpu_score_req}, GPU Memory:{gpu_memory_gb_req}GB")
                logging.info(f"節點 {node_id} 總資源: CPU:{total_cpu}, Memory:{total_memory}GB, GPU:{total_gpu}, GPU Memory:{total_gpu_memory}GB")
                logging.info(f"節點 {node_id} 資源足夠: {has_enough_resources}")
                
                # 檢查 GPU 名稱匹配
                node_gpu_name = node_info.get("gpu_name", "")
                is_gpu_name_ok = (not gpu_name_req or gpu_name_req.lower() == "any" or node_gpu_name == gpu_name_req)
                
                logging.info(f"節點 {node_id} GPU名稱檢查: 需求'{gpu_name_req}' vs 節點'{node_gpu_name}' = {is_gpu_name_ok}")
                
                # Docker狀態篩選：無Docker節點只接收高信任用戶(>50)的任務
                docker_compatible = True
                if docker_status == "disabled":
                    if user_trust_score <= 50:
                        docker_compatible = False
                        logging.info(f"節點 {node_id} 無Docker，用戶信任分數 {user_trust_score} <= 50，跳過")
                    else:
                        logging.info(f"節點 {node_id} 無Docker，但用戶信任分數 {user_trust_score} > 50，允許使用")
                
                # 檢查節點狀態是否為Idle
                node_status = node_info.get("status", "Unknown")
                is_idle = (node_status == "Idle")
                
                logging.info(f"節點 {node_id} 狀態檢查: status='{node_status}', is_idle={is_idle}")
                
                if has_enough_resources and is_gpu_name_ok and docker_compatible and is_idle:
                    # 計算負載因子 (任務數/總資源的比例)
                    load_factor = current_tasks / max_concurrent * 100
                    
                    # 創建包含節點對象的數據結構
                    class SimpleNode:
                        def __init__(self, data):
                            self.node_id = data['node_id']
                            self.hostname = data['hostname']
                            self.cpu_cores = data['cpu_cores']
                            self.memory_gb = data['memory_gb']
                            self.status = data['status']
                            self.last_heartbeat = data['last_heartbeat']
                            self.cpu_score = data['cpu_score']
                            self.gpu_score = data['gpu_score']
                            self.gpu_memory_gb = data['gpu_memory_gb']
                            self.location = data['location']
                            self.port = data['port']
                            self.gpu_name = data['gpu_name']
                    
                    simple_node = SimpleNode(node_data)
                    
                    node_group_data = {
                        'node': simple_node,
                        'credit_score': credit_score,
                        'docker_status': docker_status,
                        'trust_level': trust_level,
                        'current_tasks': current_tasks,
                        'load_factor': load_factor,
                        'available_resources': {
                            'cpu': total_cpu,  # 使用總資源而不是可用資源進行初步篩選
                            'memory': total_memory,
                            'gpu': total_gpu,
                            'gpu_memory': total_gpu_memory
                        }
                    }
                    
                    # 按信任等級分組
                    if trust_level == 'high':
                        high_trust_nodes.append(node_group_data)
                        logging.info(f"節點 {node_id} 加入高信任群組")
                    elif trust_level == 'normal':
                        normal_trust_nodes.append(node_group_data)
                        logging.info(f"節點 {node_id} 加入中信任群組")
                    else:
                        low_trust_nodes.append(node_group_data)
                        logging.info(f"節點 {node_id} 加入低信任群組")
                else:
                    reason = []
                    if not has_enough_resources:
                        reason.append("資源不足")
                    if not is_gpu_name_ok:
                        reason.append("GPU名稱不匹配")
                    if not docker_compatible:
                        reason.append("Docker不相容")
                    if not is_idle:
                        reason.append("節點非閒置狀態")
                    logging.info(f"節點 {node_id} 不符合條件: {', '.join(reason)}")

            # 對每個群組按負載排序 (負載低的優先)
            high_trust_nodes.sort(key=lambda x: (x['load_factor'], x['current_tasks']))
            normal_trust_nodes.sort(key=lambda x: (x['load_factor'], x['current_tasks']))
            low_trust_nodes.sort(key=lambda x: (x['load_factor'], x['current_tasks']))
            
            # 記錄各群組資源統計
            def calculate_group_resources(nodes):
                total_cpu = sum(n['available_resources']['cpu'] for n in nodes)
                total_memory = sum(n['available_resources']['memory'] for n in nodes)
                total_gpu = sum(n['available_resources']['gpu'] for n in nodes)
                total_gpu_memory = sum(n['available_resources']['gpu_memory'] for n in nodes)
                return total_cpu, total_memory, total_gpu, total_gpu_memory
            
            if high_trust_nodes:
                cpu, mem, gpu, gpu_mem = calculate_group_resources(high_trust_nodes)
                logging.info(f"高信任群組: {len(high_trust_nodes)} 節點, 可用資源 CPU:{cpu}, Memory:{mem}GB, GPU:{gpu}, GPU Memory:{gpu_mem}GB")
            
            if normal_trust_nodes:
                cpu, mem, gpu, gpu_mem = calculate_group_resources(normal_trust_nodes)
                logging.info(f"中信任群組: {len(normal_trust_nodes)} 節點, 可用資源 CPU:{cpu}, Memory:{mem}GB, GPU:{gpu}, GPU Memory:{gpu_mem}GB")
                
            if low_trust_nodes:
                cpu, mem, gpu, gpu_mem = calculate_group_resources(low_trust_nodes)
                logging.info(f"低信任群組: {len(low_trust_nodes)} 節點, 可用資源 CPU:{cpu}, Memory:{mem}GB, GPU:{gpu}, GPU Memory:{gpu_mem}GB")

            # 按優先級順序返回節點群組
            result = {
                'high_trust': [n['node'] for n in high_trust_nodes],
                'normal_trust': [n['node'] for n in normal_trust_nodes], 
                'low_trust': [n['node'] for n in low_trust_nodes]
            }
            
            total_available = len(result['high_trust']) + len(result['normal_trust']) + len(result['low_trust'])
            logging.info(f"節點分組完成: 高信任:{len(result['high_trust'])}, 中信任:{len(result['normal_trust'])}, 低信任:{len(result['low_trust'])}, 總計:{total_available}")
            
            return result
            
        except Exception as e:
            logging.error(f"按信任群組查找節點時發生錯誤: {e}", exc_info=True)
            return {'high_trust': [], 'normal_trust': [], 'low_trust': []}

    def allocate_node_resources(self, node_id, task_id, cpu_score, memory_gb, gpu_score, gpu_memory_gb):
        """分配節點資源給任務"""
        try:
            node_key = f"node:{node_id}"
            
            # 獲取當前可用資源
            node_info = self.redis_client.hgetall(node_key)
            if not node_info:
                return False, "節點不存在"
            
            available_cpu = int(float(node_info.get("available_cpu_score", 0) or 0))
            available_memory = float(node_info.get("available_memory_gb", 0) or 0)
            available_gpu = int(float(node_info.get("available_gpu_score", 0) or 0))
            available_gpu_memory = float(node_info.get("available_gpu_memory_gb", 0) or 0)
            
            # 檢查資源是否足夠（需求量不可超過可用量）
            required_cpu = int(cpu_score)
            required_memory = float(memory_gb)
            required_gpu = int(gpu_score)
            required_gpu_memory = float(gpu_memory_gb)
            
            if (required_cpu > available_cpu or required_memory > available_memory or 
                required_gpu > available_gpu or required_gpu_memory > available_gpu_memory):
                logging.warning(
                    f"資源不足 - 需求: CPU={required_cpu}, Mem={required_memory}GB, GPU={required_gpu}, GPU_Mem={required_gpu_memory}GB | "
                    f"可用: CPU={available_cpu}, Mem={available_memory}GB, GPU={available_gpu}, GPU_Mem={available_gpu_memory}GB"
                )
                return False, f"節點資源不足: 需要 CPU:{cpu_score}, Memory:{memory_gb}GB, GPU:{gpu_score}, GPU Memory:{gpu_memory_gb}GB (可用: CPU:{available_cpu}, Memory:{available_memory}GB, GPU:{available_gpu}, GPU Memory:{available_gpu_memory}GB)"
            
            # 扣除資源
            new_cpu = available_cpu - int(cpu_score)
            new_memory = available_memory - float(memory_gb)
            new_gpu = available_gpu - int(gpu_score)
            new_gpu_memory = available_gpu_memory - float(gpu_memory_gb)
            
            # 更新任務列表
            current_tasks = int(node_info.get("current_tasks", 0))
            running_tasks = node_info.get("running_task_ids", "")
            
            if running_tasks:
                new_task_list = f"{running_tasks},{task_id}"
            else:
                new_task_list = task_id
            
            # 批量更新Redis
            updates = {
                "available_cpu_score": str(new_cpu),
                "available_memory_gb": str(new_memory),
                "available_gpu_score": str(new_gpu),
                "available_gpu_memory_gb": str(new_gpu_memory),
                "current_tasks": str(current_tasks + 1),
                "running_task_ids": new_task_list
            }
            
            for field, value in updates.items():
                self.redis_client.hset(node_key, field, value)
            
            logging.info(f"節點 {node_id} 資源已分配給任務 {task_id}: CPU-{cpu_score}, Memory-{memory_gb}GB, GPU-{gpu_score}, GPU Memory-{gpu_memory_gb}GB")
            logging.info(f"節點 {node_id} 剩餘資源: CPU:{new_cpu}, Memory:{new_memory}GB, GPU:{new_gpu}, GPU Memory:{new_gpu_memory}GB")
            
            return True, "資源分配成功"
            
        except Exception as e:
            logging.error(f"分配節點 {node_id} 資源失敗: {e}")
            return False, f"資源分配錯誤: {e}"

    def release_node_resources(self, node_id, task_id, cpu_score, memory_gb, gpu_score, gpu_memory_gb):
        """釋放節點資源"""
        try:
            node_key = f"node:{node_id}"
            
            node_info = self.redis_client.hgetall(node_key)
            if not node_info:
                return False, "節點不存在"
            
            # 歸還資源
            available_cpu = int(float(node_info.get("available_cpu_score", 0) or 0))
            available_memory = float(node_info.get("available_memory_gb", 0) or 0)
            available_gpu = int(float(node_info.get("available_gpu_score", 0) or 0))
            available_gpu_memory = float(node_info.get("available_gpu_memory_gb", 0) or 0)
            
            # 不能超過總資源
            total_cpu = int(float(node_info.get("total_cpu_score", 0) or 0))
            total_memory = float(node_info.get("total_memory_gb", 0) or 0)
            total_gpu = int(float(node_info.get("total_gpu_score", 0) or 0))
            total_gpu_memory = float(node_info.get("total_gpu_memory_gb", 0) or 0)
            
            new_cpu = min(available_cpu + int(cpu_score), total_cpu)
            new_memory = min(available_memory + float(memory_gb), total_memory)
            new_gpu = min(available_gpu + int(gpu_score), total_gpu)
            new_gpu_memory = min(available_gpu_memory + float(gpu_memory_gb), total_gpu_memory)
            
            # 更新任務列表
            current_tasks = max(0, int(node_info.get("current_tasks", 0)) - 1)
            running_tasks = node_info.get("running_task_ids", "")
            
            # 從任務列表中移除該任務
            task_list = [t.strip() for t in running_tasks.split(",") if t.strip() and t.strip() != task_id]
            new_task_list = ",".join(task_list)
            
            # 批量更新Redis
            updates = {
                "available_cpu_score": str(new_cpu),
                "available_memory_gb": str(new_memory),
                "available_gpu_score": str(new_gpu),
                "available_gpu_memory_gb": str(new_gpu_memory),
                "current_tasks": str(current_tasks),
                "running_task_ids": new_task_list
            }
            
            # 如果沒有任務了，設為Idle
            if current_tasks == 0:
                updates["status"] = "Idle"
            
            for field, value in updates.items():
                self.redis_client.hset(node_key, field, value)
            
            logging.info(f"節點 {node_id} 任務 {task_id} 資源已釋放: CPU+{cpu_score}, Memory+{memory_gb}GB, GPU+{gpu_score}, GPU Memory+{gpu_memory_gb}GB")
            logging.info(f"節點 {node_id} 當前資源: CPU:{new_cpu}, Memory:{new_memory}GB, GPU:{new_gpu}, GPU Memory:{new_gpu_memory}GB, 任務數:{current_tasks}")
            
            return True, "資源釋放成功"
            
        except Exception as e:
            logging.error(f"釋放節點 {node_id} 資源失敗: {e}")
            return False, f"資源釋放錯誤: {e}"

    def get_node_info(self, node_id):
        """取得單一節點資訊（dict）- 包含資源追蹤"""
        node_key = f"node:{node_id}"
        try:
            node_info = self.redis_client.hgetall(node_key)
            if not node_info:
                return None
            return {
                "node_id": node_id,
                "hostname": node_info.get("hostname", "127.0.0.1"),
                "port": int(node_info.get("port", 0)),
                "status": node_info.get("status", "Unknown"),
                "last_heartbeat": float(node_info.get("last_heartbeat", 0)),
                "cpu_cores": int(node_info.get("cpu_cores", 0)),
                "memory_gb": float(node_info.get("memory_gb", 0)),
                "cpu_score": int(node_info.get("cpu_score", 0)),
                "gpu_score": int(node_info.get("gpu_score", 0)),
                "gpu_memory_gb": float(node_info.get("gpu_memory_gb", 0)),
                "location": node_info.get("location", "Unknown"),
                "gpu_name": node_info.get("gpu_name", "Unknown"),
                "docker_status": node_info.get("docker_status", "unknown"),
                "cpt_balance": float(node_info.get("cpt_balance", 0)),
                "credit_score": int(node_info.get("credit_score", 100)),
                "trust_level": node_info.get("trust_level", "low"),
                "current_tasks": int(node_info.get("current_tasks", 0)),
                "running_task_ids": node_info.get("running_task_ids", ""),
                # 總資源
                "total_cpu_score": int(node_info.get("total_cpu_score", 0)),
                "total_memory_gb": float(node_info.get("total_memory_gb", 0)),
                "total_gpu_score": int(node_info.get("total_gpu_score", 0)),
                "total_gpu_memory_gb": float(node_info.get("total_gpu_memory_gb", 0)),
                # 可用資源
                "available_cpu_score": int(node_info.get("available_cpu_score", 0)),
                "available_memory_gb": float(node_info.get("available_memory_gb", 0)),
                "available_gpu_score": int(node_info.get("available_gpu_score", 0)),
                "available_gpu_memory_gb": float(node_info.get("available_gpu_memory_gb", 0))
            }
        except Exception as e:
            logging.error(f"取得節點 {node_id} 資訊失敗: {e}")
            return None

    def report_running_status(self, request, context=None):
        """處理運行狀態報告 - 簡化版本，移除動態獎勵計算"""
        try:
            node_id = request.node_id
            task_id = request.task_id
            cpu_usage = request.cpu_usage
            memory_usage = request.memory_usage
            gpu_usage = request.gpu_usage
            gpu_memory_usage = request.gpu_memory_usage
            
            # 檢查節點是否存在
            if not self.redis_client.exists(f"node:{node_id}"):
                error_msg = f"節點 {node_id} 不存在，請先註冊"
                logging.warning(error_msg)
                if context:
                    return nodepool_pb2.RunningStatusResponse(success=False, message=error_msg, cpt_reward=0)
                else:
                    return False, error_msg
            
            current_time = time.time()
            
            if not task_id or task_id == "":
                # 空task_id表示節點整體狀態報告
                logging.debug(f"節點 {node_id} 系統負載: CPU={cpu_usage}%, Memory={memory_usage}%")
                
                # 更新節點的系統負載信息
                update_data = {
                    "current_cpu_usage": str(cpu_usage),
                    "current_memory_usage": str(memory_usage),
                    "current_gpu_usage": str(gpu_usage),
                    "current_gpu_memory_usage": str(gpu_memory_usage),
                    "last_load_report": str(current_time),
                    "last_heartbeat": str(current_time)
                }
                
                # 根據負載調整節點狀態
                max_load = max(cpu_usage, memory_usage)
                if max_load > 90:
                    load_status = "Heavy Load"
                elif max_load > 70:
                    load_status = "Medium Load"
                elif max_load > 30:
                    load_status = "Light Load"
                else:
                    load_status = "Idle"
                
                # 獲取當前任務數量來決定最終狀態
                current_tasks = int(self.redis_client.hget(f"node:{node_id}", "current_tasks") or 0)
                if current_tasks > 0:
                    final_status = f"Running {current_tasks} tasks - {load_status}"
                else:
                    final_status = load_status
                
                update_data["load_status"] = load_status
                update_data["status"] = final_status
                
                # 批量更新Redis
                self.redis_client.hset(f"node:{node_id}", mapping=update_data)
                
                logging.debug(f"節點 {node_id} 負載狀態已更新: {final_status}")
                
                if context:
                    return nodepool_pb2.RunningStatusResponse(
                        success=True, 
                        message=f"Node {node_id} load status updated: {load_status}", 
                        cpt_reward=0  # 不再動態計算獎勵
                    )
                else:
                    return True, f"節點負載狀態已更新: {load_status}"
            else:
                # 具體task_id表示任務狀態報告
                logging.debug(f"任務 {task_id} 在節點 {node_id} 的資源使用: CPU={cpu_usage}%, Memory={memory_usage}%")
                
                # 更新任務的實際資源使用情況
                task_status_key = f"task_status:{task_id}"
                task_update_data = {
                    "node_id": node_id,
                    "actual_cpu_usage": str(cpu_usage),
                    "actual_memory_usage": str(memory_usage),
                    "actual_gpu_usage": str(gpu_usage),
                    "actual_gpu_memory_usage": str(gpu_memory_usage),
                    "last_resource_report": str(current_time)
                }
                
                self.redis_client.hset(task_status_key, mapping=task_update_data)
                self.redis_client.expire(task_status_key, 3600)  # 1小時後過期
                
                logging.debug(f"任務 {task_id} 資源使用狀態已更新")
                
                if context:
                    return nodepool_pb2.RunningStatusResponse(
                        success=True, 
                        message=f"Task {task_id} resource usage updated", 
                        cpt_reward=0  # 獎勵改由任務調度器按固定費率計算
                    )
                else:
                    return True, f"任務資源使用狀態已更新"
        
        except Exception as e:
            error_msg = f"處理運行狀態報告失敗: {e}"
            logging.error(error_msg, exc_info=True)
            if context:
                return nodepool_pb2.RunningStatusResponse(success=False, message=error_msg, cpt_reward=0)
            else:
                return False, error_msg

    def get_node_load_info(self, node_id):
        """獲取節點負載信息"""
        try:
            node_key = f"node:{node_id}"
            load_fields = [
                "current_cpu_usage", "current_memory_usage", 
                "current_gpu_usage", "current_gpu_memory_usage",
                "load_status", "last_load_report"
            ]
            
            load_values = self.redis_client.hmget(node_key, load_fields)
            
            return {
                "current_cpu_usage": int(load_values[0] or 0),
                "current_memory_usage": int(load_values[1] or 0),
                "current_gpu_usage": int(load_values[2] or 0),
                "current_gpu_memory_usage": int(load_values[3] or 0),
                "load_status": load_values[4] or "Unknown",
                "last_load_report": float(load_values[5] or 0)
            }
        except Exception as e:
            logging.error(f"獲取節點 {node_id} 負載信息失敗: {e}")
            return {
                "current_cpu_usage": 0,
                "current_memory_usage": 0,
                "current_gpu_usage": 0,
                "current_gpu_memory_usage": 0,
                "load_status": "Unknown",
                "last_load_report": 0
            }

    def get_task_resource_usage(self, task_id):
        """獲取任務的實際資源使用情況"""
        try:
            task_status_key = f"task_status:{task_id}"
            if not self.redis_client.exists(task_status_key):
                return None
            
            usage_data = self.redis_client.hgetall(task_status_key)
            return {
                "node_id": usage_data.get("node_id", ""),
                "actual_cpu_usage": int(usage_data.get("actual_cpu_usage", 0)),
                "actual_memory_usage": int(usage_data.get("actual_memory_usage", 0)),
                "actual_gpu_usage": int(usage_data.get("actual_gpu_usage", 0)),
                "actual_gpu_memory_usage": int(usage_data.get("actual_gpu_memory_usage", 0)),
                "last_resource_report": float(usage_data.get("last_resource_report", 0))
            }
        except Exception as e:
            logging.error(f"獲取任務 {task_id} 資源使用失敗: {e}")
            return None
            

    def get_task_resource_usage(self, task_id):
        """獲取任務的實際資源使用情況"""
        try:
            task_status_key = f"task_status:{task_id}"
            if not self.redis_client.exists(task_status_key):
                return None
            
            usage_data = self.redis_client.hgetall(task_status_key)
            return {
                "node_id": usage_data.get("node_id", ""),
                "actual_cpu_usage": int(usage_data.get("actual_cpu_usage", 0)),
                "actual_memory_usage": int(usage_data.get("actual_memory_usage", 0)),
                "actual_gpu_usage": int(usage_data.get("actual_gpu_usage", 0)),
                "actual_gpu_memory_usage": int(usage_data.get("actual_gpu_memory_usage", 0)),
                "last_resource_report": float(usage_data.get("last_resource_report", 0))
            }
        except Exception as e:
            logging.error(f"獲取任務 {task_id} 資源使用失敗: {e}")
            return None
    def update_node_heartbeat(self, node_id, status, running_tasks, resource_info):
        """?�新節點�?跳�??��??�?�信??"""
        try:
            # 檢查節點是?��???
            if not self.redis_client.exists(f"node:{node_id}"):
                logging.warning(f"節�?{node_id} 不�??��??��??�新心跳")
                return False

            current_time = time.time()
            
            # ?�新節點�??��?心跳?��?
            update_data = {
                "status": status,
                "last_heartbeat": str(current_time),
                "updated_at": str(current_time),
                "running_tasks": str(running_tasks),
                "current_cpu_usage": str(resource_info.get('cpu_usage', 0)),
                "current_memory_usage": str(resource_info.get('memory_usage', 0)),
                "current_gpu_usage": str(resource_info.get('gpu_usage', 0)),
                "current_gpu_memory_usage": str(resource_info.get('gpu_memory_usage', 0)),
                "docker_status": resource_info.get('docker_status', 'unknown')
            }
            
            # ?�新 Redis 中�?節點信??
            self.redis_client.hset(f"node:{node_id}", mapping=update_data)
            
            logging.debug(f"節�?{node_id} ?�?�已?�新: {status}，�?行任?�數: {running_tasks}")
            return True

        except Exception as e:
            logging.error(f"?�新節�?{node_id} 心跳失�?: {e}", exc_info=True)
            return False

    def stop_task(self, node_id, task_id):
        """停止指定節點上的任務"""
        try:
            node_key = f"node:{node_id}"

            # 確認節點是否存在
            if not self.redis_client.exists(node_key):
                logging.warning(f"節點 {node_id} 不存在，無法停止任務 {task_id}")
                return False, f"節點 {node_id} 不存在"

            # 獲取當前運行的任務列表
            current_tasks = self.redis_client.hget(node_key, "running_task_ids") or ""
            task_list = current_tasks.split(",") if current_tasks else []

            if task_id not in task_list:
                logging.warning(f"任務 {task_id} 不在節點 {node_id} 的運行列表中")
                return False, f"任務 {task_id} 不存在於節點 {node_id}"

            # 從任務列表中移除指定任務
            task_list.remove(task_id)
            self.redis_client.hset(node_key, "running_task_ids", ",".join(task_list))

            # 更新當前任務數量
            self.redis_client.hincrby(node_key, "current_tasks", -1)

            logging.info(f"任務 {task_id} 已成功從節點 {node_id} 停止")
            return True, f"任務 {task_id} 已停止"

        except Exception as e:
            logging.error(f"停止節點 {node_id} 上的任務 {task_id} 時發生錯誤: {e}", exc_info=True)
            return False, str(e)
