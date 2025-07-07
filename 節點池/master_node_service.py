# node_pool/master_node_service.py
import grpc
import logging
import redis
import time
import threading
import json
import jwt
import os
import shutil
import nodepool_pb2
import nodepool_pb2_grpc

# 從正確的位置導入 NodeManager
from node_manager import NodeManager
from user_manager import UserManager
from config import Config

logging.basicConfig(level=logging.INFO, format='%(asctime)s - %(levelname)s - [%(threadName)s] - %(message)s')

# 新增文件存儲管理器
class FileStorageManager:
    def __init__(self, base_path=None):
        # 修正：預設為 Linux 路徑，可用環境變數或 config 覆蓋
        if base_path is None:
            base_path = os.environ.get("TASK_STORAGE_PATH") \
                or getattr(Config, "TASK_STORAGE_PATH", None) \
                or "/mnt/myusb/hivemind/task_storage"
        self.base_path = base_path
        self.ensure_directory()
    
    def ensure_directory(self):
        """確保存儲目錄存在"""
        try:
            os.makedirs(self.base_path, exist_ok=True)
            logging.info(f"任務存儲目錄已準備: {self.base_path}")
        except Exception as e:
            logging.error(f"創建存儲目錄失敗: {e}")
            raise
    
    def get_task_zip_path(self, task_id):
        """獲取任務ZIP文件路徑"""
        return os.path.join(self.base_path, f"task_{task_id}.zip")
    
    def get_result_zip_path(self, task_id):
        """獲取結果ZIP文件路徑"""
        return os.path.join(self.base_path, f"result_{task_id}.zip")
    
    def store_task_zip(self, task_id, task_zip_data):
        """存儲任務ZIP到硬碟"""
        try:
            file_path = self.get_task_zip_path(task_id)
            with open(file_path, 'wb') as f:
                f.write(task_zip_data)
            logging.debug(f"任務 {task_id} ZIP已存儲到: {file_path} ({len(task_zip_data)} bytes)")
            return True
        except Exception as e:
            logging.error(f"存儲任務 {task_id} ZIP失敗: {e}")
            return False
    
    def store_result_zip(self, task_id, result_zip_data):
        """存儲結果ZIP到硬碟"""
        try:
            file_path = self.get_result_zip_path(task_id)
            with open(file_path, 'wb') as f:
                f.write(result_zip_data)
            logging.debug(f"任務 {task_id} 結果ZIP已存儲到: {file_path} ({len(result_zip_data)} bytes)")
            return True
        except Exception as e:
            logging.error(f"存儲任務 {task_id} 結果ZIP失敗: {e}")
            return False
    
    def get_task_zip(self, task_id):
        """從硬碟讀取任務ZIP"""
        try:
            file_path = self.get_task_zip_path(task_id)
            if os.path.exists(file_path):
                with open(file_path, 'rb') as f:
                    data = f.read()
                logging.debug(f"已讀取任務 {task_id} ZIP: {len(data)} bytes")
                return data
            else:
                logging.warning(f"任務 {task_id} ZIP文件不存在: {file_path}")
                return b""
        except Exception as e:
            logging.error(f"讀取任務 {task_id} ZIP失敗: {e}")
            return b""
    
    def get_result_zip(self, task_id):
        """從硬碟讀取結果ZIP"""
        try:
            file_path = self.get_result_zip_path(task_id)
            if os.path.exists(file_path):
                with open(file_path, 'rb') as f:
                    data = f.read()
                logging.debug(f"已讀取任務 {task_id} 結果ZIP: {len(data)} bytes")
                return data
            else:
                logging.debug(f"任務 {task_id} 結果ZIP文件不存在: {file_path}")
                return b""
        except Exception as e:
            logging.error(f"讀取任務 {task_id} 結果ZIP失敗: {e}")
            return b""
    
    def task_zip_exists(self, task_id):
        """檢查任務ZIP是否存在"""
        return os.path.exists(self.get_task_zip_path(task_id))
    
    def result_zip_exists(self, task_id):
        """檢查結果ZIP是否存在"""
        return os.path.exists(self.get_result_zip_path(task_id))
    
    def cleanup_task_files(self, task_id):
        """清理任務相關文件"""
        try:
            task_file = self.get_task_zip_path(task_id)
            result_file = self.get_result_zip_path(task_id)
            
            files_removed = 0
            if os.path.exists(task_file):
                os.remove(task_file)
                files_removed += 1
                logging.debug(f"已刪除任務文件: {task_file}")
            
            if os.path.exists(result_file):
                os.remove(result_file)
                files_removed += 1
                logging.debug(f"已刪除結果文件: {result_file}")
            
            if files_removed > 0:
                logging.info(f"任務 {task_id} 已清理 {files_removed} 個文件")
            return True
        except Exception as e:
            logging.error(f"清理任務 {task_id} 文件失敗: {e}")
            return False
    
    def mark_for_cleanup(self, task_id):
        """標記任務文件可以清理（在結果成功傳輸後）"""
        try:
            # 創建一個標記文件，表示這個任務的文件可以被清理
            cleanup_marker_path = os.path.join(self.base_path, f"cleanup_{task_id}.marker")
            with open(cleanup_marker_path, 'w') as f:
                f.write(str(time.time()))  # 記錄標記時間
            logging.debug(f"任務 {task_id} 已標記為可清理")
            return True
        except Exception as e:
            logging.error(f"標記任務 {task_id} 清理失敗: {e}")
            return False
    
    def is_marked_for_cleanup(self, task_id):
        """檢查任務是否已標記為可清理"""
        cleanup_marker_path = os.path.join(self.base_path, f"cleanup_{task_id}.marker")
        return os.path.exists(cleanup_marker_path)
    
    def cleanup_task_files_if_marked(self, task_id):
        """如果任務已標記為可清理，則清理文件"""
        if not self.is_marked_for_cleanup(task_id):
            return False
            
        try:
            task_file = self.get_task_zip_path(task_id)
            result_file = self.get_result_zip_path(task_id)
            cleanup_marker = os.path.join(self.base_path, f"cleanup_{task_id}.marker")
            
            files_removed = 0
            
            # 刪除任務文件
            if os.path.exists(task_file):
                os.remove(task_file)
                files_removed += 1
                logging.info(f"已刪除任務文件: {task_file}")
            
            # 刪除結果文件
            if os.path.exists(result_file):
                os.remove(result_file)
                files_removed += 1
                logging.info(f"已刪除結果文件: {result_file}")
            
            # 刪除清理標記文件
            if os.path.exists(cleanup_marker):
                os.remove(cleanup_marker)
                logging.debug(f"已刪除清理標記: {cleanup_marker}")
            
            if files_removed > 0:
                logging.info(f"任務 {task_id} 已清理 {files_removed} 個文件（結果已成功傳輸）")
            
            return True
        except Exception as e:
            logging.error(f"清理已標記任務 {task_id} 文件失敗: {e}")
            return False
    
    def delayed_cleanup_task_files(self, task_id, delay_seconds=5):
        """延遲清理任務文件，確保文件傳輸完成"""
        def cleanup_after_delay():
            try:
                time.sleep(delay_seconds)
                self.cleanup_task_files_if_marked(task_id)
            except Exception as e:
                logging.error(f"延遲清理任務 {task_id} 失敗: {e}")
        
        # 在後台線程中執行延遲清理
        cleanup_thread = threading.Thread(target=cleanup_after_delay, daemon=True)
        cleanup_thread.start()
        logging.debug(f"任務 {task_id} 已安排 {delay_seconds} 秒後清理")

