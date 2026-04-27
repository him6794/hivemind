# node_pool/user_manager.py
import sqlite3
import logging
import bcrypt
import jwt
import datetime
import threading
import os
from contextlib import contextmanager
from config import Config
from database_manager import DatabaseManager

# 使用 Config.SECRET_KEY
SECRET_KEY = Config.SECRET_KEY

logging.basicConfig(level=logging.INFO, format='%(asctime)s - %(levelname)s - %(message)s')

# 使用 config.py 中的配置
TOKEN_EXPIRY_MINUTES = Config.TOKEN_EXPIRY

class UserManager:
    def __init__(self):
        self.db_manager = DatabaseManager()

    def execute(self, query: str, params: tuple = ()):
        """執行資料庫寫操作（委託給數據庫管理器）"""
        return self.db_manager.execute(query, params)

    def query_one(self, query: str, params: tuple = ()):
        """執行資料庫查詢操作（委託給數據庫管理器）"""
        return self.db_manager.query_one(query, params)

    def register_user(self, username, password):
        """註冊用戶 - 新用戶初始代幣為0，需要驗證電子郵件才能獲得100代幣"""
        if not username or not password:
            return False, "用戶名和密碼不能為空"
        
        hashed_pw = bcrypt.hashpw(password.encode(), bcrypt.gensalt()).decode()
        try:
            success = self.db_manager.create_user(username, hashed_pw)
            if success:
                logging.info(f"用戶 {username} 註冊成功，初始代幣: 0")
                return True, "註冊成功，請驗證電子郵件以獲得100 CPT獎勵"
            else:
                logging.warning(f"用戶 {username} 註冊失敗：用戶名已存在")
                return False, "用戶名已存在"
        except Exception as e:
            logging.error(f"用戶 {username} 註冊時發生錯誤: {e}", exc_info=True)
            return False, "服務器內部錯誤"

    def login_user(self, username, password):
        """登錄用戶，返回 token"""
        if not username or not password:
            return False, "用戶名和密碼不能為空", None
        
        try:
            user = self.db_manager.get_user_by_username(username)
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
            raise ValueError("Token 驗證失敗")

    def get_user_balance(self, user_id):
        """獲取用戶的 CPT 餘額"""
        try:
            if not user_id:
                logging.warning(f"餘額查詢失敗: 無效的用戶ID: {user_id}")
                return 0
            
            try:
                user_id = int(user_id)
            except ValueError:
                logging.warning(f"餘額查詢失敗: 用戶ID不是有效的整數: {user_id}")
                return 0
            
            balance = self.db_manager.get_user_balance(user_id)
            logging.info(f"用戶 {user_id} 餘額查詢成功: {balance}")
            return balance
                
        except Exception as e:
            logging.error(f"查詢用戶 {user_id} 餘額時發生錯誤: {e}")
            return 0

    def get_user_credit_score(self, username):
        """獲取用戶信用評分"""
        return self.db_manager.get_user_credit_score(username)

    def update_user_credit_score(self, user_identifier, new_credit_score):
        """更新用戶信用評分"""
        return self.db_manager.update_user_credit_score(user_identifier, new_credit_score)

    def transfer_tokens(self, sender_username, receiver_username, amount):
        """轉帳 tokens"""
        return self.db_manager.transfer_tokens(sender_username, receiver_username, amount)

    def update_user_password(self, user_id, new_password):
        """更新用戶密碼"""
        if not new_password:
            return False, "新密碼不能為空"
        
        try:
            hashed_pw = bcrypt.hashpw(new_password.encode(), bcrypt.gensalt()).decode()
            success = self.db_manager.update_user_password(user_id, hashed_pw)
            if success:
                logging.info(f"用戶 {user_id} 密碼更新成功")
                return True, "密碼更新成功"
            else:
                logging.warning(f"用戶 {user_id} 密碼更新失敗：用戶不存在")
                return False, "用戶不存在"
        except Exception as e:
            logging.error(f"用戶 {user_id} 更新密碼時發生錯誤: {e}", exc_info=True)
            return False, "服務器內部錯誤"

    def get_all_users(self):
        """獲取所有用戶信息"""
        return self.db_manager.get_all_users()

    def add_balance(self, user_identifier, amount):
        """增加用戶餘額"""
        return self.db_manager.add_user_balance(user_identifier, amount)

    def deduct_balance(self, user_identifier, amount):
        """扣除用戶餘額"""
        return self.db_manager.deduct_user_balance(user_identifier, amount)

    def update_user_email(self, user_id, email, verification_token, expires_at):
        """更新用戶電子郵件"""
        return self.db_manager.update_user_email(user_id, email, verification_token, expires_at)

    def verify_user_email(self, verification_token):
        """驗證用戶電子郵件並發放獎勵"""
        return self.db_manager.verify_user_email(verification_token)

    def get_user_by_email(self, email):
        """根據電子郵件獲取用戶"""
        return self.db_manager.get_user_by_email(email)

    def check_user_balance_for_next_payment(self, username, required_amount):
        """檢查用戶餘額是否足夠下次付款"""
        return self.db_manager.check_user_balance_sufficient(username, required_amount)
if __name__ == "__main__":
    manager = UserManager()
    manager.register_user('justin', 'password')
