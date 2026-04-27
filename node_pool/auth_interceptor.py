"""gRPC 認證攔截器 - 統一處理 JWT 驗證"""

import grpc
import jwt
import logging
from config import Config

logging.basicConfig(level=logging.INFO, format='%(asctime)s - %(levelname)s - %(message)s')

# 不需要認證的 RPC 方法白名單
WHITELIST_METHODS = {
    '/nodepool.UserService/Login',
    '/nodepool.UserService/Register',
}

# 需要節點身份的方法（使用 node_id 而非 user_id）
NODE_METHODS = {
    '/nodepool.NodeManagerService/RegisterWorkerNode',
    '/nodepool.NodeManagerService/ReportStatus',
    '/nodepool.WorkerNodeService/TaskOutputUpload',
    '/nodepool.WorkerNodeService/TaskResultUpload',
    '/nodepool.WorkerNodeService/TaskUsage',
}

class AuthInterceptor(grpc.ServerInterceptor):
    """gRPC 伺服器認證攔截器"""
    
    def intercept_service(self, continuation, handler_call_details):
        """攔截所有 RPC 調用進行認證檢查"""
        method = handler_call_details.method
        
        # 白名單方法直接放行
        if method in WHITELIST_METHODS:
            logging.debug(f"白名單方法放行: {method}")
            return continuation(handler_call_details)
        
        # 從 metadata 中提取 token
        metadata = dict(handler_call_details.invocation_metadata)
        auth_header = metadata.get('authorization', '')
        
        if not auth_header:
            logging.warning(f"缺少 authorization header: {method}")
            return self._abort_with_status(
                grpc.StatusCode.UNAUTHENTICATED,
                'Missing authorization header'
            )
        
        # 解析 Bearer token
        if not auth_header.startswith('Bearer '):
            logging.warning(f"Invalid authorization format: {method}")
            return self._abort_with_status(
                grpc.StatusCode.UNAUTHENTICATED,
                'Invalid authorization format'
            )
        
        token = auth_header[7:]  # 移除 "Bearer " 前綴
        
        try:
            # 驗證 JWT token
            payload = jwt.decode(token, Config.SECRET_KEY, algorithms=["HS256"])
            user_id = payload.get("user_id")
            username = payload.get("username")  # 某些 token 可能包含 username
            
            if not user_id:
                logging.warning(f"Token 缺少 user_id: {method}")
                return self._abort_with_status(
                    grpc.StatusCode.UNAUTHENTICATED,
                    'Invalid token payload'
                )
            
            # 將認證信息注入到 context
            # 注意：由於 gRPC Python 的限制，我們無法直接修改 context
            # 需要在具體 servicer 中重新驗證（這裡主要做門禁）
            logging.debug(f"認證成功: user_id={user_id}, method={method}")
            
            # 節點方法需要額外檢查 node_id 與 user_id 匹配
            if method in NODE_METHODS:
                # 這裡可以添加額外的節點身份驗證邏輯
                # 例如檢查 user_id 是否對應已註冊的節點
                pass
            
            return continuation(handler_call_details)
            
        except jwt.ExpiredSignatureError:
            logging.warning(f"Token 已過期: {method}")
            return self._abort_with_status(
                grpc.StatusCode.UNAUTHENTICATED,
                'Token expired'
            )
        except jwt.InvalidTokenError as e:
            logging.warning(f"無效 token: {method}, error: {e}")
            return self._abort_with_status(
                grpc.StatusCode.UNAUTHENTICATED,
                'Invalid token'
            )
        except Exception as e:
            logging.error(f"認證錯誤: {method}, error: {e}", exc_info=True)
            return self._abort_with_status(
                grpc.StatusCode.INTERNAL,
                'Authentication error'
            )
    
    def _abort_with_status(self, code, details):
        """返回錯誤狀態"""
        def abort(request, context):
            context.abort(code, details)
        
        return grpc.unary_unary_rpc_method_handler(
            abort,
            request_deserializer=lambda x: None,
            response_serializer=lambda x: None
        )


def create_authenticated_server(max_workers=20):
    """創建帶認證的 gRPC server"""
    options = [
        ('grpc.keepalive_time_ms', 10000),
        ('grpc.keepalive_timeout_ms', 5000),
        ('grpc.keepalive_permit_without_calls', True),
        ('grpc.http2.max_pings_without_data', 0),
        ('grpc.http2.min_time_between_pings_ms', 10000),
        ('grpc.http2.min_ping_interval_without_data_ms', 5000),
        ('grpc.max_receive_message_length', 1024 * 1024 * 1024),  # 1GB
        ('grpc.max_send_message_length', 1024 * 1024 * 1024),     # 1GB
        ('grpc.http2.max_frame_size', 16 * 1024 * 1024),         # 16MB frames
    ]
    
    # 創建帶認證攔截器的 server
    interceptors = [AuthInterceptor()]
    
    server = grpc.server(
        futures.ThreadPoolExecutor(max_workers=max_workers),
        interceptors=interceptors,
        options=options
    )
    
    return server


if __name__ == "__main__":
    # 測試用
    from concurrent import futures
    
    server = create_authenticated_server()
    logging.info("認證攔截器測試 server 創建成功")
