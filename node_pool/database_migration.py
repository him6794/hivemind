import sqlite3
import logging
import os

logging.basicConfig(level=logging.INFO, format='%(asctime)s - %(levelname)s - %(message)s')

def get_database_path():
    """獲取資料庫路徑"""
    try:
        from config import Config
        return Config.get_database_url()
    except:
        # 如果無法導入 Config，使用預設路徑
        db_dir = os.path.join(os.path.dirname(__file__), 'db')
        os.makedirs(db_dir, exist_ok=True)
        return os.path.join(db_dir, 'nodepool.db')

def migrate_database():
    """資料庫遷移腳本 - 從舊版本升級到新版本"""
    
    db_path = get_database_path()
    logging.info(f"資料庫路徑: {db_path}")
    logging.info("開始資料庫遷移...")
    
    try:
        # 直接連接資料庫，不使用 DatabaseManager
        conn = sqlite3.connect(db_path)
        conn.row_factory = sqlite3.Row
        cursor = conn.cursor()
        
        # 檢查現有表結構
        cursor.execute("PRAGMA table_info(users)")
        columns_info = cursor.fetchall()
        columns = [column[1] for column in columns_info]
        logging.info(f"現有欄位: {columns}")
        
        # 需要添加的新欄位（使用常數預設值）
        new_columns = [
            ('email', 'TEXT DEFAULT NULL'),
            ('email_verified', 'INTEGER DEFAULT 0'),
            ('email_verification_token', 'TEXT DEFAULT NULL'),
            ('email_verification_expires', 'INTEGER DEFAULT NULL'),
            ('email_reward_claimed', 'INTEGER DEFAULT 0'),
            ('password_reset_token', 'TEXT DEFAULT NULL'),
            ('password_reset_expires', 'INTEGER DEFAULT NULL'),
            ('updated_at', 'INTEGER DEFAULT NULL')  # 先設為 NULL，稍後再更新
        ]
        
        # 添加缺失的欄位
        for column_name, column_def in new_columns:
            if column_name not in columns:
                alter_sql = f"ALTER TABLE users ADD COLUMN {column_name} {column_def}"
                cursor.execute(alter_sql)
                logging.info(f"已添加欄位: {column_name}")
        
        # 提交欄位添加
        conn.commit()
        
        # 重新檢查表結構（確保欄位已添加）
        cursor.execute("PRAGMA table_info(users)")
        updated_columns = [column[1] for column in cursor.fetchall()]
        logging.info(f"更新後欄位: {updated_columns}")
        
        # 創建索引（只在欄位存在時創建）
        indexes = [
            ("idx_users_username", "username"),
            ("idx_users_email", "email"),
            ("idx_users_email_token", "email_verification_token")
        ]
        
        for index_name, column_name in indexes:
            if column_name in updated_columns:
                index_sql = f"CREATE INDEX IF NOT EXISTS {index_name} ON users({column_name})"
                cursor.execute(index_sql)
                logging.info(f"已創建索引: {index_name}")
            else:
                logging.warning(f"跳過索引 {index_name}，因為欄位 {column_name} 不存在")
        
        # 刪除test用戶（如果存在）
        cursor.execute("SELECT COUNT(*) FROM users WHERE username = 'test'")
        test_user_count = cursor.fetchone()[0]
        
        if test_user_count > 0:
            cursor.execute("DELETE FROM users WHERE username = 'test'")
            logging.info(f"已刪除 {test_user_count} 個test用戶")
        else:
            logging.info("未找到test用戶")
        
        # 檢查是否有 created_at 欄位，如果沒有則添加
        if 'created_at' not in updated_columns:
            cursor.execute("ALTER TABLE users ADD COLUMN created_at INTEGER DEFAULT NULL")
            logging.info("已添加欄位: created_at")
            # 重新獲取更新後的欄位列表
            cursor.execute("PRAGMA table_info(users)")
            updated_columns = [column[1] for column in cursor.fetchall()]
        
        # 設置所有用戶的 created_at 和 updated_at 時間戳
        current_timestamp = int(__import__('time').time())
        
        if 'created_at' in updated_columns:
            cursor.execute("""
                UPDATE users 
                SET created_at = ? 
                WHERE created_at IS NULL
            """, (current_timestamp,))
            created_rows = cursor.rowcount
            if created_rows > 0:
                logging.info(f"已設置 {created_rows} 個用戶的創建時間戳")
        
        if 'updated_at' in updated_columns:
            cursor.execute("""
                UPDATE users 
                SET updated_at = ? 
                WHERE updated_at IS NULL
            """, (current_timestamp,))
            updated_rows = cursor.rowcount
            if updated_rows > 0:
                logging.info(f"已設置 {updated_rows} 個用戶的更新時間戳")
        
        # 顯示遷移後的統計信息
        cursor.execute("SELECT COUNT(*) FROM users")
        total_users = cursor.fetchone()[0]
        
        users_with_email = 0
        verified_users = 0
        
        if 'email' in updated_columns:
            cursor.execute("SELECT COUNT(*) FROM users WHERE email IS NOT NULL")
            users_with_email = cursor.fetchone()[0]
        
        if 'email_verified' in updated_columns:
            cursor.execute("SELECT COUNT(*) FROM users WHERE email_verified = 1")
            verified_users = cursor.fetchone()[0]
        
        # 提交所有更改
        conn.commit()
        
        logging.info("遷移完成統計:")
        logging.info(f"  - 總用戶數: {total_users}")
        logging.info(f"  - 有電子郵件的用戶: {users_with_email}")
        logging.info(f"  - 已驗證電子郵件的用戶: {verified_users}")
        
    except Exception as e:
        logging.error(f"資料庫遷移失敗: {e}")
        if 'conn' in locals():
            conn.rollback()
        raise
    finally:
        if 'conn' in locals():
            conn.close()
    
    logging.info("資料庫遷移成功完成！")

def backup_database():
    """備份現有資料庫"""
    import shutil
    from datetime import datetime
    
    db_path = get_database_path()
    if not os.path.exists(db_path):
        logging.warning("資料庫文件不存在，無需備份")
        return None
    
    timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")
    backup_path = f"{db_path}.backup_{timestamp}"
    
    shutil.copy2(db_path, backup_path)
    logging.info(f"資料庫已備份到: {backup_path}")
    return backup_path

if __name__ == '__main__':
    print("HiveMind 資料庫遷移工具")
    print("=" * 50)
    
    # 詢問用戶是否要備份
    backup_choice = input("是否要先備份現有資料庫？(y/N): ").lower().strip()
    if backup_choice == 'y':
        try:
            backup_path = backup_database()
            if backup_path:
                print(f"✅ 備份完成: {backup_path}")
        except Exception as e:
            print(f"❌ 備份失敗: {e}")
            exit(1)
    
    # 確認遷移
    confirm = input("確定要開始資料庫遷移嗎？(y/N): ").lower().strip()
    if confirm != 'y':
        print("遷移已取消")
        exit(0)
    
    try:
        migrate_database()
        print("✅ 資料庫遷移成功完成！")
    except Exception as e:
        print(f"❌ 遷移失敗: {e}")
        exit(1)
