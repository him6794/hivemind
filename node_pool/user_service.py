# node_pool/user_service.py
import grpc
import logging
import bcrypt
import jwt
import datetime
from user_manager import UserManager
import nodepool_pb2
import nodepool_pb2_grpc
from config import Config

SECRET_KEY = Config.SECRET_KEY
logging.basicConfig(level=logging.INFO, format='%(asctime)s - %(levelname)s - %(message)s')

class UserServiceServicer(nodepool_pb2_grpc.UserServiceServicer):
    def __init__(self):
        self.user_manager = UserManager()

    def _verify_token(self, token: str) -> int:
        """驗證 JWT token 並返回用戶 ID"""
        try:
            if not token or token.count('.') != 2:
                raise ValueError("Token 格式不正確")
            payload = jwt.decode(token, SECRET_KEY, algorithms=["HS256"])
            user_id = payload["user_id"]
            logging.debug(f"Token verified successfully for user {user_id}")
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

    def _generate_token(self, user_id):
        """生成 JWT Token"""
        payload = {
            "user_id": user_id,
            "exp": datetime.datetime.now(datetime.timezone.utc) + datetime.timedelta(minutes=60)
        }
        return jwt.encode(payload, SECRET_KEY, algorithm="HS256")

    def Login(self, request, context):
        """用戶登錄"""
        logging.info(f"處理登錄請求: {request.username}")
        try:
            success, message, token = self.user_manager.login_user(request.username, request.password)
            if success:
                logging.info(f"用戶 {request.username} 登錄成功")
                return nodepool_pb2.LoginResponse(success=True, message=message, token=token)
            else:
                logging.warning(f"用戶 {request.username} 登錄失敗: {message}")
                return nodepool_pb2.LoginResponse(success=False, message=message, token="")
        except Exception as e:
            logging.error(f"登錄錯誤: {e}")
            return nodepool_pb2.LoginResponse(success=False, message="服務器內部錯誤", token="")

    def Transfer(self, request, context):
        """用戶轉帳"""
        logging.info(f"處理轉帳請求: 到 {request.receiver_username}, 金額: {request.amount}")
        try:
            user_id = self._verify_token(request.token)
            amount = request.amount
            receiver = request.receiver_username

            # 驗證基本參數
            if amount <= 0:
                logging.warning("轉帳金額無效")
                return nodepool_pb2.TransferResponse(success=False, message="金額必須大於零")
            
            # 獲取發送方用戶名
            sender_user_info = self.user_manager.query_one(
                "SELECT username FROM users WHERE id = ?",
                (user_id,)
            )
            
            if not sender_user_info:
                logging.warning(f"發送方用戶ID {user_id} 不存在")
                return nodepool_pb2.TransferResponse(success=False, message="發送方用戶不存在")
            
            sender_username = sender_user_info["username"]
            
            # 使用 user_manager 的事務安全轉帳方法
            success, message = self.user_manager.transfer_tokens(sender_username, receiver, amount)
            if success:
                logging.info(f"轉帳成功: 從用戶 {sender_username} 到 {receiver}，金額: {amount}")
            else:
                logging.warning(f"轉帳失敗: {message}")
            
            return nodepool_pb2.TransferResponse(success=success, message=message)

        except ValueError as e:
            logging.warning(f"轉帳驗證失敗: {str(e)}")
            return nodepool_pb2.TransferResponse(success=False, message=str(e))
        except Exception as e:
            logging.error(f"轉帳錯誤: {e}", exc_info=True)
            return nodepool_pb2.TransferResponse(success=False, message=f"服務器內部錯誤: {str(e)}")

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
                    message="TOKEN_EXPIRED",
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

    def RefreshToken(self, request, context):
        """刷新用戶令牌"""
        try:
            old_token = request.old_token
            user_id = self._verify_token(old_token)  # 驗證舊 token
            new_token = self._generate_token(user_id)  # 生成新 token
            logging.info(f"用戶 {user_id} Token 刷新成功")
            return nodepool_pb2.RefreshTokenResponse(
                success=True,
                message="Token 刷新成功",
                new_token=new_token
            )
        except ValueError as e:
            logging.warning(f"Token 刷新失敗: {str(e)}")
            return nodepool_pb2.RefreshTokenResponse(
                success=False,
                message=str(e),
                new_token=""
            )
        except Exception as e:
            logging.error(f"Token 刷新失敗: {str(e)}")
            return nodepool_pb2.RefreshTokenResponse(
                success=False,
                message="服務器內部錯誤",
                new_token=""
            )