# --- TaskManager 類修改版本 ---
class TaskManager:
    def __init__(self):
        # decode_responses=True for easier handling
        self.redis_client = redis.Redis(host='localhost', port=6379, db=0, decode_responses=True)
        self.file_storage = FileStorageManager()  # 新增文件存儲管理器
        try:
            self.redis_client.ping()
            logging.info("TaskManager: Redis 連線成功")
        except redis.RedisError as e:
            logging.error(f"TaskManager: Redis 連線失敗: {e}")
            raise

    def store_task(self, task_id, task_zip, memory_gb, cpu_score, gpu_score, gpu_memory_gb, location, gpu_name, user_id, cpt_cost=None):
        """存儲任務信息，ZIP文件存到硬碟，其他信息存到Redis"""
        task_key = f"task:{task_id}"
        try:
            # 記錄詳細的輸入參數
            logging.info(f"存儲任務 {task_id} 的詳細信息: memory_gb={memory_gb}, cpu_score={cpu_score}, "
                         f"gpu_score={gpu_score}, gpu_memory_gb={gpu_memory_gb}, user_id={user_id}")
            
            # 計算 CPT 成本：cpu_score + gpu_score + memory_gb + gpu_memory_gb
            calculated_cpt_cost = int(cpu_score) + int(gpu_score) + int(memory_gb) + int(gpu_memory_gb)
            # 確保至少為 1
            calculated_cpt_cost = max(1, calculated_cpt_cost)
            
            logging.info(f"任務 {task_id} 計算的 CPT 成本: {calculated_cpt_cost} (CPU:{cpu_score} + GPU:{gpu_score} + MEM:{memory_gb} + GPU_MEM:{gpu_memory_gb})")
            
            # 存儲ZIP文件到硬碟
            if task_zip:
                if not self.file_storage.store_task_zip(task_id, task_zip):
                    logging.error(f"任務 {task_id} ZIP文件存儲失敗")
                    return False
                logging.info(f"任務 {task_id} ZIP文件已存儲到硬碟 ({len(task_zip)} bytes)")
            
            # 存儲任務元數據到Redis（不包含ZIP文件）
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
                "updated_at": str(time.time()),
                "cpt_cost": str(calculated_cpt_cost),
                "has_task_zip": "1" if task_zip else "0"  # 標記是否有ZIP文件
            }
            
            # 存儲元數據到Redis
            self.redis_client.hset(task_key, mapping=task_info)
            
            # 將任務ID添加到用戶的任務集合中
            if user_id:
                user_tasks_key = f"user:{user_id}:tasks"
                self.redis_client.sadd(user_tasks_key, task_id)
                logging.info(f"已將任務 {task_id} 關聯到用戶 {user_id}")
            else:
                logging.warning(f"任務 {task_id} 無用戶ID，未能關聯到用戶")

            # 驗證存儲是否成功
            stored_user_id = self.redis_client.hget(task_key, "user_id")
            stored_cpt_cost = self.redis_client.hget(task_key, "cpt_cost")
            logging.info(f"任務 {task_id} 存儲後驗證: 用戶ID = {stored_user_id}, CPT成本 = {stored_cpt_cost}")

            logging.info(f"任務 {task_id} 已存儲 (用戶ID: {user_id}, CPT成本: {calculated_cpt_cost}/分鐘, 需求 GPU: '{gpu_name}')")
            return True
        except redis.RedisError as e:
            logging.error(f"Redis 錯誤，任務 {task_id} 存儲失敗: {e}")
            # 如果Redis失敗，清理可能已存儲的文件
            self.file_storage.cleanup_task_files(task_id)
            return False
        except Exception as e:
            logging.error(f"存儲任務 {task_id} 時發生未知錯誤: {e}", exc_info=True)
            # 如果出錯，清理可能已存儲的文件
            self.file_storage.cleanup_task_files(task_id)
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
        """存儲任務的最終結果 ZIP到硬碟"""
        task_key = f"task:{task_id}"
        try:
            # 存儲結果ZIP到硬碟
            if result_zip and not self.file_storage.store_result_zip(task_id, result_zip):
                logging.error(f"任務 {task_id} 結果ZIP存儲失敗")
                return False
            
            # 更新Redis中的狀態和標記
            update_data = {
                "status": "COMPLETED",
                "has_result_zip": "1" if result_zip else "0",
                "updated_at": str(time.time())
            }
            self.redis_client.hset(task_key, mapping=update_data)
            logging.info(f"任務 {task_id} 結果已存儲到硬碟，狀態更新為 COMPLETED")
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

    def store_logs(self, task_id, node_id, logs, timestamp):
        """存儲任務日誌"""
        try:
            # 存儲到任務的日誌字段
            task_key = f"task:{task_id}"
            
            # 檢查任務是否存在
            if not self.redis_client.exists(task_key):
                logging.warning(f"嘗試存儲日誌但任務 {task_id} 不存在")
                return False
            
            # 獲取現有日誌
            current_logs = self.redis_client.hget(task_key, "logs") or ""
            
            # 修復時間戳處理 - 將毫秒轉換為秒
            try:
                # 如果是毫秒時間戳，轉換為秒
                if timestamp > 1e12:  # 大於這個值說明是毫秒時間戳
                    timestamp_seconds = timestamp / 1000.0
                else:
                    timestamp_seconds = float(timestamp)
                
                timestamp_str = time.strftime('%Y-%m-%d %H:%M:%S', time.localtime(timestamp_seconds))
            except (ValueError, OSError) as e:
                # 如果時間戳無效，使用當前時間
                logging.warning(f"無效的時間戳 {timestamp}，使用當前時間: {e}")
                timestamp_str = time.strftime('%Y-%m-%d %H:%M:%S', time.localtime())
            
            # 格式化新日誌條目
            new_log_entry = f"[{timestamp_str}] [{node_id}] {logs}"
            
            # 追加新日誌
            updated_logs = current_logs + new_log_entry + "\n"
            
            # 限制日誌大小（保留最後 50KB）
            max_log_size = 50 * 1024
            if len(updated_logs) > max_log_size:
                updated_logs = updated_logs[-max_log_size:]
            
            # 更新日誌
            self.redis_client.hset(task_key, "logs", updated_logs)
            self.redis_client.hset(task_key, "updated_at", str(time.time()))
            
            logging.debug(f"任務 {task_id} 日誌已更新 (來自節點 {node_id})")
            return True
            
        except redis.RedisError as e:
            logging.error(f"Redis 錯誤，存儲任務 {task_id} 日誌失敗: {e}")
            return False
        except Exception as e:
            logging.error(f"存儲任務 {task_id} 日誌時發生未知錯誤: {e}", exc_info=True)
            return False

    def get_task_logs(self, task_id):
        """獲取任務日誌"""
        try:
            task_key = f"task:{task_id}"
            
            if not self.redis_client.exists(task_key):
                logging.warning(f"獲取日誌失敗：任務 {task_id} 不存在")
                return None
            
            # 獲取任務信息
            task_info = self.redis_client.hmget(task_key, ["logs", "status", "output"])
            logs = task_info[0] or ""
            status = task_info[1] or "UNKNOWN"
            output = task_info[2] or ""
            
            # 合併日誌和輸出
            combined_logs = []
            
            # 添加結構化日誌
            if logs:
                for line in logs.strip().split('\n'):
                    if line.strip():
                        combined_logs.append(line)
            
            # 添加輸出日誌（如果有的話）
            if output:
                combined_logs.append(f"--- Task Output ---")
                for line in output.strip().split('\n'):
                    if line.strip():
                        combined_logs.append(line)
            
            result = {
                "task_id": task_id,
                "status": status,
                "logs": combined_logs,
                "total_logs": len(combined_logs)
            }
            
            logging.debug(f"成功獲取任務 {task_id} 的日誌 ({len(combined_logs)} 條)")
            return result
            
        except redis.RedisError as e:
            logging.error(f"Redis 錯誤，獲取任務 {task_id} 日誌失敗: {e}")
            return None
        except Exception as e:
            logging.error(f"獲取任務 {task_id} 日誌時發生未知錯誤: {e}", exc_info=True)
            return None

    def get_task_info(self, task_id, include_zip=False):
        """獲取任務信息，根據需要從硬碟讀取ZIP文件"""
        task_key = f"task:{task_id}"
        task_info = {}
        string_keys = [
            "memory_gb", "cpu_score", "gpu_score", "gpu_memory_gb",
            "location", "gpu_name", "status", "output", "assigned_node",
            "user_id", "created_at", "updated_at", "logs", "cpt_cost",
            "has_task_zip", "has_result_zip"  # 添加ZIP文件存在標記
        ]

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

            # 根據需要從硬碟讀取ZIP文件
            if include_zip:
                # 讀取任務ZIP
                if task_info.get("has_task_zip") == "1":
                    task_zip = self.file_storage.get_task_zip(task_id)
                    task_info["task_zip"] = task_zip
                else:
                    task_info["task_zip"] = b""
                
                # 讀取結果ZIP
                if task_info.get("has_result_zip") == "1":
                    result_zip = self.file_storage.get_result_zip(task_id)
                    task_info["result_zip"] = result_zip
                else:
                    task_info["result_zip"] = b""
            else:
                # 不包含ZIP時，顯示存在狀態
                task_info["task_zip"] = "<file on disk>" if task_info.get("has_task_zip") == "1" else "<empty>"
                task_info["result_zip"] = "<file on disk>" if task_info.get("has_result_zip") == "1" else "<empty>"

            logging.debug(f"成功獲取任務 {task_id} 的信息 (include_zip={include_zip})")
            return task_info

        except redis.RedisError as e:
            logging.error(f"Redis 錯誤，獲取任務 {task_id} 信息失敗: {e}")
            return None
        except Exception as e:
            logging.error(f"獲取任務 {task_id} 信息時發生未知錯誤: {e}", exc_info=True)
            return None

    def get_task_result_zip(self, task_id):
        """專門獲取結果 ZIP (從硬碟讀取) - 支援 STOPPED 狀態"""
        task_key = f"task:{task_id}"
        try:
            # 從Redis獲取狀態
            status = self.redis_client.hget(task_key, "status") or "UNKNOWN"
            has_result = self.redis_client.hget(task_key, "has_result_zip") == "1"

            # 允許 COMPLETED 和 STOPPED 狀態的任務下載結果
            if status in ["COMPLETED", "STOPPED"]:
                if has_result:
                    result_zip = self.file_storage.get_result_zip(task_id)
                    if result_zip:
                        logging.info(f"任務 {task_id} 狀態為 {status}，從硬碟返回結果 ZIP ({len(result_zip)} bytes)")
                        return result_zip, status
                    else:
                        logging.warning(f"任務 {task_id} 狀態為 {status} 但硬碟上無結果文件")
                        return b"", status
                else:
                    logging.warning(f"任務 {task_id} 狀態為 {status} 但標記為無結果 ZIP")
                    return b"", status
            elif status in ["PENDING", "RUNNING"]:
                return b"", status
            else:
                # 對於其他狀態（如 FAILED），也檢查是否有結果
                if has_result:
                    result_zip = self.file_storage.get_result_zip(task_id)
                    if result_zip:
                        logging.info(f"任務 {task_id} 狀態為 {status}，但有結果可下載")
                        return result_zip, status
                return b"", status

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
                    cpt_cost = self.redis_client.hget(key, "cpt_cost") or "1"  # 從 Redis 讀取 cpt_cost
                    
                    tasks.append({
                        "task_id": task_id,
                        "user_id": user_id,
                        "cpt_cost": cpt_cost,  # 包含 cpt_cost
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

    def get_user_tasks(self, user_id):
        """獲取用戶的所有任務"""
        tasks = []
        try:
            task_keys = self.redis_client.keys("task:*")
            for key in task_keys:
                task_id = key.split(":", 1)[1]
                task_info = self.get_task_info(task_id, include_zip=False)
                if task_info and str(task_info.get("user_id", "")) == str(user_id):
                    tasks.append({
                        "task_id": task_id,
                        "status": task_info.get("status", "UNKNOWN"),
                        "created_at": task_info.get("created_at", ""),
                        "updated_at": task_info.get("updated_at", "")
                    })
            return tasks
        except Exception as e:
            logging.error(f"get_user_tasks error: {e}", exc_info=True)
            return []

    def get_task_status(self, task_id):
        """獲取任務狀態"""
        task_info = self.get_task_info(task_id, include_zip=False)
        if task_info:
            return {
                "task_id": task_id,
                "status": task_info.get("status", "UNKNOWN"),
                "user_id": task_info.get("user_id", ""),
                "created_at": task_info.get("created_at", ""),
                "updated_at": task_info.get("updated_at", "")
            }
        return None

    def mark_reward_given(self, task_id):
        """標記獎勵已發放"""
        task_key = f"task:{task_id}"
        try:
            self.redis_client.hset(task_key, "reward_distributed", "1")
            return True
        except Exception as e:
            logging.error(f"標記獎勵失敗: {e}")
            return False

    def force_stop_task(self, task_id):
        """強制停止任務"""
        return self.update_task_status(task_id, "STOPPED")

    def cleanup_task_data(self, task_id):
        """清理任務相關數據，包括硬碟文件"""
        try:
            task_key = f"task:{task_id}"
            
            # 獲取任務信息以便記錄
            task_info = self.get_task_info(task_id, include_zip=False)
            user_id = task_info.get("user_id") if task_info else None
            
            # 清理硬碟文件（立即清理，不等待標記）
            self.file_storage.cleanup_task_files(task_id)
            
            # 刪除Redis中的任務數據
            self.redis_client.delete(task_key)
            
            # 從用戶任務集合中移除
            if user_id:
                user_tasks_key = f"user:{user_id}:tasks"
                self.redis_client.srem(user_tasks_key, task_id)
            
            # 刪除任務日誌
            logs_key = f"task_logs:{task_id}"
            self.redis_client.delete(logs_key)
            
            logging.info(f"任務 {task_id} 的所有數據(包括硬碟文件)已清理")
            return True
            
        except Exception as e:
            logging.error(f"清理任務 {task_id} 數據失敗: {e}", exc_info=True)
            return False

    def check_user_balance_for_next_payment(self, username, cpt_cost):
        """檢查用戶餘額是否足夠下次付款（使用用戶名）"""
        try:
            user_manager = UserManager()
            # 使用用戶名查詢餘額
            user_row = user_manager.query_one("SELECT tokens FROM users WHERE username = ?", (username,))
            if user_row:
                return user_row['tokens'] >= cpt_cost
            return False
        except Exception as e:
            logging.error(f"檢查用戶 {username} 餘額失敗: {e}")
            return False

# --- MasterNodeServiceServicer 類 ---
class MasterNodeServiceServicer(nodepool_pb2_grpc.MasterNodeServiceServicer):
    def __init__(self):
        self.task_manager = TaskManager()
        self.node_manager = NodeManager()
        self._stop_event = threading.Event()
        self.dispatch_interval = 10
        self.dispatcher_thread = None
        self.health_check_interval = 5  # 縮短健康檢查間隔到5秒
        self.reward_interval = 60
        self.task_health = {}
        self.node_timeout_threshold = 10  # 節點超時閾值：10秒
        self.auth_manager = None  # 將在 server 啟動時設置
        self.start_task_dispatcher()
        self.start_health_checker()
        self.start_reward_scheduler()

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

    def _dispatch_pending_tasks_loop(self):
        """後台線程循環，定期檢查並分發任務"""
        while not self._stop_event.is_set():
            logging.debug("開始新一輪任務分發檢查...")
            try:
                self._dispatch_pending_tasks_once()
            except Exception as e:
                logging.error(f"任務分發循環出錯: {e}", exc_info=True)

            self._stop_event.wait(self.dispatch_interval)

        logging.info("任務分發後台線程循環結束")

    def start_health_checker(self):
        def health_check_loop():
            while not self._stop_event.is_set():
                try:
                    # 檢查所有 RUNNING 任務的節點心跳狀態（排除 STOPPED 狀態）
                    running_tasks = self.task_manager.get_running_tasks()
                    current_time = time.time()
                    
                    for task in running_tasks:
                        task_id = task["task_id"]
                        user_id = task.get("user_id")
                        
                        # 獲取完整任務信息
                        task_info = self.task_manager.get_task_info(task_id, include_zip=False)
                        if not task_info:
                            continue
                        
                        # 檢查任務狀態，如果是 STOPPED 則跳過健康檢查
                        current_status = task_info.get("status")
                        if current_status == "STOPPED":
                            logging.debug(f"任務 {task_id} 已停止，跳過健康檢查")
                            continue
                            
                        assigned_node = task_info.get("assigned_node")
                        if not assigned_node:
                            continue

                        # 檢查節點狀態
                        node_info = self.node_manager.get_node_info(assigned_node)
                        if not node_info:
                            logging.warning(f"節點 {assigned_node} 信息不存在，任務 {task_id} 可能需要重新分配")
                            self._increment_task_fail_count(task_id, assigned_node, user_id)
                            continue

                        last_heartbeat = node_info.get("last_heartbeat", 0)
                        if current_time - float(last_heartbeat) > self.node_timeout_threshold:
                            logging.warning(f"節點 {assigned_node} 心跳超時，任務 {task_id} 標記為失敗")
                            self._increment_task_fail_count(task_id, assigned_node, user_id)
                
                except Exception as e:
                    logging.error(f"健康檢查錯誤: {e}", exc_info=True)

                self._stop_event.wait(self.health_check_interval)
        
        threading.Thread(target=health_check_loop, daemon=True).start()
        logging.info("健康檢查線程已啟動")

    def start_reward_scheduler(self):
        """啟動獎勵調度器，定期發放完成任務的獎勵並檢查餘額"""
        def reward_scheduler_loop():
            while not self._stop_event.is_set():
                try:
                    # 處理正在運行的任務，檢查餘額並發放獎勵
                    running_tasks = self.task_manager.get_running_tasks()
                    
                    for task in running_tasks:
                        task_id = task["task_id"]
                        user_id = task.get("user_id")  # 任務發起者用戶名
                        
                        # 從 Redis 直接讀取 cpt_cost，而不是從 get_task_info
                        try:
                            cpt_cost_from_redis = self.task_manager.redis_client.hget(f"task:{task_id}", "cpt_cost")
                            if not cpt_cost_from_redis or cpt_cost_from_redis == "":
                                cpt_cost = 1  # 默認值
                            else:
                                cpt_cost = int(float(cpt_cost_from_redis))
                        except (ValueError, TypeError) as e:
                            logging.warning(f"任務 {task_id} 的 cpt_cost 值無效: '{cpt_cost_from_redis}', 使用默認值 1")
                            cpt_cost = 1  # 默認值
                        
                        # 獲取分配的節點信息
                        assigned_node = self.task_manager.redis_client.hget(f"task:{task_id}", "assigned_node")
                        
                        if user_id and assigned_node:
                            # 檢查任務發起者餘額是否足夠下次付款
                            if not self.task_manager.check_user_balance_for_next_payment(user_id, cpt_cost):
                                # 餘額不足，停止任務
                                self.task_manager.update_task_status(task_id, "STOPPED", assigned_node=None)
                                self.node_manager.report_status(assigned_node, "Idle")
                                
                                # 從健康檢查中移除
                                if task_id in self.task_health:
                                    del self.task_health[task_id]
                                
                                # 記錄到日誌
                                log_message = f"任務 {task_id} 因用戶 {user_id} 餘額不足已自動停止 (每分鐘需要 {cpt_cost} CPT)"
                                logging.warning(log_message)
                                self.task_manager.store_logs(task_id, "system", log_message, int(time.time()))
                                continue
                            
                            # 發放獎勵：從任務發起者轉帳給工作端用戶
                            user_manager = UserManager()
                            
                            # 使用用戶名進行轉帳
                            success, msg = user_manager.transfer_tokens(
                                user_id,        # 發送者用戶名（任務發起者）
                                assigned_node,  # 接收者用戶名（工作端用戶）
                                cpt_cost
                            )
                            
                            if success:
                                logging.info(f"任務 {task_id} 轉帳成功: {cpt_cost} CPT 從發起者 {user_id} 到工作端 {assigned_node}")
                                
                                # 轉帳後再次檢查餘額
                                if not self.task_manager.check_user_balance_for_next_payment(user_id, cpt_cost):
                                    # 餘額不足下次付款，停止任務
                                    self.task_manager.update_task_status(task_id, "STOPPED", assigned_node=None)
                                    self.node_manager.report_status(assigned_node, "Idle")
                                    
                                    if task_id in self.task_health:
                                        del self.task_health[task_id]
                                    
                                    log_message = f"任務 {task_id} 轉帳後發起者餘額不足，已停止任務"
                                    logging.warning(log_message)
                                    self.task_manager.store_logs(task_id, "system", log_message, int(time.time()))
                            else:
                                logging.error(f"任務 {task_id} 轉帳失敗: {msg}")
                                # 轉帳失敗，考慮停止任務
                                if "餘額不足" in msg:
                                    self.task_manager.update_task_status(task_id, "STOPPED", assigned_node=None)
                                    self.node_manager.report_status(assigned_node, "Idle")
                                    
                                    if task_id in self.task_health:
                                        del self.task_health[task_id]
                                    
                                    log_message = f"任務 {task_id} 因轉帳失敗已停止: {msg}"
                                    logging.warning(log_message)
                                    self.task_manager.store_logs(task_id, "system", log_message, int(time.time()))
                    
                    # 獲取已完成但未發放獎勵的任務
                    completed_tasks = self.task_manager.get_completed_tasks_without_reward()
                    
                    for task in completed_tasks:
                        task_id = task["task_id"]
                        user_id = task.get("user_id")
                        
                        # 獲取任務詳細信息
                        task_info = self.task_manager.get_task_info(task_id, include_zip=False)
                        if not task_info:
                            continue
                        
                        assigned_node = task_info.get("assigned_node")
                        
                        if assigned_node and user_id:
                            # 標記獎勵已處理
                            self.task_manager.mark_reward_given(task_id)
                            logging.info(f"任務 {task_id} 已完成，標記獎勵已發放")
                
                except Exception as e:
                    logging.error(f"獎勵調度錯誤: {e}", exc_info=True)
                
                self._stop_event.wait(self.reward_interval)
        
        threading.Thread(target=reward_scheduler_loop, daemon=True).start()
        logging.info("獎勵調度器已啟動")

    def _increment_task_fail_count(self, task_id, assigned_node, user_id):
        """
        增加任務失敗計數。
        - 如果是 worker 超時（健康檢查發現節點失聯），則重新分發（設為 PENDING）。
        - 如果是任務本身失敗（worker 回報失敗），則直接設為 FAILED，不重新分發。
        """
        try:
            if task_id not in self.task_health:
                self.task_health[task_id] = {
                    "fail_count": 0,
                    "last_check": 0,
                    "assigned_node": assigned_node,
                    "user_id": user_id,
                    "cpt_cost": 0
                }
            
            self.task_health[task_id]["fail_count"] += 1
            self.task_health[task_id]["last_check"] = time.time()
            
            # 只有健康檢查（worker 超時）才會進來這裡
            if self.task_health[task_id]["fail_count"] >= 3:
                # 重新分發：設為 PENDING，assigned_node 設為 None
                self.task_manager.update_task_status(task_id, "PENDING", assigned_node=None)
                logging.warning(f"任務 {task_id} 失敗次數過多（worker 超時），已重設為 PENDING，等待重新分發")
                del self.task_health[task_id]
        except Exception as e:
            logging.error(f"更新任務失敗計數錯誤: {e}")

    def stop_task_dispatcher(self):
        """停止任務分發線程"""
        if hasattr(self, '_stop_event'):
            self._stop_event.set()
        if hasattr(self, 'dispatcher_thread') and self.dispatcher_thread and self.dispatcher_thread.is_alive():
            self.dispatcher_thread.join(timeout=2)
            logging.info("任務分發線程已停止")

    def _dispatch_pending_tasks_once(self):
        """執行一次任務分發檢查"""
        try:
            pending_tasks = self.task_manager.get_pending_tasks()
            if not pending_tasks:
                logging.debug("沒有待處理的任務")
                return
            logging.info(f"===== 任務分發開始 =====")
            logging.info(f"發現 {len(pending_tasks)} 個待處理任務")
            
            for task in pending_tasks:
                task_id = task["task_id"]
                requirements = task["requirements"]
                
                logging.info(f"處理任務 {task_id}，需求: {requirements}")
                
                available_nodes = self.node_manager.get_available_nodes(
                    requirements["memory_gb"],
                    requirements["cpu_score"],
                    requirements["gpu_score"],
                    requirements["gpu_memory_gb"],
                    requirements["location"],
                    requirements["gpu_name"]
                )
                
                if available_nodes:
                    selected_node = available_nodes[0]
                    node_id = selected_node.node_id
                    logging.info(f"為任務 {task_id} 選擇節點: {node_id}")
                    
                    # 先嘗試推送任務到工作端，成功後才更新狀態
                    task_pushed_successfully = False
                    
                    try:
                        # 取得節點資訊
                        node_info = self.node_manager.get_node_info(node_id)
                        if not node_info:
                            logging.error(f"找不到節點 {node_id} 的資訊，無法推送任務")
                            continue
                        
                        worker_host = node_info.get("hostname")
                        worker_port = node_info.get("port", 50053)
                        
                        logging.info(f"節點 {node_id} 詳細信息: host={worker_host}, port={worker_port}")
                        
                        # 驗證節點連接性
                        if not worker_host or worker_host == "127.0.0.1":
                            logging.warning(f"節點 {node_id} 使用無效 IP {worker_host}，跳過推送")
                            continue
                        
                        # 取得任務內容並檢查大小
                        task_info = self.task_manager.get_task_info(task_id, include_zip=True)
                        if not task_info:
                            logging.error(f"找不到任務 {task_id} 的內容，無法推送")
                            continue
                        task_zip = task_info.get("task_zip", b"")
                        
                        # 根據檔案大小動態調整超時時間
                        file_size_mb = len(task_zip) / (1024 * 1024)
                        base_timeout = 30  # 基礎30秒
                        size_timeout = max(file_size_mb * 2, 10)  # 每MB給2秒，最少10秒
                        total_timeout = min(base_timeout + size_timeout, 120)  # 最多2分鐘
                        
                        logging.info(f"推送任務 {task_id} 到 {worker_host}:{worker_port} (大小: {file_size_mb:.1f}MB, 超時: {total_timeout:.0f}秒)")
                        
                        # 建立 gRPC 連線
                        channel = grpc.insecure_channel(
                            f"{worker_host}:{worker_port}",
                            options=[
                                ('grpc.keepalive_time_ms', 10000),
                                ('grpc.keepalive_timeout_ms', 5000),
                                ('grpc.keepalive_permit_without_calls', True),
                                ('grpc.http2.max_pings_without_data', 0),
                                ('grpc.http2.min_time_between_pings_ms', 10000),
                                ('grpc.http2.min_ping_interval_without_data_ms', 5000),
                                # 增加消息大小限制以支持大檔案
                                ('grpc.max_receive_message_length', 100 * 1024 * 1024),  # 100MB
                                ('grpc.max_send_message_length', 100 * 1024 * 1024),     # 100MB
                            ]
                        )
                        
                        # 先測試連接（快速測試）
                        try:
                            grpc.channel_ready_future(channel).result(timeout=5)
                            logging.info(f"成功連接到節點 {node_id} ({worker_host}:{worker_port})")
                        except grpc.FutureTimeoutError:
                            logging.error(f"連接到節點 {node_id} 超時，節點可能離線")
                            channel.close()
                            continue
                        
                        stub = nodepool_pb2_grpc.WorkerNodeServiceStub(channel)
                        req = nodepool_pb2.ExecuteTaskRequest(
                            node_id=node_id,
                            task_id=task_id,
                            task_zip=task_zip
                        )
                        
                        logging.info(f"開始發送 ExecuteTask 請求到節點 {node_id}...")
                        
                        # 使用動態計算的超時時間
                        resp = stub.ExecuteTask(req, timeout=total_timeout)
                        channel.close()
                        
                        logging.info(f"收到節點 {node_id} 的響應: success={resp.success}, message='{resp.message}'")
                        
                        if resp.success:
                            logging.info(f"已成功推送任務 {task_id} 給節點 {node_id} (耗時包含 {file_size_mb:.1f}MB 傳輸)")
                            task_pushed_successfully = True
                        else:
                            logging.error(f"節點 {node_id} 拒絕任務 {task_id}: {resp.message}")
                            continue
                            
                    except grpc.RpcError as e:
                        if e.code() == grpc.StatusCode.DEADLINE_EXCEEDED:
                            logging.error(f"推送任務 {task_id} 給節點 {node_id} 超時 (檔案大小: {file_size_mb:.1f}MB)，可能是網路或檔案傳輸較慢")
                        elif e.code() == grpc.StatusCode.UNAVAILABLE:
                            logging.error(f"無法連接到節點 {node_id}，節點可能離線")
                        elif e.code() == grpc.StatusCode.RESOURCE_EXHAUSTED:
                            logging.error(f"推送任務 {task_id} 失敗：訊息太大 (檔案大小: {file_size_mb:.1f}MB)")
                        else:
                            logging.error(f"推送任務 {task_id} 給節點 {node_id} gRPC 錯誤: {e.code()} - {e.details()}")
                        continue
                    except Exception as e:
                        logging.error(f"推送任務 {task_id} 給節點 {node_id} 發生錯誤: {e}", exc_info=True)
                        continue
                    
                    # 只有成功推送任務後才更新狀態和設置節點為忙碌
                    if task_pushed_successfully:
                        success = self.task_manager.update_task_status(
                            task_id, 
                            "RUNNING", 
                            assigned_node=node_id
                        )
                        if success:
                            self.node_manager.report_status(node_id, "BUSY")
                            logging.info(f"任務 {task_id} 已成功分配給節點 {node_id} 並開始執行")
                            self.task_health[task_id] = {
                                "fail_count": 0,
                                "last_check": time.time(),
                                "assigned_node": node_id,
                                "user_id": task.get("user_id", ""),
                                "cpt_cost": 0
                            }
                        else:
                            logging.error(f"更新任務 {task_id} 狀態失敗")
                else:
                    logging.debug(f"任務 {task_id} 暫時沒有符合要求的可用節點")
            
            logging.info(f"===== 任務分發結束 =====")
        except Exception as e:
            logging.error(f"分發任務時發生錯誤: {e}", exc_info=True)

    def UploadTask(self, request, context):
        """處理任務上傳"""
        try:
            # 驗證 user_id
            if not request.user_id:
                return nodepool_pb2.UploadTaskResponse(
                    success=False,
                    message="Missing user_id"
                )
            
            # 存儲任務
            success = self.task_manager.store_task(
                request.task_id,
                request.task_zip,
                request.memory_gb,
                request.cpu_score,
                request.gpu_score,
                request.gpu_memory_gb,
                request.location,
                request.gpu_name,
                request.user_id
            )
            
            if success:
                logging.info(f"任務 {request.task_id} 上傳成功 (用戶: {request.user_id})")
                return nodepool_pb2.UploadTaskResponse(
                    success=True,
                    message="任務已上傳"
                )
            else:
                return nodepool_pb2.UploadTaskResponse(
                    success=False,
                    message="任務上傳失敗"
                )
                
        except Exception as e:
            logging.error(f"UploadTask 服務錯誤: {e}", exc_info=True)
            context.set_code(grpc.StatusCode.INTERNAL)
            context.set_details(f"Internal server error: {str(e)}")
            return nodepool_pb2.UploadTaskResponse(
                success=False,
                message=f"Internal error: {str(e)}"
            )

    def PollTaskStatus(self, request, context):
        """輪詢任務狀態"""
        try:
            task_info = self.task_manager.get_task_info(request.task_id, include_zip=False)
            
            if not task_info:
                return nodepool_pb2.PollTaskStatusResponse(
                    task_id=request.task_id,
                    status="NOT_FOUND",
                    output=[],
                    message="Task not found"
                )
            
            # 獲取輸出行
            output_lines = []
            if task_info.get("output"):
                output_lines = task_info["output"].strip().split('\n')
                output_lines = [line for line in output_lines if line.strip()]
            
            return nodepool_pb2.PollTaskStatusResponse(
                task_id=request.task_id,
                status=task_info.get("status", "UNKNOWN"),
                output=output_lines,
                message=f"Task status: {task_info.get('status', 'UNKNOWN')}"
            )
            
        except Exception as e:
            logging.error(f"PollTaskStatus 服務錯誤: {e}", exc_info=True)
            context.set_code(grpc.StatusCode.INTERNAL)
            context.set_details(f"Internal server error: {str(e)}")
            return nodepool_pb2.PollTaskStatusResponse(
                task_id=request.task_id,
                status="ERROR",
                output=[],
                message=f"Internal error: {str(e)}"
            )

    def StoreOutput(self, request, context):
        """存儲任務輸出"""
        try:
            success = self.task_manager.store_output(request.task_id, request.output)
            
            if success:
                return nodepool_pb2.StatusResponse(
                    success=True,
                    message=f"Output for task {request.task_id} stored successfully"
                )
            else:
                return nodepool_pb2.StatusResponse(
                    success=False,
                    message=f"Failed to store output for task {request.task_id}"
                )
                
        except Exception as e:
            logging.error(f"StoreOutput 服務錯誤: {e}", exc_info=True)
            context.set_code(grpc.StatusCode.INTERNAL)
            context.set_details(f"Internal server error: {str(e)}")
            return nodepool_pb2.StatusResponse(success=False, message=f"Internal error: {str(e)}")

    def StoreResult(self, request, context):
        """存儲任務結果"""
        try:
            success = self.task_manager.store_result(request.task_id, request.result_zip)
            
            if success:
                return nodepool_pb2.StatusResponse(
                    success=True,
                    message=f"Result for task {request.task_id} stored successfully"
                )
            else:
                return nodepool_pb2.StatusResponse(
                    success=False,
                    message=f"Failed to store result for task {request.task_id}"
                )
                
        except Exception as e:
            logging.error(f"StoreResult 服務錯誤: {e}", exc_info=True)
            context.set_code(grpc.StatusCode.INTERNAL)
            context.set_details(f"Internal server error: {str(e)}")
            return nodepool_pb2.StatusResponse(success=False, message=f"Internal error: {str(e)}")

    def StoreLogs(self, request, context):
        """存儲任務日誌"""
        try:
            success = self.task_manager.store_logs(
                request.task_id,
                request.node_id,
                request.logs,
                request.timestamp
            )
            if success:
                return nodepool_pb2.StatusResponse(
                    success=True,
                    message=f"Logs for task {request.task_id} stored successfully"
                )
            else:
                return nodepool_pb2.StatusResponse(
                    success=False,
                    message=f"Failed to store logs for task {request.task_id}"
                )
        except Exception as e:
            logging.error(f"StoreLogs 服務錯誤: {e}", exc_info=True)
            return nodepool_pb2.StatusResponse(
                success=False,
                message=f"Internal error: {str(e)}"
            )

    def GetTaskLogs(self, request, context):
        """獲取任務日誌"""
        try:
            # 驗證 token
            username = self._extract_user_from_token(request.token)
            if not username:
                return nodepool_pb2.GetTaskLogsResponse(
                    success=False,
                    message="Invalid or expired token",
                    logs=""
                )
            
            # 取得日誌
            log_info = self.task_manager.get_task_logs(request.task_id)
            if not log_info:
                return nodepool_pb2.GetTaskLogsResponse(
                    success=False,
                    message="Task not found or no logs",
                    logs=""
                )
            
            # 將日誌合併為字串
            logs_str = "\n".join(log_info.get("logs", []))
            return nodepool_pb2.GetTaskLogsResponse(
                success=True,
                message="Logs retrieved successfully",
                logs=logs_str
            )
        except Exception as e:
            logging.error(f"GetTaskLogs 服務錯誤: {e}", exc_info=True)
            return nodepool_pb2.GetTaskLogsResponse(
                success=False,
                message=f"Internal error: {str(e)}",
                logs=""
            )

    def ReturnTaskResult(self, request, context):
        """接收工作端傳回的任務結果（包括停止的任務）"""
        try:
            task_id = request.task_id
            result_zip = request.result_zip
            
            logging.info(f"接收到任務 {task_id} 的結果 ({len(result_zip)} bytes)")
            
            # 存儲結果
            if self.task_manager.store_result(task_id, result_zip):
                # 獲取任務信息以便清理
                task_info = self.task_manager.get_task_info(task_id, include_zip=False)
                assigned_node = task_info.get("assigned_node") if task_info else None
                
                # 清理節點狀態
                if assigned_node:
                    self.node_manager.report_status(assigned_node, "Idle")
                    logging.info(f"節點 {assigned_node} 狀態已重置為 Idle")
                
                # 從健康檢查中移除
                if task_id in self.task_health:
                    del self.task_health[task_id]
                
                # 更新任務的 assigned_node 為空（表示已完成）
                self.task_manager.update_task_status(task_id, task_info.get("status", "COMPLETED"), assigned_node=None)
                
                logging.info(f"任務 {task_id} 結果已成功存儲並清理完成")
                return nodepool_pb2.ReturnTaskResultResponse(
                    success=True,
                    message="Task result stored successfully"
                )
            else:
                logging.error(f"存儲任務 {task_id} 結果失敗")
                return nodepool_pb2.ReturnTaskResultResponse(
                    success=False,
                    message="Failed to store task result"
                )
        except Exception as e:
            logging.error(f"ReturnTaskResult 錯誤: {e}", exc_info=True)
            return nodepool_pb2.ReturnTaskResultResponse(
                success=False,
                message=f"Error: {str(e)}"
            )

    def TaskCompleted(self, request, context):
        """處理任務完成通知"""
        try:
            task_id = request.task_id
            node_id = request.node_id
            task_success = request.success
            
            # 檢查任務當前狀態
            task_info = self.task_manager.get_task_info(task_id, include_zip=False)
            if not task_info:
                logging.warning(f"任務 {task_id} 不存在，無法更新完成狀態")
                return nodepool_pb2.StatusResponse(success=False, message="Task not found")
            
            current_status = task_info.get("status")
            logging.info(f"收到任務 {task_id} 完成通知，成功: {task_success}，當前狀態: {current_status}")
            
            # 如果任務已經是 COMPLETED，不要覆蓋為 FAILED
            if current_status == "COMPLETED":
                logging.info(f"任務 {task_id} 已經是 COMPLETED 狀態，保持不變")
                message = f"Task {task_id} already completed"
            elif task_success:
                # 任務成功完成但還未標记為 COMPLETED
                success = self.task_manager.update_task_status(task_id, "COMPLETED")
                message = f"Task {task_id} completed successfully"
                logging.info(message)
            else:
                # 任務本身失敗（worker 回報失敗），直接設為 FAILED，不重新分發
                success = self.task_manager.update_task_status(task_id, "FAILED")
                message = f"Task {task_id} failed"
                logging.info(message)
                # 從健康檢查中移除，**不會進行重新分發**
                if task_id in self.task_health:
                    del self.task_health[task_id]
            
            # 將節點狀態設回 Idle
            if node_id:
                self.node_manager.report_status(node_id, "Idle")
            
            # 從健康檢查中移除
            if task_id in self.task_health:
                del self.task_health[task_id]
            
            return nodepool_pb2.StatusResponse(success=True, message=message)
        except Exception as e:
            logging.error(f"TaskCompleted 服務錯誤: {e}", exc_info=True)
            context.set_code(grpc.StatusCode.INTERNAL)
            context.set_details(f"Internal server error: {str(e)}")
            return nodepool_pb2.StatusResponse(success=False, message=f"Internal error: {str(e)}")

    # 修正 GetAllTasks 中的用戶ID查詢問題
    def GetAllTasks(self, request, context):
        """獲取用戶的所有任務（修正版本）"""
        try:
            if not request.token:
                return nodepool_pb2.GetAllTasksResponse(
                    success=False,
                    message="Token is required"
                )
            
            # 從 token 中獲取用戶名
            username = self._extract_user_from_token(request.token)
            if not username:
                return nodepool_pb2.GetAllTasksResponse(
                    success=False,
                    message="Invalid or expired token"
                )
            
            logging.info(f"用戶 {username} 請求獲取所有任務")
            
            # 查詢用戶的任務（使用用戶名而不是用戶ID）
            task_statuses = []
            try:
                task_keys = self.task_manager.redis_client.keys("task:*")
                task_count = 0
                
                for key in task_keys:
                    if task_count >= 100:  # 限制最多返回100個任務
                        break
                        
                    task_id = key.split(":", 1)[1]
                    task_user_id = self.task_manager.redis_client.hget(key, "user_id")
                    
                    # 直接比較用戶名，因為存儲時用的是用戶名
                    if str(task_user_id) == username:
                        status = self.task_manager.redis_client.hget(key, "status") or "UNKNOWN"
                        created_at = self.task_manager.redis_client.hget(key, "created_at") or ""
                        updated_at = self.task_manager.redis_client.hget(key, "updated_at") or ""
                        assigned_node = self.task_manager.redis_client.hget(key, "assigned_node") or ""
                        
                        task_status = nodepool_pb2.TaskStatus(
                            task_id=task_id,
                            status=status,
                            created_at=created_at,
                            updated_at=updated_at,
                            assigned_node=assigned_node
                        )
                        task_statuses.append(task_status)
                        task_count += 1
                
            except Exception as e:
                logging.error(f"查詢用戶任務時發生錯誤: {e}")
            
            logging.info(f"返回用戶 {username} 的 {len(task_statuses)} 個任務")
            return nodepool_pb2.GetAllTasksResponse(
                success=True,
                message=f"Found {len(task_statuses)} tasks",
                tasks=task_statuses
            )
            
        except Exception as e:
            logging.error(f"GetAllTasks error: {e}", exc_info=True)
            return nodepool_pb2.GetAllTasksResponse(
                success=False,
                message=f"Internal error: {str(e)}"
            )

    def _extract_user_from_token(self, token):
        """從 token 中提取用戶名（簡化實現）"""
        try:
            # 直接解析 JWT token
            payload = jwt.decode(token, Config.SECRET_KEY, algorithms=["HS256"])
            user_id = payload.get("user_id")
            
            if user_id:
                # 從資料庫獲取用戶名
                user_manager = UserManager()
                user_info = user_manager.query_one("SELECT username FROM users WHERE id = ?", (user_id,))
                if user_info:
                    return user_info["username"]
                else:
                    logging.warning(f"找不到用戶ID {user_id} 對應的用戶名")
                    return None
            return None
        except jwt.ExpiredSignatureError:
            logging.warning("Token 已過期")
            return None
        except jwt.InvalidTokenError:
            logging.warning("無效的 Token")
            return None
        except Exception as e:
            logging.error(f"Extract user from token failed: {e}")
            return None

    def StopTask(self, request, context):
        """停止任務"""
        try:
            task_id = request.task_id
            token = request.token
            
            logging.info(f"收到停止任務請求: task_id={task_id}, token={token[:10]}...")
            
            # 驗證 token 並獲取用戶信息
            username = self._extract_user_from_token(token)
            if not username:
                logging.warning(f"停止任務 {task_id} 失敗: 無效或過期的 token")
                return nodepool_pb2.StopTaskResponse(
                    success=False, 
                    message="Invalid or expired token"
                )
            
            # 獲取用戶ID
            user_manager = UserManager()
            user_info = user_manager.query_one("SELECT id FROM users WHERE username = ?", (username,))
            if not user_info:
                logging.warning(f"停止任務 {task_id} 失敗: 找不到用戶 {username}")
                return nodepool_pb2.StopTaskResponse(
                    success=False, 
                    message="User not found"
                )
            
            user_id = str(user_info["id"])
            logging.info(f"用戶 {username} (ID: {user_id}) 請求停止任務 {task_id}")
            
            # 檢查任務是否存在
            task_info = self.task_manager.get_task_info(task_id, include_zip=False)
            if not task_info:
                logging.warning(f"停止任務 {task_id} 失敗: 任務不存在")
                return nodepool_pb2.StopTaskResponse(
                    success=False, 
                    message="任務不存在"
                )
            
            # 檢查任務所有權
            task_user_id = task_info.get("user_id", "")
            logging.info(f"任務 {task_id} 的擁有者: {task_user_id}, 請求停止的用戶: {username}")
            
            # 修正：任務存儲時可能使用用戶名，所以需要靈活比較
            task_belongs_to_user = (
                str(task_user_id) == str(user_id) or  # 比較用戶ID
                str(task_user_id) == username         # 比較用戶名
            )
            
            if not task_belongs_to_user:
                logging.warning(f"用戶 {username} 無權限停止任務 {task_id} (任務擁有者: {task_user_id})")
                return nodepool_pb2.StopTaskResponse(
                    success=False, 
                    message="無權限停止此任務"
                )
            
            # 檢查任務當前狀態
            current_status = task_info.get("status")
            if current_status in ["COMPLETED", "FAILED", "STOPPED"]:
                logging.info(f"任務 {task_id} 已處於 {current_status} 狀態，無需停止")
                return nodepool_pb2.StopTaskResponse(
                    success=False, 
                    message=f"任務已處於 {current_status} 狀態，無法停止"
                )
            
            # 立即更新任務狀態為 STOPPED，這樣健康檢查就不會視為超時
            success = self.task_manager.update_task_status(task_id, "STOPPED")
            if not success:
                logging.error(f"更新任務 {task_id} 狀態為 STOPPED 失敗")
                return nodepool_pb2.StopTaskResponse(
                    success=False, 
                    message="更新任務狀態失敗"
                )
            
            # 記錄停止日誌
            stop_log = f"任務 {task_id} 已被用戶 {username} 請求停止"
            self.task_manager.store_logs(task_id, "system", stop_log, int(time.time()))
            
            # 執行停止任務
            assigned_node = task_info.get("assigned_node")
            
            # 通知工作端停止任務（工作端會負責打包結果）
            if assigned_node:
                try:
                    node_info = self.node_manager.get_node_info(assigned_node)
                    if node_info and node_info.get("port", 0) > 0:
                        worker_host = node_info.get("hostname")
                        worker_port = node_info.get("port", 50053)
                        
                        channel = grpc.insecure_channel(f"{worker_host}:{worker_port}")
                        stub = nodepool_pb2_grpc.WorkerNodeServiceStub(channel)
                        stop_req = nodepool_pb2.StopTaskExecutionRequest(task_id=task_id)
                        
                        # 發送停止請求給工作端，不用等太久，因為狀態已更新
                        stop_response = stub.StopTaskExecution(stop_req, timeout=50)  # 50秒足夠
                        channel.close()
                        
                        if stop_response.success:
                            logging.info(f"已成功通知節點 {assigned_node} 停止任務 {task_id}")
                            
                            # 記錄成功通知日誌
                            success_log = f"任務 {task_id} 停止請求已發送到節點 {assigned_node}，工作端正在打包結果"
                            self.task_manager.store_logs(task_id, "system", success_log, int(time.time()))
                            
                        else:
                            logging.warning(f"節點 {assigned_node} 拒絕停止任務 {task_id}: {stop_response.message}")
                            # 即使工作端拒絕，狀態已經更新為 STOPPED，這是合理的
                            
                except Exception as e:
                    logging.warning(f"通知節點 {assigned_node} 停止任務 {task_id} 失敗: {e}")
                    # 即使通知失敗，狀態已經更新為 STOPPED
            
            # 從健康檢查中移除（如果存在）
            if task_id in self.task_health:
                del self.task_health[task_id]
                logging.info(f"任務 {task_id} 已從健康檢查中移除")
            
            # 成功停止（狀態已更新）
            return nodepool_pb2.StopTaskResponse(
                success=True, 
                message=f"任務 {task_id} 已停止，工作端正在處理結果打包"
            )
                
        except Exception as e:
            logging.error(f"StopTask 服務錯誤: {e}", exc_info=True)
            return nodepool_pb2.StopTaskResponse(
                success=False, 
                message=f"內部錯誤: {str(e)}"
            )

    def GetTaskResult(self, request, context):
            """主控端請求任務結果，節點池回傳暫存的 ZIP - 支援 STOPPED 狀態，傳輸完成後自動清理文件和Redis數據"""
            try:
                task_id = request.task_id

                # 驗證 token（如果有提供）
                if hasattr(request, 'token') and request.token:
                    username = self._extract_user_from_token(request.token)
                    if not username:
                        context.set_code(grpc.StatusCode.UNAUTHENTICATED)
                        context.set_details("Invalid or expired token")
                        return nodepool_pb2.GetTaskResultResponse(
                            success=False,
                            message="Authentication failed",
                            result_zip=b""
                        )

                    # 檢查任務是否屬於該用戶
                    task_info = self.task_manager.get_task_info(task_id, include_zip=False)
                    if task_info and str(task_info.get("user_id")) != username:
                        context.set_code(grpc.StatusCode.PERMISSION_DENIED)
                        context.set_details("Access denied")
                        return nodepool_pb2.GetTaskResultResponse(
                            success=False,
                            message="Access denied to this task",
                            result_zip=b""
                        )

                result_zip, status = self.task_manager.get_task_result_zip(task_id)

                # 支援 COMPLETED 和 STOPPED 狀態的結果下載
                if status in ["COMPLETED", "STOPPED"]:
                    if result_zip:
                        status_msg = "completed successfully" if status == "COMPLETED" else "stopped but has partial results"

                        # 在成功返回結果前，標記文件可以清理
                        self.task_manager.file_storage.mark_for_cleanup(task_id)

                        # 安排延遲清理文件，確保文件傳輸完成
                        self.task_manager.file_storage.delayed_cleanup_task_files(task_id, delay_seconds=10)

                        # 清理Redis數據（包括任務元數據、日誌和用戶任務關聯）
                        self.task_manager.cleanup_task_data(task_id)

                        logging.info(f"任務 {task_id} 結果已成功傳輸 ({len(result_zip)} bytes)，已清理Redis數據並安排文件清理")

                        return nodepool_pb2.GetTaskResultResponse(
                            success=True,
                            message=f"Task {task_id} {status_msg}",
                            result_zip=result_zip
                        )
                    else:
                        return nodepool_pb2.GetTaskResultResponse(
                            success=False,
                            message=f"Task {task_id} is {status.lower()} but no result available",
                            result_zip=b""
                        )
                elif status in ["PENDING", "RUNNING"]:
                    return nodepool_pb2.GetTaskResultResponse(
                        success=False,
                        message=f"Task {task_id} is still {status.lower()}",
                        result_zip=b""
                    )
                elif status == "FAILED":
                    # 失敗的任務也可能有部分結果
                    if result_zip:
                        # 失敗的任務結果也需要清理
                        self.task_manager.file_storage.mark_for_cleanup(task_id)
                        self.task_manager.file_storage.delayed_cleanup_task_files(task_id, delay_seconds=10)

                        # 清理Redis數據
                        self.task_manager.cleanup_task_data(task_id)

                        logging.info(f"任務 {task_id} 失敗但有部分結果已傳輸，已清理Redis數據並安排文件清理")

                        return nodepool_pb2.GetTaskResultResponse(
                            success=True,
                            message=f"Task {task_id} failed but has partial results",
                            result_zip=result_zip
                        )
                    else:
                        return nodepool_pb2.GetTaskResultResponse(
                            success=False,
                            message=f"Task {task_id} failed with no results",
                            result_zip=b""
                        )
                else:
                    return nodepool_pb2.GetTaskResultResponse(
                        success=False,
                        message=f"Task {task_id} status: {status}",
                        result_zip=b""
                    )
            except Exception as e:
                logging.error(f"GetTaskResult 服務錯誤: {e}", exc_info=True)
                context.set_code(grpc.StatusCode.INTERNAL)
                context.set_details(f"Internal server error: {str(e)}")
                return nodepool_pb2.GetTaskResultResponse(
                    success=False,
                    message=f"Internal error: {str(e)}",
                    result_zip=b""
                )