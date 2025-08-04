# node_pool/user_manager.py
import sqlite3
import logging
import bcrypt
import jwt
import datetime
import threading
import os
from contextlib import contextmanager
from config import Config  # 導入 Config 類

# 使用 Config.SECRET_KEY
SECRET_KEY = Config.SECRET_KEY

logging.basicConfig(level=logging.INFO, format='%(asctime)s - %(levelname)s - %(message)s')

# 使用 config.py 中的配置
TOKEN_EXPIRY_MINUTES = Config.TOKEN_EXPIRY

class UserManager:
    def __init__(self):
        # 使用 Config.DB_PATH
        self.db_path = Config.DB_PATH
        # 新增：自動建立資料夾
        db_dir = os.path.dirname(self.db_path)
        if db_dir and not os.path.exists(db_dir):
            try:
                os.makedirs(db_dir, exist_ok=True)
                logging.info(f"自動建立資料庫資料夾: {db_dir}")
            except Exception as e:
                logging.critical(f"無法建立資料庫資料夾 {db_dir}: {e}")
                raise
        self._lock = threading.Lock() # 用於保護資料庫連接創建（雖然 sqlite 連接本身有線程安全機制，但顯式鎖定更安全）
        self._init_db()

    @contextmanager
    def _db_connection(self):
        """資料庫連線上下文管理器，自動處理提交和回滾"""
        # timeout 參數增加等待鎖的時間
        conn = sqlite3.connect(self.db_path, timeout=10.0)
        conn.row_factory = sqlite3.Row
        cursor = conn.cursor()
        try:
            # SQLite 的 BEGIN/COMMIT/ROLLBACK 由 with 語句自動管理
            # 進入 with 塊時隱式開始事務
            yield cursor
            # 如果 with 塊成功執行完畢，隱式提交
            conn.commit()
            logging.debug("資料庫事務提交成功")
        except Exception as e:
            # 如果 with 塊中發生異常，隱式回滾
            conn.rollback()
            logging.error(f"資料庫事務回滾: {e}", exc_info=True) # 記錄詳細錯誤
            raise # 將異常重新拋出，以便上層能捕獲
        finally:
            conn.close()
            logging.debug("資料庫連線關閉")

    def _init_db(self):
        """初始化資料庫表結構並插入測試用戶"""
        logging.info("初始化資料庫...")
        try:
            with self._db_connection() as cursor:
                cursor.execute("""
                    CREATE TABLE IF NOT EXISTS users (
                        id INTEGER PRIMARY KEY AUTOINCREMENT,
                        username TEXT UNIQUE NOT NULL,
                        password TEXT NOT NULL,
                        tokens INTEGER DEFAULT 100 CHECK(tokens >= 0), -- 添加非負約束
                        credit_score INTEGER DEFAULT 100
                    )
                """)
                
                # 插入或忽略測試用戶，避免重複插入錯誤
                hashed_password = bcrypt.hashpw("password".encode(), bcrypt.gensalt()).decode()
                cursor.execute("INSERT OR IGNORE INTO users (username, password, tokens, credit_score) VALUES (?, ?, ?, ?)",
                              ("test", hashed_password, 1000, 100)) # 給測試用戶多點 token
                logging.info("資料庫初始化完成")
        except Exception as e:
            logging.error(f"資料庫初始化失敗: {e}", exc_info=True)
            raise # 初始化失敗是嚴重問題，直接拋出

    def execute(self, query: str, params: tuple = ()):
        """執行資料庫寫操作 (INSERT, UPDATE, DELETE)"""
        logging.debug(f"執行資料庫寫操作: {query} with params {params}")
        try:
            with self._db_connection() as cursor:
                cursor.execute(query, params)
                rows = cursor.rowcount
                logging.debug(f"影響行數: {rows}")
                return rows
        except Exception as e:
             # _db_connection 會處理回滾和日誌
            raise # 重新拋出異常

    def query_one(self, query: str, params: tuple = ()):
        """執行資料庫查詢操作，返回單條記錄 (字典形式)"""
        logging.debug(f"查詢資料庫單條記錄: {query} with params {params}")
        try:
            with self._db_connection() as cursor:
                cursor.execute(query, params)
                result = cursor.fetchone()
                logging.debug(f"查詢結果: {result}")
                # fetchone 返回的是 sqlite3.Row 對象，可以像字典一樣訪問
                return result
        except Exception as e:
            # _db_connection 會處理回滾和日誌
            raise # 重新拋出異常

    def register_user(self, username, password):
        """註冊用戶"""
        if not username or not password:
             return False, "用戶名和密碼不能為空"
        hashed_pw = bcrypt.hashpw(password.encode(), bcrypt.gensalt()).decode()
        try:
            rows = self.execute(
                "INSERT INTO users (username, password) VALUES (?, ?)",
                (username, hashed_pw)
            )
            if rows > 0:
                logging.info(f"用戶 {username} 註冊成功")
                return True, "注册成功"
            # 理論上 execute 失敗會拋異常，但以防萬一
            return False, "注册失败（未知原因）"
        except sqlite3.IntegrityError:
            logging.warning(f"用戶 {username} 註冊失敗：用戶名已存在")
            return False, "用户名已存在"
        except Exception as e:
            logging.error(f"用戶 {username} 註冊時發生錯誤: {e}", exc_info=True)
            return False, "服務器內部錯誤"

    def login_user(self, username, password):
        """登錄用戶，返回 token"""
        if not username or not password:
            return False, "用戶名和密碼不能為空", None
        
        try:
            user = self.query_one("SELECT id, username, password FROM users WHERE username = ?", (username,))
            if user and bcrypt.checkpw(password.encode(), user['password'].encode()):
                # 生成 JWT token
                payload = {
                    'user_id': user['id'],
                    'username': user['username'],
                    'exp': datetime.datetime.now(datetime.timezone.utc) + datetime.timedelta(minutes=TOKEN_EXPIRY_MINUTES)
                }
                token = jwt.encode(payload, SECRET_KEY, algorithm="HS256")
                logging.info(f"用戶 {username} 登錄成功")
                return True, "登錄成功", token
            else:
                logging.warning(f"用戶 {username} 登錄失敗：用戶名或密碼錯誤")
                return False, "用戶名或密碼錯誤", None
        except Exception as e:
            logging.error(f"用戶 {username} 登錄時發生錯誤: {e}", exc_info=True)
            return False, "服務器內部錯誤", None

    def _generate_token(self, user_id):
        """生成 JWT Token"""
        payload = {
            "user_id": user_id,
            "exp": datetime.datetime.now(datetime.timezone.utc) + datetime.timedelta(minutes=TOKEN_EXPIRY_MINUTES)
        }
        return jwt.encode(payload, SECRET_KEY, algorithm="HS256")

    def verify_token(self, token: str) -> int:
        """驗證 JWT Token，返回 user_id 或拋出 ValueError"""
        try:
            payload = jwt.decode(token, SECRET_KEY, algorithms=["HS256"])
            user_id = payload["user_id"]
            # 可選：檢查 token 是否在黑名單中（如果實現了登出功能）
            logging.debug(f"Token 驗證成功: 用戶 ID {user_id}")
            return user_id
        except jwt.ExpiredSignatureError:
            logging.warning("Token 驗證失敗: 令牌已過期")
            raise ValueError("令牌已过期")
        except jwt.InvalidTokenError as e:
            logging.warning(f"Token 驗證失敗: 無效令牌 ({e})")
            raise ValueError("无效令牌")
        except Exception as e:
             logging.error(f"Token 驗證時發生未知錯誤: {e}", exc_info=True)
             raise ValueError("Token 驗證失敗") # 向上層拋出標準化的錯誤

    def get_user_balance(self, user_id):
        """
        獲取用戶的 CPT 餘額
        
        Args:
            user_id: 用戶ID
            
        Returns:
            int: 用戶餘額，或在失敗時返回0
        """
        try:
            # 確保用戶ID有效
            if not user_id:
                logging.warning(f"餘額查詢失敗: 無效的用戶ID: {user_id}")
                return 0
            
            # 轉換用戶ID為整數（如果需要）
            try:
                user_id = int(user_id)
            except ValueError:
                logging.warning(f"餘額查詢失敗: 用戶ID不是有效的整數: {user_id}")
                return 0
            
            user = self.query_one(
                "SELECT tokens FROM users WHERE id = ?",
                (user_id,)
            )
            
            if user:
                balance = user['tokens']
                logging.info(f"用戶 {user_id} 餘額查詢成功: {balance}")
                return balance
            else:
                logging.warning(f"用戶 {user_id} 餘額查詢失敗：用戶不存在")
                return 0
                
        except Exception as e:
            logging.error(f"查詢用戶 {user_id} 餘額時發生錯誤: {e}")
            return 0

    def get_user_credit_score(self, username):
        """獲取用戶信用評分"""
        try:
            user = self.query_one("SELECT credit_score FROM users WHERE username = ?", (username,))
            if user:
                return user['credit_score']
            else:
                logging.warning(f"找不到用戶 {username}，返回默認信用評分 100")
                return 100
        except Exception as e:
            logging.error(f"獲取用戶 {username} 信用評分失敗: {e}")
            return 100

    def update_user_credit_score(self, user_identifier, new_credit_score):
        """
        更新用戶信用評分
        
        Args:
            user_identifier: 用戶名或用戶ID
            new_credit_score: 新的信用評分
            
        Returns:
            tuple: (success: bool, message: str)
        """
        if new_credit_score < 0 or new_credit_score > 1000:
            return False, "信用評分必須在 0-1000 之間"
        
        try:
            # 根據類型決定查詢方式
            if isinstance(user_identifier, str) and not user_identifier.isdigit():
                # 用戶名
                user_row = self.query_one("SELECT id, username, credit_score FROM users WHERE username = ?", (user_identifier,))
            else:
                # 用戶ID
                user_id = int(user_identifier)
                user_row = self.query_one("SELECT id, username, credit_score FROM users WHERE id = ?", (user_id,))
            
            if not user_row:
                return False, "用戶不存在"
            
            old_score = user_row["credit_score"]
            rows_affected = self.execute("UPDATE users SET credit_score = ? WHERE id = ?", (new_credit_score, user_row["id"]))
            
            if rows_affected > 0:
                logging.info(f"用戶 {user_row['username']} (ID: {user_row['id']}) 信用評分已更新: {old_score} → {new_credit_score}")
                return True, f"信用評分已更新: {old_score} → {new_credit_score}"
            else:
                return False, "更新信用評分失敗"
                
        except Exception as e:
            logging.error(f"更新用戶 {user_identifier} 信用評分失敗: {e}", exc_info=True)
            return False, f"操作失敗: {str(e)}"

    def transfer_tokens(self, sender_username, receiver_username, amount):
        """轉帳 tokens（用戶名版本）"""
        if amount <= 0:
            return False, "轉帳金額必須大於 0"
        
        try:
            with self._db_connection() as cursor:
                # 檢查發送方餘額
                sender = cursor.execute("SELECT tokens FROM users WHERE username = ?", (sender_username,)).fetchone()
                if not sender:
                    return False, f"發送方用戶 {sender_username} 不存在"
                
                if sender['tokens'] < amount:
                    return False, f"發送方餘額不足，當前餘額: {sender['tokens']}"
                
                # 檢查接收方是否存在
                receiver = cursor.execute("SELECT id FROM users WHERE username = ?", (receiver_username,)).fetchone()
                if not receiver:
                    return False, f"接收方用戶 {receiver_username} 不存在"
                
                # 執行轉帳
                cursor.execute("UPDATE users SET tokens = tokens - ? WHERE username = ?", (amount, sender_username))
                cursor.execute("UPDATE users SET tokens = tokens + ? WHERE username = ?", (amount, receiver_username))
                
                logging.info(f"轉帳成功: {sender_username} -> {receiver_username}, 金額: {amount}")
                return True, "轉帳成功"
                
        except Exception as e:
            logging.error(f"轉帳失敗: {e}", exc_info=True)
            return False, f"轉帳失敗: {str(e)}"

    def update_user_password(self, user_id, new_password):
        """更新用戶密碼"""
        if not new_password:
             return False, "新密碼不能為空"
        try:
            hashed_pw = bcrypt.hashpw(new_password.encode(), bcrypt.gensalt()).decode()
            rows = self.execute(
                "UPDATE users SET password = ? WHERE id = ?",
                (hashed_pw, user_id)
            )
            if rows > 0:
                logging.info(f"用戶 {user_id} 密碼更新成功")
                return True, "密码更新成功"
            else:
                 # 如果 rows == 0，說明 user_id 不存在
                 logging.warning(f"用戶 {user_id} 密碼更新失敗：用戶不存在")
                 return False, "用户不存在"
        except Exception as e:
             logging.error(f"用戶 {user_id} 更新密碼時發生錯誤: {e}", exc_info=True)
             return False, "服務器內部錯誤"

    def get_all_users(self):
        """获取所有用户信息"""
        try:
            with self._db_connection() as cursor:
                cursor.execute("""
                    SELECT id, username, tokens, credit_score
                    FROM users
                """)
                users = cursor.fetchall()
                return [dict(user) for user in users]
        except Exception as e:
            logging.error(f"获取所有用户失败: {e}", exc_info=True)
            return []

    def add_balance(self, user_identifier, amount):
        """增加用戶餘額"""
        if amount <= 0:
            return False, "金額必須大於0"
        
        try:
            # 根據類型決定查詢方式
            if isinstance(user_identifier, str) and not user_identifier.isdigit():
                # 用戶名
                user_row = self.query_one("SELECT id, username, tokens FROM users WHERE username = ?", (user_identifier,))
            else:
                # 用戶ID
                user_id = int(user_identifier)
                user_row = self.query_one("SELECT id, username, tokens FROM users WHERE id = ?", (user_id,))
            
            if not user_row:
                return False, "用戶不存在"
            
            new_balance = user_row["tokens"] + amount
            self.execute("UPDATE users SET tokens = ? WHERE id = ?", (new_balance, user_row["id"]))
            
            logging.info(f"用戶 {user_row['username']} (ID: {user_row['id']}) 餘額增加 {amount} CPT，新餘額: {new_balance}")
            return True, f"成功增加 {amount} CPT，新餘額: {new_balance}"
            
        except Exception as e:
            logging.error(f"增加用戶餘額失敗: {e}", exc_info=True)
            return False, f"操作失敗: {str(e)}"

    def deduct_balance(self, user_identifier, amount):
        """扣除用戶餘額"""
        if amount <= 0:
            return False, "金額必須大於0"
        
        try:
            # 根據類型決定查詢方式
            if isinstance(user_identifier, str) and not user_identifier.isdigit():
                # 用戶名
                user_row = self.query_one("SELECT id, username, tokens FROM users WHERE username = ?", (user_identifier,))
            else:
                # 用戶ID
                user_id = int(user_identifier)
                user_row = self.query_one("SELECT id, username, tokens FROM users WHERE id = ?", (user_id,))
            
            if not user_row:
                return False, "用戶不存在"
            
            if user_row["tokens"] < amount:
                return False, f"餘額不足，當前餘額: {user_row['tokens']} CPT，需要: {amount} CPT"
            
            new_balance = user_row["tokens"] - amount
            self.execute("UPDATE users SET tokens = ? WHERE id = ?", (new_balance, user_row["id"]))
            
            logging.info(f"用戶 {user_row['username']} (ID: {user_row['id']}) 餘額扣除 {amount} CPT，新餘額: {new_balance}")
            return True, f"成功扣除 {amount} CPT，新餘額: {new_balance}"
            
        except Exception as e:
            logging.error(f"扣除用戶餘額失敗: {e}", exc_info=True)
            return False, f"操作失敗: {str(e)}"
if __name__ == "__main__":
    manager = UserManager()
    manager.register_user('justin', 'password')
