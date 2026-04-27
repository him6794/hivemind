import sqlite3
import logging
import threading
import os
import time
from contextlib import contextmanager
from config import Config
import resend
import secrets
from urllib.parse import urljoin

logging.basicConfig(level=logging.INFO, format='%(asctime)s - %(levelname)s - %(message)s')

class DatabaseManager:
    def __init__(self):
        self.db_path = Config.get_database_url()
        # 自動建立資料夾
        db_dir = os.path.dirname(self.db_path)
        if db_dir and not os.path.exists(db_dir):
            try:
                os.makedirs(db_dir, exist_ok=True)
                logging.info(f"自動建立資料庫資料夾: {db_dir}")
            except Exception as e:
                logging.critical(f"無法建立資料庫資料夾 {db_dir}: {e}")
                raise
        self._lock = threading.Lock()
        self._init_db()
        
        # 初始化 Resend 客戶端
        if Config.RESEND_API_KEY and not Config.SKIP_EMAIL_VERIFICATION:
            resend.api_key = Config.RESEND_API_KEY
            self.email_enabled = True
            logging.info("Resend 電子郵件服務已初始化")
        else:
            self.email_enabled = False
            if Config.SKIP_EMAIL_VERIFICATION:
                logging.info("電子郵件驗證已跳過（開發模式）")
            else:
                logging.warning("未設定 RESEND_API_KEY，電子郵件功能將被禁用")

    @contextmanager
    def _db_connection(self):
        """資料庫連線上下文管理器，自動處理提交和回滾"""
        conn = sqlite3.connect(self.db_path, timeout=10.0)
        conn.row_factory = sqlite3.Row
        cursor = conn.cursor()
        try:
            yield cursor
            conn.commit()
            logging.debug("資料庫事務提交成功")
        except Exception as e:
            conn.rollback()
            logging.error(f"資料庫事務回滾: {e}", exc_info=True)
            raise
        finally:
            conn.close()
            logging.debug("資料庫連線關閉")

    def _init_db(self):
        """初始化資料庫表結構"""
        logging.info("初始化資料庫...")
        try:
            with self._db_connection() as cursor:
                # 用戶表 - 新增電子郵件相關欄位和獎勵追蹤欄位
                cursor.execute("""
                    CREATE TABLE IF NOT EXISTS users (
                        id INTEGER PRIMARY KEY AUTOINCREMENT,
                        username TEXT UNIQUE NOT NULL,
                        password TEXT NOT NULL,
                        tokens INTEGER DEFAULT 0 CHECK(tokens >= 0),
                        credit_score INTEGER DEFAULT 100,
                        email TEXT DEFAULT NULL,
                        email_verified INTEGER DEFAULT 0,
                        email_verification_token TEXT DEFAULT NULL,
                        email_verification_expires INTEGER DEFAULT NULL,
                        email_reward_claimed INTEGER DEFAULT 0,
                        password_reset_token TEXT DEFAULT NULL,
                        password_reset_expires INTEGER DEFAULT NULL,
                        created_at INTEGER DEFAULT (strftime('%s', 'now')),
                        updated_at INTEGER DEFAULT (strftime('%s', 'now'))
                    )
                """)
                
                # 檢查是否需要添加新欄位（為現有資料庫升級）
                cursor.execute("PRAGMA table_info(users)")
                columns = [column[1] for column in cursor.fetchall()]
                
                if 'email_reward_claimed' not in columns:
                    cursor.execute("ALTER TABLE users ADD COLUMN email_reward_claimed INTEGER DEFAULT 0")
                    logging.info("添加 email_reward_claimed 欄位到現有資料庫")
                
                if 'password_reset_token' not in columns:
                    cursor.execute("ALTER TABLE users ADD COLUMN password_reset_token TEXT DEFAULT NULL")
                    logging.info("添加 password_reset_token 欄位到現有資料庫")
                
                if 'password_reset_expires' not in columns:
                    cursor.execute("ALTER TABLE users ADD COLUMN password_reset_expires INTEGER DEFAULT NULL")
                    logging.info("添加 password_reset_expires 欄位到現有資料庫")
                
                # 創建索引提高查詢效率
                cursor.execute("CREATE INDEX IF NOT EXISTS idx_users_username ON users(username)")
                cursor.execute("CREATE INDEX IF NOT EXISTS idx_users_email ON users(email)")
                cursor.execute("CREATE INDEX IF NOT EXISTS idx_users_email_token ON users(email_verification_token)")
                
                logging.info("資料庫初始化完成")
        except Exception as e:
            logging.error(f"資料庫初始化失敗: {e}", exc_info=True)
            raise

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
            raise

    def query_one(self, query: str, params: tuple = ()):
        """執行資料庫查詢操作，返回單條記錄 (字典形式)"""
        logging.debug(f"查詢資料庫單條記錄: {query} with params {params}")
        try:
            with self._db_connection() as cursor:
                cursor.execute(query, params)
                result = cursor.fetchone()
                logging.debug(f"查詢結果: {result}")
                # 確保返回字典格式
                return dict(result) if result else None
        except Exception as e:
            logging.error(f"Database query error: {e}")
            raise

    def query_all(self, query: str, params: tuple = ()):
        """執行資料庫查詢操作，返回所有記錄"""
        logging.debug(f"查詢資料庫所有記錄: {query} with params {params}")
        try:
            with self._db_connection() as cursor:
                cursor.execute(query, params)
                results = cursor.fetchall()
                logging.debug(f"查詢結果數量: {len(results)}")
                return [dict(row) for row in results]
        except Exception as e:
            raise

    # === 用戶相關操作 ===
    def create_user(self, username: str, password: str):
        """創建用戶 - 新用戶初始 tokens 為 0"""
        try:
            rows = self.execute(
                "INSERT INTO users (username, password, tokens) VALUES (?, ?, 0)",
                (username, password)
            )
            return rows > 0
        except sqlite3.IntegrityError:
            return False

    def get_user_by_username(self, username: str):
        """根據用戶名獲取用戶信息"""
        result = self.query_one("SELECT * FROM users WHERE username = ?", (username,))
        return result

    def get_user_by_id(self, user_id: int):
        """根據用戶ID獲取用戶信息"""
        result = self.query_one("SELECT * FROM users WHERE id = ?", (user_id,))
        return result

    def get_user_by_email(self, email: str):
        """根據電子郵件獲取用戶信息"""
        result = self.query_one("SELECT * FROM users WHERE email = ?", (email,))
        return result

    def update_user_password(self, user_id: int, new_password: str):
        """更新用戶密碼"""
        rows = self.execute(
            "UPDATE users SET password = ?, updated_at = strftime('%s', 'now') WHERE id = ?",
            (new_password, user_id)
        )
        return rows > 0

    def update_user_email(self, user_id: int, email: str, verification_token: str, expires_at: int):
        """更新用戶電子郵件和驗證token"""
        rows = self.execute(
            """UPDATE users SET 
               email = ?, 
               email_verified = 0, 
               email_verification_token = ?, 
               email_verification_expires = ?,
               updated_at = strftime('%s', 'now')
               WHERE id = ?""",
            (email, verification_token, expires_at, user_id)
        )
        return rows > 0

    def verify_user_email(self, verification_token: str):
        """驗證用戶電子郵件，只有第一次驗證才發放獎勵代幣"""
        try:
            with self._db_connection() as cursor:
                # 檢查驗證token是否有效
                user = cursor.execute(
                    """SELECT id, username, email, email_verified, email_verification_expires, email_reward_claimed
                       FROM users 
                       WHERE email_verification_token = ? AND email IS NOT NULL""",
                    (verification_token,)
                ).fetchone()
                
                if not user:
                    return False, "Invalid verification link"
                
                # 檢查是否已經驗證過
                if user['email_verified']:
                    return False, "Email has already been verified"
                
                # 檢查是否過期
                current_time = int(time.time())
                if user['email_verification_expires'] < current_time:
                    return False, "Verification link has expired"
                
                # 檢查是否已經領取過獎勵
                reward_amount = 0
                reward_message = ""
                
                if not user['email_reward_claimed']:
                    # 第一次驗證，給予100 CPT獎勵
                    reward_amount = 100
                    reward_message = " You have received 100 CPT reward tokens."
                    
                    # 更新驗證狀態、發放獎勵並標記已領取獎勵
                    cursor.execute(
                        """UPDATE users SET 
                           email_verified = 1, 
                           tokens = tokens + ?,
                           email_reward_claimed = 1,
                           email_verification_token = NULL,
                           email_verification_expires = NULL,
                           updated_at = strftime('%s', 'now')
                           WHERE id = ? AND email_verified = 0""",
                        (reward_amount, user['id'])
                    )
                    
                    logging.info(f"First email verification for user {user['username']} (ID: {user['id']}), awarded {reward_amount} CPT")
                else:
                    # 不是第一次驗證（換了新的電子郵件），不給獎勵
                    reward_message = " Email verified successfully (no additional reward for subsequent verifications)."
                    
                    # 只更新驗證狀態，不發放獎勵
                    cursor.execute(
                        """UPDATE users SET 
                           email_verified = 1, 
                           email_verification_token = NULL,
                           email_verification_expires = NULL,
                           updated_at = strftime('%s', 'now')
                           WHERE id = ? AND email_verified = 0""",
                        (user['id'],)
                    )
                    
                    logging.info(f"Subsequent email verification for user {user['username']} (ID: {user['id']}), no reward given")
                
                # 檢查是否成功更新
                if cursor.rowcount > 0:
                    return True, f"Email verification successful!{reward_message}"
                else:
                    # 如果沒有更新任何行，說明可能已經被驗證了
                    return False, "Email has already been verified"
                
        except Exception as e:
            logging.error(f"Email verification failed: {e}")
            return False, f"Verification failed: {str(e)}"

    def get_user_balance(self, user_id: int):
        """獲取用戶餘額"""
        user = self.get_user_by_id(user_id)
        return user['tokens'] if user else 0

    def get_user_credit_score(self, username: str):
        """獲取用戶信用評分"""
        user = self.get_user_by_username(username)
        return user['credit_score'] if user else 100

    def update_user_credit_score(self, user_identifier, new_credit_score: int):
        """更新用戶信用評分"""
        if new_credit_score < 0 or new_credit_score > 1000:
            return False, "信用評分必須在 0-1000 之間"
        
        try:
            if isinstance(user_identifier, str) and not user_identifier.isdigit():
                user_row = self.get_user_by_username(user_identifier)
            else:
                user_id = int(user_identifier)
                user_row = self.get_user_by_id(user_id)
            
            if not user_row:
                return False, "用戶不存在"
            
            old_score = user_row["credit_score"]
            rows_affected = self.execute(
                "UPDATE users SET credit_score = ?, updated_at = strftime('%s', 'now') WHERE id = ?", 
                (new_credit_score, user_row["id"])
            )
            
            if rows_affected > 0:
                logging.info(f"用戶 {user_row['username']} (ID: {user_row['id']}) 信用評分已更新: {old_score} → {new_credit_score}")
                return True, f"信用評分已更新: {old_score} → {new_credit_score}"
            else:
                return False, "更新信用評分失敗"
                
        except Exception as e:
            logging.error(f"更新用戶 {user_identifier} 信用評分失敗: {e}", exc_info=True)
            return False, f"操作失敗: {str(e)}"

    def transfer_tokens(self, sender_username: str, receiver_username: str, amount: int):
        """轉帳代幣（事務安全）"""
        if amount <= 0:
            return False, "轉帳金額必須大於 0"
        
        try:
            with self._db_connection() as cursor:
                # 檢查發送方餘額
                sender = cursor.execute("SELECT id, tokens FROM users WHERE username = ?", (sender_username,)).fetchone()
                if not sender:
                    return False, f"發送方用戶 {sender_username} 不存在"
                
                if sender['tokens'] < amount:
                    return False, f"發送方餘額不足，當前餘額: {sender['tokens']}"
                
                # 檢查接收方是否存在
                receiver = cursor.execute("SELECT id FROM users WHERE username = ?", (receiver_username,)).fetchone()
                if not receiver:
                    return False, f"接收方用戶 {receiver_username} 不存在"
                
                # 執行轉帳
                cursor.execute(
                    "UPDATE users SET tokens = tokens - ?, updated_at = strftime('%s', 'now') WHERE username = ?", 
                    (amount, sender_username)
                )
                cursor.execute(
                    "UPDATE users SET tokens = tokens + ?, updated_at = strftime('%s', 'now') WHERE username = ?", 
                    (amount, receiver_username)
                )
                
                logging.info(f"轉帳成功: {sender_username} -> {receiver_username}, 金額: {amount}")
                return True, "轉帳成功"
                
        except Exception as e:
            logging.error(f"轉帳失敗: {e}", exc_info=True)
            return False, f"轉帳失敗: {str(e)}"

    def add_user_balance(self, user_identifier, amount: int):
        """增加用戶餘額"""
        if amount <= 0:
            return False, "金額必須大於0"
        
        try:
            if isinstance(user_identifier, str) and not user_identifier.isdigit():
                user_row = self.get_user_by_username(user_identifier)
            else:
                user_id = int(user_identifier)
                user_row = self.get_user_by_id(user_id)
            
            if not user_row:
                return False, "用戶不存在"
            
            new_balance = user_row["tokens"] + amount
            self.execute(
                "UPDATE users SET tokens = ?, updated_at = strftime('%s', 'now') WHERE id = ?", 
                (new_balance, user_row["id"])
            )
            
            logging.info(f"用戶 {user_row['username']} (ID: {user_row['id']}) 餘額增加 {amount} CPT，新餘額: {new_balance}")
            return True, f"成功增加 {amount} CPT，新餘額: {new_balance}"
            
        except Exception as e:
            logging.error(f"增加用戶餘額失敗: {e}", exc_info=True)
            return False, f"操作失敗: {str(e)}"

    def deduct_user_balance(self, user_identifier, amount: int):
        """扣除用戶餘額"""
        if amount <= 0:
            return False, "金額必須大於0"
        
        try:
            if isinstance(user_identifier, str) and not user_identifier.isdigit():
                user_row = self.get_user_by_username(user_identifier)
            else:
                user_id = int(user_identifier)
                user_row = self.get_user_by_id(user_id)
            
            if not user_row:
                return False, "用戶不存在"
            
            if user_row["tokens"] < amount:
                return False, f"餘額不足，當前餘額: {user_row['tokens']} CPT，需要: {amount} CPT"
            
            new_balance = user_row["tokens"] - amount
            self.execute(
                "UPDATE users SET tokens = ?, updated_at = strftime('%s', 'now') WHERE id = ?", 
                (new_balance, user_row["id"])
            )
            
            logging.info(f"用戶 {user_row['username']} (ID: {user_row['id']}) 餘額扣除 {amount} CPT，新餘額: {new_balance}")
            return True, f"成功扣除 {amount} CPT，新餘額: {new_balance}"
            
        except Exception as e:
            logging.error(f"扣除用戶餘額失敗: {e}", exc_info=True)
            return False, f"操作失敗: {str(e)}"

    def get_all_users(self):
        """獲取所有用戶信息"""
        try:
            return self.query_all("""
                SELECT id, username, tokens, credit_score, email, email_verified, created_at, updated_at
                FROM users
                ORDER BY created_at DESC
            """)
        except Exception as e:
            logging.error(f"獲取所有用戶失敗: {e}", exc_info=True)
            return []

    def check_user_balance_sufficient(self, user_identifier, required_amount: int):
        """檢查用戶餘額是否足夠（支持 user_id 或 username）"""
        try:
            # 支持傳入 user_id (int/str) 或 username (str)
            if isinstance(user_identifier, str) and not user_identifier.isdigit():
                user = self.get_user_by_username(user_identifier)
            else:
                user_id = int(user_identifier)
                user = self.get_user_by_id(user_id)
            
            if user:
                has_sufficient = user['tokens'] >= required_amount
                logging.debug(f"檢查用戶 {user_identifier} 餘額: {user['tokens']} CPT, 需要: {required_amount} CPT, 足夠: {has_sufficient}")
                return has_sufficient
            logging.warning(f"用戶 {user_identifier} 不存在，餘額檢查失敗")
            return False
        except Exception as e:
            logging.error(f"檢查用戶 {user_identifier} 餘額失敗: {e}")
            return False

    def update_user_email_and_send_verification(self, user_id: int, email: str):
        """更新用戶電子郵件並發送驗證郵件"""
        try:
            # 生成驗證 token
            verification_token = secrets.token_urlsafe(32)
            expires_at = int(time.time()) + (24 * 60 * 60)  # 24小時後過期
            
            # 更新數據庫
            success = self.update_user_email(user_id, email, verification_token, expires_at)
            if not success:
                return False, "Failed to update email in database"
            
            # 發送驗證郵件
            if self.email_enabled:
                email_sent = self._send_verification_email(email, verification_token)
                if email_sent:
                    return True, "Email updated successfully. Verification email has been sent, please check your inbox."
                else:
                    # 郵件發送失敗，但數據庫已更新，允許用戶重試發送
                    return False, "Email updated but verification email failed to send. Please check your email address and try again."
            else:
                if Config.SKIP_EMAIL_VERIFICATION:
                    # 開發模式下跳過郵件發送
                    return True, "Email updated successfully (development mode - email verification skipped)."
                else:
                    return False, "Email service is not configured. Please contact administrator."
                
        except Exception as e:
            logging.error(f"Failed to update email and send verification: {e}")
            return False, f"Operation failed: {str(e)}"

    def _send_verification_email(self, email: str, verification_token: str):
        """發送電子郵件驗證郵件（英文版）"""
        try:
            if not self.email_enabled:
                if Config.SKIP_EMAIL_VERIFICATION:
                    logging.info("Development mode: Skipping email sending")
                    return True  # 開發模式下假裝發送成功
                else:
                    logging.warning("Email service not enabled")
                    return False
            
            # 構建驗證連結 - 確保 BASE_URL 正確格式
            base_url = Config.BASE_URL.rstrip('/')  # 移除末尾斜杠
            verification_url = f"{base_url}/verify-email/{verification_token}"
            
            # 電子郵件內容（英文版）
            html_content = f"""
            <!DOCTYPE html>
            <html>
            <head>
                <meta charset="UTF-8">
                <title>HiveMind Email Verification</title>
                <style>
                    body {{ font-family: Arial, sans-serif; line-height: 1.6; color: #333; }}
                    .container {{ max-width: 600px; margin: 0 auto; padding: 20px; }}
                    .header {{ background: linear-gradient(135deg, #667eea 0%, #764ba2 100%); color: white; padding: 30px; text-align: center; border-radius: 8px 8px 0 0; }}
                    .content {{ background: #f9f9f9; padding: 30px; border-radius: 0 0 8px 8px; }}
                    .button {{ display: inline-block; background: #667eea; color: white; padding: 12px 30px; text-decoration: none; border-radius: 5px; margin: 20px 0; }}
                    .footer {{ text-align: center; margin-top: 20px; font-size: 12px; color: #666; }}
                </style>
            </head>
            <body>
                <div class="container">
                    <div class="header">
                        <h1> Welcome to HiveMind!</h1>
                    </div>
                    <div class="content">
                        <h2>Verify Your Email Address</h2>
                        <p>Thank you for joining the HiveMind distributed computing platform!</p>
                        <p>Please click the button below to verify your email address. After verification, you will receive <strong>100 CPT</strong> reward tokens:</p>
                        
                        <div style="text-align: center;">
                            <a href="{verification_url}" class="button">🔐 Verify Email</a>
                        </div>
                        
                        <p>If the button doesn't work, please copy and paste the following link into your browser:</p>
                        <p style="word-break: break-all; background: #e9e9e9; padding: 10px; border-radius: 4px;">
                            {verification_url}
                        </p>
                        
                        <div style="background: #e7f3ff; border-left: 4px solid #2196F3; padding: 15px; margin: 20px 0;">
                            <strong>💡 Important:</strong>
                            <ul>
                                <li>This verification link will expire in 24 hours</li>
                                <li>You will receive 100 CPT tokens immediately after verification</li>
                                <li>Tokens can be used for distributed computing tasks</li>
                            </ul>
                        </div>
                        
                        <p>If you didn't create a HiveMind account, please ignore this email.</p>
                    </div>
                    <div class="footer">
                        <p>This email was sent automatically by the HiveMind system. Please do not reply.</p>
                        <p>© 2025 HiveMind Project. All rights reserved.</p>
                    </div>
                </div>
            </body>
            </html>
            """
            
            text_content = f"""
            Welcome to HiveMind!
            
            Please verify your email address to complete registration and receive 100 CPT reward tokens.
            
            Verification link: {verification_url}
            
            This link will expire in 24 hours.
            
            If you didn't create a HiveMind account, please ignore this email.
            
            © 2025 HiveMind Project
            """
            
            # 發送郵件
            params = {
                "from": f"HiveMind <{Config.FROM_EMAIL}>",
                "to": [email],
                "subject": " HiveMind Email Verification - Get 100 CPT Tokens!",
                "html": html_content,
                "text": text_content,
                "headers": {
                    "X-Entity-Ref-ID": f"hivemind-verification-{int(time.time())}",
                }
            }
            
            response = resend.Emails.send(params)
            
            if response and response.get('id'):
                logging.info(f"Verification email sent successfully to {email}, email ID: {response['id']}")
                return True
            else:
                logging.error(f"Failed to send verification email to {email}, response: {response}")
                return False
                
        except Exception as e:
            logging.error(f"Error occurred while sending verification email to {email}: {e}")
            return False

    def resend_verification_email(self, user_id: int):
        """重新發送驗證郵件"""
        try:
            user = self.get_user_by_id(user_id)
            if not user:
                return False, "用戶不存在"
            
            if user.get('email_verified'):
                return False, "電子郵件已經驗證過了"
            
            if not user.get('email'):
                return False, "用戶尚未設定電子郵件"
            
            # 生成新的驗證 token
            verification_token = secrets.token_urlsafe(32)
            expires_at = int(time.time()) + (24 * 60 * 60)
            
            # 更新驗證 token
            rows = self.execute(
                """UPDATE users SET 
                   email_verification_token = ?, 
                   email_verification_expires = ?,
                   updated_at = strftime('%s', 'now')
                   WHERE id = ?""",
                (verification_token, expires_at, user_id)
            )
            
            if rows > 0:
                # 發送驗證郵件
                if self.email_enabled:
                    email_sent = self._send_verification_email(user['email'], verification_token)
                    if email_sent:
                        return True, "驗證郵件已重新發送"
                    else:
                        return False, "驗證郵件發送失敗"
                else:
                    return False, "郵件服務未啟用"
            else:
                return False, "更新驗證 token 失敗"
                
        except Exception as e:
            logging.error(f"重新發送驗證郵件失敗: {e}")
            return False, f"操作失敗: {str(e)}"

    def create_password_reset_token(self, email: str):
        """為用戶創建密碼重設 token"""
        try:
            # 檢查用戶是否存在且已驗證電子郵件
            user = self.get_user_by_email(email)
            if not user:
                return False, "No account found with this email address"
            
            if not user.get('email_verified'):
                return False, "Please verify your email address first"
            
            # 生成重設 token
            reset_token = secrets.token_urlsafe(32)
            expires_at = int(time.time()) + (60 * 60)  # 1小時後過期
            
            # 更新資料庫
            rows = self.execute(
                """UPDATE users SET 
                   password_reset_token = ?, 
                   password_reset_expires = ?,
                   updated_at = strftime('%s', 'now')
                   WHERE email = ?""",
                (reset_token, expires_at, email)
            )
            
            if rows > 0:
                # 發送重設郵件
                if self.email_enabled:
                    email_sent = self._send_password_reset_email(email, reset_token, user['username'])
                    if email_sent:
                        return True, "Password reset email has been sent to your email address"
                    else:
                        return False, "Failed to send password reset email"
                else:
                    return False, "Email service is not configured"
            else:
                return False, "Failed to create password reset token"
                
        except Exception as e:
            logging.error(f"Failed to create password reset token: {e}")
            return False, f"Operation failed: {str(e)}"

    def reset_password_with_token(self, token: str, new_password: str):
        """使用 token 重設密碼"""
        try:
            with self._db_connection() as cursor:
                # 檢查 token 是否有效
                user = cursor.execute(
                    """SELECT id, username, password_reset_expires 
                       FROM users 
                       WHERE password_reset_token = ?""",
                    (token,)
                ).fetchone()
                
                if not user:
                    return False, "Invalid password reset link"
                
                # 檢查是否過期
                current_time = int(time.time())
                if user['password_reset_expires'] < current_time:
                    return False, "Password reset link has expired"
                
                # 加密新密碼
                import bcrypt
                hashed_password = bcrypt.hashpw(new_password.encode(), bcrypt.gensalt()).decode()
                
                # 更新密碼並清除重設 token
                cursor.execute(
                    """UPDATE users SET 
                       password = ?, 
                       password_reset_token = NULL,
                       password_reset_expires = NULL,
                       updated_at = strftime('%s', 'now')
                       WHERE id = ?""",
                    (hashed_password, user['id'])
                )
                
                logging.info(f"Password reset successful for user {user['username']} (ID: {user['id']})")
                return True, "Password has been reset successfully"
                
        except Exception as e:
            logging.error(f"Password reset failed: {e}")
            return False, f"Password reset failed: {str(e)}"

    def _send_password_reset_email(self, email: str, reset_token: str, username: str):
        """發送密碼重設郵件"""
        try:
            if not self.email_enabled:
                if Config.SKIP_EMAIL_VERIFICATION:
                    logging.info("Development mode: Skipping password reset email sending")
                    return True
                else:
                    logging.warning("Email service not enabled")
                    return False
            
            # 構建重設連結 - 確保 BASE_URL 正確格式
            base_url = Config.BASE_URL.rstrip('/')  # 移除末尾斜杠
            reset_url = f"{base_url}/reset-password/{reset_token}"
            
            # 電子郵件內容（英文版）
            html_content = f"""
            <!DOCTYPE html>
            <html>
            <head>
                <meta charset="UTF-8">
                <title>HiveMind Password Reset</title>
                <style>
                    body {{ font-family: Arial, sans-serif; line-height: 1.6; color: #333; }}
                    .container {{ max-width: 600px; margin: 0 auto; padding: 20px; }}
                    .header {{ background: linear-gradient(135deg, #667eea 0%, #764ba2 100%); color: white; padding: 30px; text-align: center; border-radius: 8px 8px 0 0; }}
                    .content {{ background: #f9f9f9; padding: 30px; border-radius: 0 0 8px 8px; }}
                    .button {{ display: inline-block; background: #dc3545; color: white; padding: 12px 30px; text-decoration: none; border-radius: 5px; margin: 20px 0; }}
                    .footer {{ text-align: center; margin-top: 20px; font-size: 12px; color: #666; }}
                </style>
            </head>
            <body>
                <div class="container">
                    <div class="header">
                        <h1>🔐 Password Reset Request</h1>
                    </div>
                    <div class="content">
                        <h2>Reset Your Password</h2>
                        <p>Hello {username},</p>
                        <p>We received a request to reset your password for your HiveMind account.</p>
                        <p>Click the button below to reset your password:</p>
                        
                        <div style="text-align: center;">
                            <a href="{reset_url}" class="button">🔑 Reset Password</a>
                        </div>
                        
                        <p>If the button doesn't work, please copy and paste the following link into your browser:</p>
                        <p style="word-break: break-all; background: #e9e9e9; padding: 10px; border-radius: 4px;">
                            {reset_url}
                        </p>
                        
                        <div style="background: #fff3cd; border-left: 4px solid #ffc107; padding: 15px; margin: 20px 0;">
                            <strong>⚠️ Important:</strong>
                            <ul>
                                <li>This password reset link will expire in 1 hour</li>
                                <li>If you didn't request this reset, please ignore this email</li>
                                <li>Never share this link with anyone</li>
                            </ul>
                        </div>
                        
                        <p>If you didn't request a password reset, please ignore this email and your password will remain unchanged.</p>
                    </div>
                    <div class="footer">
                        <p>This email was sent automatically by the HiveMind system. Please do not reply.</p>
                        <p>© 2025 HiveMind Project. All rights reserved.</p>
                    </div>
                </div>
            </body>
            </html>
            """
            
            text_content = f"""
            Password Reset Request - HiveMind
            
            Hello {username},
            
            We received a request to reset your password for your HiveMind account.
            
            Reset link: {reset_url}
            
            This link will expire in 1 hour.
            
            If you didn't request a password reset, please ignore this email.
            
            © 2025 HiveMind Project
            """
            
            # 發送郵件
            params = {
                "from": f"HiveMind <{Config.FROM_EMAIL}>",
                "to": [email],
                "subject": "🔐 HiveMind Password Reset Request",
                "html": html_content,
                "text": text_content,
                "headers": {
                    "X-Entity-Ref-ID": f"hivemind-password-reset-{int(time.time())}",
                }
            }
            
            response = resend.Emails.send(params)
            
            if response and response.get('id'):
                logging.info(f"Password reset email sent successfully to {email}, email ID: {response['id']}")
                return True
            else:
                logging.error(f"Failed to send password reset email to {email}, response: {response}")
                return False
                
        except Exception as e:
            logging.error(f"Error occurred while sending password reset email to {email}: {e}")
            return False


if __name__=='__main__':
    db_manager = DatabaseManager()
    # For testing purposes
    print(db_manager.add_user_balance("justin", 99999999))