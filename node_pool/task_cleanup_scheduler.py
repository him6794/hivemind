"""
任務自動清理排程器

此模組負責定期清理已完成、失敗或停止的任務，避免 Redis 和硬碟資源持續占用。
"""

import time
import logging
import threading
from datetime import datetime, timedelta

class TaskCleanupScheduler:
    """任務自動清理排程器"""
    
    def __init__(self, task_manager, cleanup_interval=3600, task_retention_hours=24):
        """
        初始化清理排程器
        
        Args:
            task_manager: TaskManager 實例
            cleanup_interval: 清理檢查間隔（秒），預設 3600（1小時）
            task_retention_hours: 任務保留時間（小時），預設 24 小時
        """
        self.task_manager = task_manager
        self.cleanup_interval = cleanup_interval
        self.task_retention_hours = task_retention_hours
        self._stop_event = threading.Event()
        self._cleanup_thread = None
        
        logging.info(f"任務清理排程器初始化: 檢查間隔={cleanup_interval}秒, 保留時間={task_retention_hours}小時")
    
    def start(self):
        """啟動清理排程器"""
        if self._cleanup_thread is None or not self._cleanup_thread.is_alive():
            self._stop_event.clear()
            self._cleanup_thread = threading.Thread(
                target=self._cleanup_loop, 
                name="TaskCleanupScheduler",
                daemon=True
            )
            self._cleanup_thread.start()
            logging.info("任務清理排程器已啟動")
    
    def stop(self):
        """停止清理排程器"""
        self._stop_event.set()
        if self._cleanup_thread and self._cleanup_thread.is_alive():
            self._cleanup_thread.join(timeout=5)
        logging.info("任務清理排程器已停止")
    
    def _cleanup_loop(self):
        """清理循環主邏輯"""
        while not self._stop_event.is_set():
            try:
                self._perform_cleanup()
            except Exception as e:
                logging.error(f"任務清理循環出錯: {e}", exc_info=True)
            
            # 等待下一次清理週期
            self._stop_event.wait(self.cleanup_interval)
    
    def _perform_cleanup(self):
        """執行清理操作"""
        logging.info("開始任務自動清理檢查...")
        
        try:
            # 獲取所有任務 keys
            task_keys = self.task_manager.redis_client.keys("task:*")
            
            if not task_keys:
                logging.info("沒有任務需要清理")
                return
            
            current_time = time.time()
            retention_seconds = self.task_retention_hours * 3600
            
            cleaned_count = 0
            checked_count = 0
            
            for task_key in task_keys:
                try:
                    task_key_str = task_key.decode('utf-8') if isinstance(task_key, bytes) else task_key
                    task_id = task_key_str.replace("task:", "")
                    
                    # 獲取任務資訊
                    task_info = self.task_manager.redis_client.hmget(
                        task_key_str,
                        ["status", "updated_at", "created_at"]
                    )
                    
                    if not task_info or not task_info[0]:
                        continue
                    
                    status = task_info[0].decode('utf-8') if isinstance(task_info[0], bytes) else task_info[0]
                    updated_at = task_info[1].decode('utf-8') if task_info[1] and isinstance(task_info[1], bytes) else task_info[1]
                    created_at = task_info[2].decode('utf-8') if task_info[2] and isinstance(task_info[2], bytes) else task_info[2]
                    
                    checked_count += 1
                    
                    # 只清理已完成、失敗或停止的任務
                    if status not in ["COMPLETED", "FAILED", "STOPPED"]:
                        continue
                    
                    # 檢查任務更新時間
                    task_time = float(updated_at) if updated_at else (float(created_at) if created_at else 0)
                    
                    if task_time == 0:
                        logging.warning(f"任務 {task_id} 沒有時間戳，跳過清理")
                        continue
                    
                    # 檢查是否超過保留時間
                    age_seconds = current_time - task_time
                    
                    if age_seconds > retention_seconds:
                        # 計算任務年齡（小時）
                        age_hours = age_seconds / 3600
                        
                        logging.info(
                            f"清理過期任務: {task_id} "
                            f"(狀態={status}, 年齡={age_hours:.1f}小時, 閾值={self.task_retention_hours}小時)"
                        )
                        
                        # 執行清理
                        success = self.task_manager.cleanup_task_data(task_id)
                        
                        if success:
                            cleaned_count += 1
                        else:
                            logging.warning(f"清理任務 {task_id} 失敗")
                    
                except Exception as e:
                    logging.error(f"處理任務 {task_key} 時出錯: {e}", exc_info=True)
                    continue
            
            if cleaned_count > 0:
                logging.info(
                    f"任務自動清理完成: 檢查了 {checked_count} 個任務，清理了 {cleaned_count} 個過期任務"
                )
            else:
                logging.info(f"任務自動清理完成: 檢查了 {checked_count} 個任務，沒有需要清理的過期任務")
        
        except Exception as e:
            logging.error(f"任務自動清理失敗: {e}", exc_info=True)
    
    def force_cleanup_task(self, task_id):
        """
        強制清理指定任務（無視保留時間）
        
        Args:
            task_id: 任務 ID
            
        Returns:
            bool: 是否清理成功
        """
        try:
            logging.info(f"強制清理任務: {task_id}")
            return self.task_manager.cleanup_task_data(task_id)
        except Exception as e:
            logging.error(f"強制清理任務 {task_id} 失敗: {e}", exc_info=True)
            return False
    
    def get_cleanup_stats(self):
        """
        獲取清理統計資訊
        
        Returns:
            dict: 統計資訊
        """
        try:
            task_keys = self.task_manager.redis_client.keys("task:*")
            total_tasks = len(task_keys)
            
            status_counts = {
                "PENDING": 0,
                "RUNNING": 0,
                "COMPLETED": 0,
                "FAILED": 0,
                "STOPPED": 0,
                "OTHER": 0
            }
            
            eligible_for_cleanup = 0
            current_time = time.time()
            retention_seconds = self.task_retention_hours * 3600
            
            for task_key in task_keys:
                try:
                    task_key_str = task_key.decode('utf-8') if isinstance(task_key, bytes) else task_key
                    task_info = self.task_manager.redis_client.hmget(
                        task_key_str,
                        ["status", "updated_at"]
                    )
                    
                    if not task_info or not task_info[0]:
                        continue
                    
                    status = task_info[0].decode('utf-8') if isinstance(task_info[0], bytes) else task_info[0]
                    updated_at = task_info[1].decode('utf-8') if task_info[1] and isinstance(task_info[1], bytes) else task_info[1]
                    
                    # 統計狀態
                    if status in status_counts:
                        status_counts[status] += 1
                    else:
                        status_counts["OTHER"] += 1
                    
                    # 檢查是否符合清理條件
                    if status in ["COMPLETED", "FAILED", "STOPPED"] and updated_at:
                        task_time = float(updated_at)
                        age_seconds = current_time - task_time
                        if age_seconds > retention_seconds:
                            eligible_for_cleanup += 1
                
                except Exception:
                    continue
            
            return {
                "total_tasks": total_tasks,
                "status_counts": status_counts,
                "eligible_for_cleanup": eligible_for_cleanup,
                "retention_hours": self.task_retention_hours,
                "cleanup_interval_seconds": self.cleanup_interval
            }
        
        except Exception as e:
            logging.error(f"獲取清理統計失敗: {e}", exc_info=True)
            return {
                "error": str(e)
            }
