# node_pool/user_service.py
import sys
import os
sys.path.append(os.path.dirname(__file__))

import grpc
import logging
import sqlite3
import bcrypt
import jwt
import datetime
from user_manager import UserManager
import nodepool_pb2
import nodepool_pb2_grpc
from config import Config

SECRET_KEY = Config.SECRET_KEY

def handle_rpc_errors(func):
    def wrapper(self, request, context):
        try:
            return func(self, request, context)
        except ValueError as e:
            context.set_code(grpc.StatusCode.INVALID_ARGUMENT)
            context.set_details(str(e))
            return func._response_type(success=False, message=str(e))
        except Exception as e:
            logging.exception("Internal server error")
            context.set_code(grpc.StatusCode.INTERNAL)
            context.set_details("Internal server error")
            return func._response_type(success=False, message="Internal error")
    return wrapper


class UserServiceServicer(nodepool_pb2_grpc.UserServiceServicer):
    def __init__(self):
        self.user_manager = UserManager()

    def _verify_token(self, token: str) -> int:
        """驗證 JWT token 並返回用戶 ID"""
        try:
            if not token or token.count('.') != 2:
                raise ValueError("Token 格式不正確")
            logging.debug(f"Verifying token: {token[:10]}...")
            payload = jwt.decode(token, SECRET_KEY, algorithms=["HS256"])
            user_id = payload["user_id"]
            logging.info(f"Token verified successfully for user {user_id}")
            return user_id
        except jwt.ExpiredSignatureError:
            logging.warning("Token expired")
            raise ValueError("令牌已過期")
        except jwt.InvalidTokenError as e:
            logging.warning(f"Invalid token: {str(e)}")
            raise ValueError("無效令牌")
        except Exception as e:
            logging.error(f"Token 驗證錯誤: {e}")
            raise ValueError(f"Token 驗證失敗: {str(e)}")

    def verify_token(self, token):
        """驗證 token 並返回用戶信息"""
        try:
            user_id = self.user_manager.verify_token(token)
            if user_id:
                # 獲取用戶詳細信息
                user_data = self.user_manager.db_manager.get_user_by_id(user_id)
                if user_data:
                    # 確保返回字典格式
                    user_dict = dict(user_data) if user_data else {}
                    return {
                        'user_id': user_dict.get('id'),
                        'username': user_dict.get('username'),
                        'email': user_dict.get('email'),
                        'email_verified': bool(user_dict.get('email_verified', 0))
                    }
            return None
        except Exception as e:
            logging.error(f"Token verification failed: {e}")
            return None

    def verify_token_from_metadata(self, context):
        """從 gRPC metadata 中驗證 token"""
        try:
            metadata = dict(context.invocation_metadata())
            auth_header = metadata.get('authorization', '')
            
            if not auth_header:
                return {
                    "success": False,
                    "message": "Missing authorization header"
                }
            
            if auth_header.startswith('Bearer '):
                token = auth_header[7:]  
            else:
                token = auth_header
            
            user_info = self.verify_token(token)
            if user_info:
                return {
                    "success": True,
                    "user_id": user_info["user_id"],
                    "message": "Token verified successfully"
                }
            else:
                return {
                    "success": False,
                    "message": "Invalid or expired token"
                }
                
        except Exception as e:
            logging.error(f"從 metadata 驗證 token 時發生錯誤: {e}")
            return {
                "success": False,
                "message": f"Token verification error: {str(e)}"
            }

    @handle_rpc_errors
    def Register(self, request, context):
        logging.info(f"處理註冊請求: {request.username}")
        hashed_pw = bcrypt.hashpw(request.password.encode(), bcrypt.gensalt()).decode()
        try:
            if self.user_manager.execute(
                "INSERT INTO users (username, password) VALUES (?, ?)",
                (request.username, hashed_pw)
            ) > 0:
                logging.info(f"用戶 {request.username} 註冊成功")
                return nodepool_pb2.StatusResponse(success=True, message="注册成功")
            return nodepool_pb2.StatusResponse(success=False, message="注册失败")
        except sqlite3.IntegrityError:
            logging.warning(f"用戶 {request.username} 已存在")
            return nodepool_pb2.StatusResponse(success=False, message="用户名已存在")
        except Exception as e:
            logging.error(f"註冊錯誤: {e}")
            return nodepool_pb2.StatusResponse(success=False, message="服務器內部錯誤")

    def Login(self, request, context):
        logging.info(f"處理登錄請求: {request.username}")
        try:
            user = self.user_manager.query_one(
                "SELECT id, password FROM users WHERE username = ?",
                (request.username,)
            )
            if user and bcrypt.checkpw(request.password.encode(), user["password"].encode()):
                token = jwt.encode({
                    "user_id": user["id"],
                    "exp": datetime.datetime.now(datetime.timezone.utc) + datetime.timedelta(minutes=60)
                }, SECRET_KEY, algorithm="HS256")
                logging.info(f"用戶 {request.username} 登錄成功，生成的 token: {token}")
                return nodepool_pb2.LoginResponse(success=True, message="登录成功", token=token)
            logging.warning(f"用戶 {request.username} 登錄失敗: 用户名或密码错误")
            return nodepool_pb2.LoginResponse(success=False, message="用户名或密码错误")
        except Exception as e:
            logging.error(f"登錄錯誤: {e}")
            return nodepool_pb2.LoginResponse(success=False, message="服務器內部錯誤")

    def GetBalance(self, request, context):
        """獲取用戶餘額"""
        try:
            if not request.token:
                return nodepool_pb2.GetBalanceResponse(
                    success=False,
                    message="Token is required",
                    balance=0
                )
            
            user_id = self._verify_token(request.token)
            balance_result = self.user_manager.get_user_balance(user_id)
            
            if balance_result is not None:
                return nodepool_pb2.GetBalanceResponse(
                    success=True,
                    message=f"Balance retrieved successfully for user {user_id}",
                    balance=balance_result
                )
            else:
                return nodepool_pb2.GetBalanceResponse(
                    success=False,
                    message="Failed to get balance",
                    balance=0
                )
        except ValueError as e:
            # Token 過期或無效時的特殊處理
            error_message = str(e)
            if "令牌已過期" in error_message or "已過期" in error_message:
                logging.warning(f"GetBalance: Token expired")
                return nodepool_pb2.GetBalanceResponse(
                    success=False,
                    message="TOKEN_EXPIRED",  # 特殊標記，前端可識別
                    balance=0
                )
            else:
                logging.warning(f"GetBalance: Token validation failed: {error_message}")
                return nodepool_pb2.GetBalanceResponse(
                    success=False,
                    message="INVALID_TOKEN",
                    balance=0
                )
        except Exception as e:
            logging.error(f"GetBalance error: {e}", exc_info=True)
            return nodepool_pb2.GetBalanceResponse(
                success=False,
                message=f"Error: {str(e)}",
                balance=0
            )

    def Transfer(self, request, context):
        logging.info(f"處理轉帳請求: 從用戶 {request.token[:10]}... 到 {request.receiver_username}, 金額: {request.amount}")
        try:
            user_id = self._verify_token(request.token)
            amount = request.amount
            receiver = request.receiver_username

            # 先驗證基本參數
            if amount <= 0:
                logging.warning("轉帳金額無效")
                return nodepool_pb2.StatusResponse(success=False, message="金額必須大於零")
            
            # 獲取發送方用戶名
            sender_user_info = self.user_manager.query_one(
                "SELECT username FROM users WHERE id = ?",
                (user_id,)
            )
            
            if not sender_user_info:
                logging.warning(f"發送方用戶ID {user_id} 不存在")
                return nodepool_pb2.StatusResponse(success=False, message="發送方用戶不存在")
            
            sender_username = sender_user_info["username"]
            
            # 使用 user_manager 的事務安全轉帳方法
            success, message = self.user_manager.transfer_tokens(sender_username, receiver, amount)
            if success:
                logging.info(f"轉帳成功: 從用戶 {sender_username} 到 {receiver}，金額: {amount}")
            else:
                logging.warning(f"轉帳失敗: {message}")
            
            return nodepool_pb2.StatusResponse(success=success, message=message)

        except Exception as e:
            logging.error(f"轉帳錯誤: {e}", exc_info=True)
            return nodepool_pb2.StatusResponse(success=False, message=f"服務器內部錯誤: {str(e)}")

    def UpdatePassword(self, request, context):
        logging.info(f"處理密碼更新請求: 用戶 token {request.token[:10]}...")
        try:
            user_id = self._verify_token(request.token)
            hashed_pw = bcrypt.hashpw(request.new_password.encode(), bcrypt.gensalt()).decode()
            logging.info(f"更新用戶 {user_id} 密碼")
            rows_affected = self.user_manager.execute(
                "UPDATE users SET password = ? WHERE id = ?",
                (hashed_pw, user_id)
            )
            if rows_affected == 0:
                logging.warning(f"用戶 {user_id} 不存在")
                return nodepool_pb2.StatusResponse(success=False, message="用戶不存在")
            logging.info(f"用戶 {user_id} 密碼更新成功")
            return nodepool_pb2.StatusResponse(success=True, message="密码更新成功")
        except Exception as e:
            logging.error(f"密碼更新錯誤: {e}")
            return nodepool_pb2.StatusResponse(success=False, message=f"服務器內部錯誤: {str(e)}")

    def refresh_token(self, request, context):
        try:
            old_token = request.token
            user_id = self._verify_token(old_token)  # 驗證舊 token
            new_token = self._generate_token(user_id)  # 生成新 token
            return nodepool_pb2.LoginResponse(
                success=True,
                message="Token 刷新成功",
                token=new_token
            )
        except Exception as e:
            logging.error(f"Token 刷新失敗: {str(e)}")
            return nodepool_pb2.LoginResponse(
                success=False,
                message="Token 刷新失敗",
                token=""
            )

    def _generate_token(self, user_id):
        """生成 JWT Token"""
        payload = {
            "user_id": user_id,
            "exp": datetime.datetime.now(datetime.timezone.utc) + datetime.timedelta(minutes=60)
        }
        return jwt.encode(payload, SECRET_KEY, algorithm="HS256")