#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
HiveMind 用戶管理工具
用於管理資料庫中的用戶帳戶
"""

import sys
import os
import sqlite3
import logging
from user_manager import UserManager
from config import Config

logging.basicConfig(level=logging.INFO, format='%(asctime)s - %(levelname)s - %(message)s')

class UserAdmin:
    def __init__(self):
        self.user_manager = UserManager()
        self.db_path = Config.DB_PATH
    
    def list_all_users(self):
        """列出所有用戶"""
        try:
            users = self.user_manager.get_all_users()
            if not users:
                print("資料庫中沒有用戶")
                return
            
            print(f"\n{'='*60}")
            print(f"{'ID':<5} {'用戶名':<15} {'餘額':<10} {'信用分數':<10}")
            print(f"{'='*60}")
            
            for user in users:
                print(f"{user['id']:<5} {user['username']:<15} {user['tokens']:<10} {user['credit_score']:<10}")
            
            print(f"{'='*60}")
            print(f"總共 {len(users)} 個用戶")
            
        except Exception as e:
            print(f"列出用戶失敗: {e}")
    
    def delete_user_by_id(self, user_id):
        """根據用戶ID刪除用戶"""
        try:
            # 先查詢用戶是否存在
            user = self.user_manager.query_one("SELECT id, username, tokens FROM users WHERE id = ?", (user_id,))
            if not user:
                print(f"用戶ID {user_id} 不存在")
                return False
            
            username = user['username']
            tokens = user['tokens']
            
            # 確認刪除
            print(f"即將刪除用戶:")
            print(f"  ID: {user_id}")
            print(f"  用戶名: {username}")
            print(f"  餘額: {tokens} CPT")
            
            confirm = input("\n確認刪除? (輸入 'YES' 確認): ").strip()
            if confirm != 'YES':
                print("已取消刪除")
                return False
            
            # 執行刪除
            rows_affected = self.user_manager.execute("DELETE FROM users WHERE id = ?", (user_id,))
            
            if rows_affected > 0:
                print(f"✓ 用戶 {username} (ID: {user_id}) 已成功刪除")
                
                # 清理相關的任務數據
                self._cleanup_user_tasks(user_id)
                
                return True
            else:
                print(f"✗ 刪除用戶失敗")
                return False
                
        except Exception as e:
            print(f"刪除用戶失敗: {e}")
            return False
    
    def delete_user_by_name(self, username):
        """根據用戶名刪除用戶"""
        try:
            # 先查詢用戶是否存在
            user = self.user_manager.query_one("SELECT id, username, tokens FROM users WHERE username = ?", (username,))
            if not user:
                print(f"用戶名 '{username}' 不存在")
                return False
            
            return self.delete_user_by_id(user['id'])
            
        except Exception as e:
            print(f"刪除用戶失敗: {e}")
            return False
    
    def _cleanup_user_tasks(self, user_id):
        """清理用戶相關的任務數據"""
        try:
            import redis
            redis_client = redis.Redis(host='localhost', port=6379, db=0, decode_responses=True)
            
            # 查找該用戶的所有任務
            task_keys = redis_client.keys("task:*")
            user_tasks = []
            
            for key in task_keys:
                task_user_id = redis_client.hget(key, "user_id")
                if str(task_user_id) == str(user_id):
                    task_id = key.split(":", 1)[1]
                    user_tasks.append(task_id)
            
            if user_tasks:
                print(f"  發現 {len(user_tasks)} 個相關任務，正在清理...")
                
                for task_id in user_tasks:
                    # 刪除任務數據
                    redis_client.delete(f"task:{task_id}")
                    # 刪除用戶任務集合
                    redis_client.delete(f"user:{user_id}:tasks")
                
                print(f"  ✓ 已清理 {len(user_tasks)} 個任務")
            else:
                print(f"  ✓ 沒有發現相關任務需要清理")
                
        except Exception as e:
            print(f"  ⚠ 清理任務數據失敗: {e}")
    
    def reset_user_password(self, username, new_password):
        """重置用戶密碼"""
        try:
            # 檢查用戶是否存在
            user = self.user_manager.query_one("SELECT id FROM users WHERE username = ?", (username,))
            if not user:
                print(f"用戶名 '{username}' 不存在")
                return False
            
            success, message = self.user_manager.update_user_password(user['id'], new_password)
            
            if success:
                print(f"✓ 用戶 {username} 的密碼已重置")
                return True
            else:
                print(f"✗ 重置密碼失敗: {message}")
                return False
                
        except Exception as e:
            print(f"重置密碼失敗: {e}")
            return False
    
    def update_user_balance(self, username, new_balance):
        """更新用戶餘額"""
        try:
            # 檢查用戶是否存在
            user = self.user_manager.query_one("SELECT id, tokens FROM users WHERE username = ?", (username,))
            if not user:
                print(f"用戶名 '{username}' 不存在")
                return False
            
            old_balance = user['tokens']
            
            rows_affected = self.user_manager.execute(
                "UPDATE users SET tokens = ? WHERE id = ?",
                (new_balance, user['id'])
            )
            
            if rows_affected > 0:
                print(f"✓ 用戶 {username} 餘額已更新: {old_balance} → {new_balance} CPT")
                return True
            else:
                print(f"✗ 更新餘額失敗")
                return False
                
        except Exception as e:
            print(f"更新餘額失敗: {e}")
            return False
    
    def create_user(self, username, password, initial_balance=150):
        """創建新用戶"""
        try:
            success, message = self.user_manager.register_user(username, password)
            
            if success:
                # 設置初始餘額
                if initial_balance != 100:  # 默認是100，如果不同則更新
                    self.update_user_balance(username, initial_balance)
                
                print(f"✓ 用戶 {username} 創建成功，初始餘額: {initial_balance} CPT")
                return True
            else:
                print(f"✗ 創建用戶失敗: {message}")
                return False
                
        except Exception as e:
            print(f"創建用戶失敗: {e}")
            return False
    
    def backup_database(self, backup_path=None):
        """備份資料庫"""
        try:
            if not backup_path:
                import datetime
                timestamp = datetime.datetime.now().strftime("%Y%m%d_%H%M%S")
                backup_path = f"hivemind_backup_{timestamp}.db"
            
            import shutil
            shutil.copy2(self.db_path, backup_path)
            print(f"✓ 資料庫已備份到: {backup_path}")
            return True
            
        except Exception as e:
            print(f"備份資料庫失敗: {e}")
            return False
    
    def reset_database(self, confirm_input=None):
        """重置整個資料庫 - 刪除所有用戶和數據"""
        try:
            # 安全確認
            if confirm_input != "RESET_ALL_DATA":
                print("\n⚠️  警告：此操作將完全刪除所有用戶數據！")
                print("如要繼續，請輸入: RESET_ALL_DATA")
                confirm = input("\n確認輸入: ").strip()
                if confirm != "RESET_ALL_DATA":
                    print("已取消重置操作")
                    return False
            
            print("\n正在重置資料庫...")
            
            # 刪除資料庫文件
            if os.path.exists(self.db_path):
                os.remove(self.db_path)
                print(f"✓ 已刪除舊資料庫: {self.db_path}")
            
            # 重新初始化 UserManager，這會自動創建新的空資料庫
            self.user_manager = UserManager()
            print("✓ 已創建新的空資料庫")
            
            # 顯示結果
            print(f"\n🎉 資料庫重置完成！")
            print(f"   資料庫位置: {self.db_path}")
            print(f"   預設測試用戶: test / password (餘額: 1000 CPT)")
            
            return True
            
        except Exception as e:
            print(f"重置資料庫失敗: {e}")
            return False

    def quick_reset(self):
        """快速重置 - 無需確認（僅限測試環境）"""
        try:
            print("執行快速資料庫重置...")
            
            if os.path.exists(self.db_path):
                os.remove(self.db_path)
                print(f"✓ 已刪除: {self.db_path}")
            
            self.user_manager = UserManager()
            print(f"✓ 資料庫已重置，預設用戶: test/password")
            
            return True
        except Exception as e:
            print(f"快速重置失敗: {e}")
            return False

def show_help():
    """顯示幫助信息"""
    print("""
