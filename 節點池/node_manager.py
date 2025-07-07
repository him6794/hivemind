# node_pool/node_manager.py
import grpc
import logging
import redis
import time
import nodepool_pb2

logging.basicConfig(level=logging.INFO, format='%(asctime)s - %(levelname)s - %(message)s')

class NodeManager:
    def __init__(self):
        self.redis_client = redis.Redis(host='localhost', port=6379, db=0, decode_responses=True)
        try:
            self.redis_client.ping()
            logging.info("Redis 連線成功")
        except redis.RedisError as e:
            logging.error(f"Redis 連線失敗: {e}")
            raise # 啟動時連不上 Redis 就拋出異常

    def register_worker_node(self, node_id, hostname, cpu_cores, memory_gb, cpu_score, gpu_score, gpu_memory_gb, location, port, gpu_name):
        """註冊或更新工作節點資訊"""
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
            status_message = request.status_message
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
                return nodepool_pb2.StatusResponse(success=True, message="狀態報告成功")
            else:
                return True, "狀態更新成功"

        except Exception as e:
            error_msg = f"更新節點 {node_id} 狀態失敗: {e}"
            logging.error(error_msg, exc_info=True)
            if is_grpc_call:
                return nodepool_pb2.StatusResponse(success=False, message=error_msg)
            else:
                return False, error_msg

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
                        # 移除 ip 字段
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
        """根據需求查找可用的空閒節點 - 優化分配策略"""
        try:
            available_nodes_info = []
            all_nodes = self.get_node_list() # 直接復用 get_node_list 獲取已處理好的節點列表

            # 第一階段：基本篩選 - 找出所有滿足最低需求的節點
            qualifying_nodes = []
            for node in all_nodes:
                # 檢查基本狀態和資源需求
                is_status_ok = (node.status == "Idle")
                is_resource_ok = (
                    node.memory_gb >= memory_gb_req and
                    node.cpu_score >= cpu_score_req and
                    node.gpu_score >= gpu_score_req and
                    node.gpu_memory_gb >= gpu_memory_gb_req
                )
                # 檢查 GPU 名稱匹配
                is_gpu_name_ok = (not gpu_name_req or gpu_name_req.lower() == "any" or node.gpu_name == gpu_name_req)

                if is_status_ok and is_resource_ok and is_gpu_name_ok:
                    # 計算總效能分數差距
                    total_performance = node.cpu_score + node.gpu_score
                    required_performance = cpu_score_req + gpu_score_req
                    
                    # 計算效能差距百分比
                    if required_performance > 0:
                        performance_gap = ((total_performance - required_performance) / required_performance) * 100
                    else:
                        performance_gap = 0
                    
                    qualifying_nodes.append({
                        'node': node,
                        'performance_gap': performance_gap,
                        'total_performance': total_performance
                    })

            if not qualifying_nodes:
                logging.info("沒有找到滿足基本需求的節點")
                return []

            logging.info(f"找到 {len(qualifying_nodes)} 個滿足基本需求的節點")

            # 第二階段：性能匹配篩選 - 找出效能差距在5%以內的節點
            performance_matched_nodes = []
            for node_info in qualifying_nodes:
                if node_info['performance_gap'] <= 5:  # 效能差距不超過5%
                    performance_matched_nodes.append(node_info)

            if performance_matched_nodes:
                logging.info(f"找到 {len(performance_matched_nodes)} 個效能匹配的節點（差距≤5%）")
                candidate_nodes = performance_matched_nodes
            else:
                logging.info("沒有找到效能差距≤5%的節點，使用所有滿足最低需求的節點")
                candidate_nodes = qualifying_nodes

            # 第三階段：地區篩選
            if location_req and location_req != "Any":
                # 優先選擇指定地區的節點
                location_matched_nodes = []
                for node_info in candidate_nodes:
                    if node_info['node'].location == location_req:
                        location_matched_nodes.append(node_info)
                
                if location_matched_nodes:
                    logging.info(f"找到 {len(location_matched_nodes)} 個地區匹配的節點（地區: {location_req}）")
                    final_candidates = location_matched_nodes
                else:
                    logging.info(f"指定地區 {location_req} 沒有可用節點，使用其他地區的節點")
                    final_candidates = candidate_nodes
            else:
                logging.info("沒有指定地區偏好，使用所有候選節點")
                final_candidates = candidate_nodes

            # 第四階段：最終排序和選擇
            # 按效能差距排序（差距越小越好），然後按總效能排序（效能越高越好）
            final_candidates.sort(key=lambda x: (x['performance_gap'], -x['total_performance']))

            # 提取節點對象
            for node_info in final_candidates:
                available_nodes_info.append(node_info['node'])

            # 記錄詳細的選擇結果
            if available_nodes_info:
                best_node = final_candidates[0]
                logging.info(f"最終選擇結果：共 {len(available_nodes_info)} 個可用節點")
                logging.info(f"最佳節點：{best_node['node'].node_id}，效能差距：{best_node['performance_gap']:.1f}%，總效能：{best_node['total_performance']}")
                
                # 記錄前幾個節點的詳細信息
                for i, node_info in enumerate(final_candidates[:3]):
                    node = node_info['node']
                    logging.info(f"候選節點 {i+1}: {node.node_id} - CPU:{node.cpu_score}, GPU:{node.gpu_score}, "
                               f"地區:{node.location}, 效能差距:{node_info['performance_gap']:.1f}%")
            else:
                logging.warning("沒有找到任何可用節點")

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
                "hostname": node_info.get("hostname", "127.0.0.1"),
                "port": int(node_info.get("port", 0)),
                "status": node_info.get("status", "Unknown"),
                "last_heartbeat": float(node_info.get("last_heartbeat", 0)),
                "cpu_cores": int(node_info.get("cpu_cores", 0)),
                "memory_gb": int(node_info.get("memory_gb", 0)),
                "cpu_score": int(node_info.get("cpu_score", 0)),
                "gpu_score": int(node_info.get("gpu_score", 0)),
                "gpu_memory_gb": int(node_info.get("gpu_memory_gb", 0)),
                "location": node_info.get("location", "Unknown"),
                "gpu_name": node_info.get("gpu_name", "Unknown"),
                "cpt_balance": float(node_info.get("cpt_balance", 0))
            }
        except Exception as e:
            logging.error(f"取得節點 {node_id} 資訊失敗: {e}")
            return None
