"""
測試餘額檢查功能，確保支持 user_id 和 username
"""
import sys
import os
from pathlib import Path

# 添加 node_pool 到路徑
node_pool_path = str(Path(__file__).parent.parent)
sys.path.insert(0, node_pool_path)

# 必須在導入前設置環境變量避免導入錯誤
os.chdir(node_pool_path)

import pytest
import sqlite3
import tempfile
import logging

# 直接導入，避免 __init__.py 的依賴問題
import importlib.util
spec = importlib.util.spec_from_file_location("database_manager", os.path.join(node_pool_path, "database_manager.py"))
database_manager = importlib.util.module_from_spec(spec)
spec.loader.exec_module(database_manager)
DatabaseManager = database_manager.DatabaseManager

logging.basicConfig(level=logging.DEBUG)


class TestBalanceCheck:
    @pytest.fixture
    def db_manager(self):
        """創建測試用的數據庫管理器"""
        # 使用帶時間戳和隨機ID的臨時文件，確保唯一性
        import time
        import random
        temp_file = f'test_db_{int(time.time()*1000000)}_{random.randint(1000,9999)}.db'
        temp_path = os.path.join(tempfile.gettempdir(), temp_file)
        
        # 直接修改 Config.DB_PATH，避免環境變數的問題
        from config import Config
        original_db_path = Config.DB_PATH
        Config.DB_PATH = temp_path
        
        # 確保文件不存在
        if os.path.exists(temp_path):
            os.unlink(temp_path)
        
        # 創建新的 DatabaseManager 實例，它會使用修改後的 Config.DB_PATH
        db = DatabaseManager()
        
        # 創建測試用戶（不指定 ID，讓數據庫自動分配）
        db.execute("""
            INSERT INTO users (username, password, email, tokens, created_at, updated_at)
            VALUES (?, ?, ?, ?, strftime('%s', 'now'), strftime('%s', 'now'))
        """, ('test_user_100', 'hashed_password', 'test100@example.com', 1198))
        
        db.execute("""
            INSERT INTO users (username, password, email, tokens, created_at, updated_at)
            VALUES (?, ?, ?, ?, strftime('%s', 'now'), strftime('%s', 'now'))
        """, ('test_user_101', 'hashed_password', 'test101@example.com', 50))
        
        # 獲取自動分配的 ID
        user100 = db.get_user_by_username('test_user_100')
        user101 = db.get_user_by_username('test_user_101')
        db._test_user100_id = user100['id']
        db._test_user101_id = user101['id']
        
        yield db
        
        # 恢復原始配置
        Config.DB_PATH = original_db_path
        
        # 清理臨時資料庫
        try:
            os.unlink(temp_path)
        except:
            pass
    
    def test_check_balance_with_username(self, db_manager):
        """測試使用 username 檢查餘額"""
        # test_user_100 有 1198 CPT，需要 1 CPT
        assert db_manager.check_user_balance_sufficient('test_user_100', 1) == True
        
        # test_user_100 有 1198 CPT，需要 1198 CPT
        assert db_manager.check_user_balance_sufficient('test_user_100', 1198) == True
        
        # test_user_100 有 1198 CPT，需要 1199 CPT（不足）
        assert db_manager.check_user_balance_sufficient('test_user_100', 1199) == False
        
        # test_user_101 有 50 CPT，需要 100 CPT（不足）
        assert db_manager.check_user_balance_sufficient('test_user_101', 100) == False
    
    def test_check_balance_with_user_id_int(self, db_manager):
        """測試使用 user_id (int) 檢查餘額"""
        user100_id = db_manager._test_user100_id
        user101_id = db_manager._test_user101_id
        
        # 用戶有 1198 CPT，需要 1 CPT
        assert db_manager.check_user_balance_sufficient(user100_id, 1) == True
        
        # 用戶有 1198 CPT，需要 1198 CPT
        assert db_manager.check_user_balance_sufficient(user100_id, 1198) == True
        
        # 用戶有 1198 CPT，需要 1199 CPT（不足）
        assert db_manager.check_user_balance_sufficient(user100_id, 1199) == False
        
        # 用戶有 50 CPT，需要 100 CPT（不足）
        assert db_manager.check_user_balance_sufficient(user101_id, 100) == False
    
    def test_check_balance_with_user_id_string(self, db_manager):
        """測試使用 user_id (string) 檢查餘額"""
        user100_id = str(db_manager._test_user100_id)
        
        # 用戶有 1198 CPT，需要 1 CPT
        assert db_manager.check_user_balance_sufficient(user100_id, 1) == True
        
        # 用戶有 1198 CPT，需要 1198 CPT
        assert db_manager.check_user_balance_sufficient(user100_id, 1198) == True
        
        # 用戶有 1198 CPT，需要 1199 CPT（不足）
        assert db_manager.check_user_balance_sufficient(user100_id, 1199) == False
    
    def test_check_balance_nonexistent_user(self, db_manager):
        """測試不存在的用戶"""
        assert db_manager.check_user_balance_sufficient('nonexistent', 1) == False
        assert db_manager.check_user_balance_sufficient(999, 1) == False
        assert db_manager.check_user_balance_sufficient("999", 1) == False
    
    def test_get_user_by_id(self, db_manager):
        """測試通過 ID 獲取用戶"""
        user100_id = db_manager._test_user100_id
        user = db_manager.get_user_by_id(user100_id)
        assert user is not None
        assert user['username'] == 'test_user_100'
        assert user['tokens'] == 1198
    
    def test_get_user_by_username(self, db_manager):
        """測試通過 username 獲取用戶"""
        user = db_manager.get_user_by_username('test_user_100')
        assert user is not None
        assert user['id'] == db_manager._test_user100_id
        assert user['tokens'] == 1198
    
    def test_transfer_with_user_id(self, db_manager):
        """測試使用 username 進行轉帳"""
        user100_id = db_manager._test_user100_id
        # test_user_100 (username) 轉 10 CPT 給 test_user_101 (username)
        success, msg = db_manager.transfer_tokens('test_user_100', 'test_user_101', 10)
        assert success == True
        
        # 驗證餘額
        user100 = db_manager.get_user_by_id(user100_id)
        user101 = db_manager.get_user_by_username('test_user_101')
        assert user100['tokens'] == 1188  # 1198 - 10
        assert user101['tokens'] == 60     # 50 + 10
    
    def test_transfer_insufficient_balance(self, db_manager):
        """測試餘額不足的轉帳"""
        # test_user_101 只有 50 CPT，嘗試轉 100 CPT
        success, msg = db_manager.transfer_tokens('test_user_101', 'test_user_100', 100)
        assert success == False
        assert '餘額不足' in msg


if __name__ == '__main__':
    pytest.main([__file__, '-v', '-s'])