HiveMind 用戶管理工具

用法:
    python user_admin.py <命令> [參數]

命令:
    list                        - 列出所有用戶
    delete-id <user_id>         - 根據用戶ID刪除用戶
    delete-name <username>      - 根據用戶名刪除用戶
    reset-password <username> <new_password>  - 重置用戶密碼
    update-balance <username> <new_balance>   - 更新用戶餘額
    create <username> <password> [balance]    - 創建新用戶
    reset-database              - 重置整個資料庫（刪除所有數據）
    quick-reset                 - 快速重置資料庫（無確認，測試用）
    cleanup-sessions            - 檢查用戶活躍狀態
    backup [path]               - 備份資料庫
    help                        - 顯示此幫助信息

示例:
    python user_admin.py list
    python user_admin.py delete-name testuser
    python user_admin.py reset-database
    python user_admin.py quick-reset
    python user_admin.py create newuser password123 200
""")

def main():
    if len(sys.argv) < 2:
        show_help()
        return
    
    command = sys.argv[1].lower()
    admin = UserAdmin()
    
    try:
        if command == 'list':
            admin.list_all_users()
            
        elif command == 'reset-database':
            admin.reset_database()
            
        elif command == 'quick-reset':
            admin.quick_reset()
            
        elif command == 'cleanup-sessions':
            admin.cleanup_expired_sessions()

        elif command == 'delete-id':
            if len(sys.argv) < 3:
                print("錯誤: 請提供用戶ID")
                print("用法: python user_admin.py delete-id <user_id>")
                return
            
            try:
                user_id = int(sys.argv[2])
                admin.delete_user_by_id(user_id)
            except ValueError:
                print("錯誤: 用戶ID必須是數字")
                
        elif command == 'delete-name':
            if len(sys.argv) < 3:
                print("錯誤: 請提供用戶名")
                print("用法: python user_admin.py delete-name <username>")
                return
            
            username = sys.argv[2]
            admin.delete_user_by_name(username)
            
        elif command == 'reset-password':
            if len(sys.argv) < 4:
                print("錯誤: 請提供用戶名和新密碼")
                print("用法: python user_admin.py reset-password <username> <new_password>")
                return
            
            username = sys.argv[2]
            new_password = sys.argv[3]
            admin.reset_user_password(username, new_password)
            
        elif command == 'update-balance':
            if len(sys.argv) < 4:
                print("錯誤: 請提供用戶名和新餘額")
                print("用法: python user_admin.py update-balance <username> <new_balance>")
                return
            
            username = sys.argv[2]
            try:
                new_balance = int(sys.argv[3])
                admin.update_user_balance(username, new_balance)
            except ValueError:
                print("錯誤: 餘額必須是數字")
                
        elif command == 'create':
            if len(sys.argv) < 4:
                print("錯誤: 請提供用戶名和密碼")
                print("用法: python user_admin.py create <username> <password> [balance]")
                return
            
            username = sys.argv[2]
            password = sys.argv[3]
            initial_balance = 150  # 默認值
            
            if len(sys.argv) >= 5:
                try:
                    initial_balance = int(sys.argv[4])
                except ValueError:
                    print("錯誤: 初始餘額必須是數字，使用默認值 150")
            
            admin.create_user(username, password, initial_balance)
            
        elif command == 'backup':
            backup_path = sys.argv[2] if len(sys.argv) >= 3 else None
            admin.backup_database(backup_path)
            
        elif command == 'help':
            show_help()
            
        else:
            print(f"錯誤: 未知命令 '{command}'")
            show_help()
            
    except KeyboardInterrupt:
        print("\n操作已取消")
    except Exception as e:
        print(f"執行命令時發生錯誤: {e}")

if __name__ == "__main__":
    main()
