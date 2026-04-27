"""認證攔截器單元測試"""

import unittest
from unittest.mock import MagicMock, patch
import jwt
import grpc
from datetime import datetime, timedelta, timezone

from auth_interceptor import AuthInterceptor, WHITELIST_METHODS
from config import Config


class TestAuthInterceptor(unittest.TestCase):
    
    def setUp(self):
        self.interceptor = AuthInterceptor()
        self.context = MagicMock()
        self.continuation = MagicMock()
        
    def _create_handler_call_details(self, method, metadata=None):
        """創建模擬的 handler_call_details"""
        details = MagicMock()
        details.method = method
        details.invocation_metadata = metadata or []
        return details
    
    def _generate_valid_token(self, user_id=1, username="test_user"):
        """生成有效的 JWT token"""
        payload = {
            "user_id": user_id,
            "username": username,
            "exp": datetime.now(timezone.utc) + timedelta(hours=1)
        }
        return jwt.encode(payload, Config.SECRET_KEY, algorithm="HS256")
    
    def test_whitelist_methods_pass_through(self):
        """測試白名單方法直接放行"""
        for method in WHITELIST_METHODS:
            details = self._create_handler_call_details(method)
            result = self.interceptor.intercept_service(self.continuation, details)
            self.assertEqual(result, self.continuation.return_value)
    
    def test_missing_authorization_header(self):
        """測試缺少 authorization header"""
        method = "/nodepool.NodeManagerService/RegisterWorkerNode"
        details = self._create_handler_call_details(method, metadata=[])
        
        result = self.interceptor.intercept_service(self.continuation, details)
        
        # 應該返回錯誤處理器
        self.assertNotEqual(result, self.continuation.return_value)
    
    def test_invalid_authorization_format(self):
        """測試無效的 authorization 格式"""
        method = "/nodepool.MasterNodeService/UploadTask"
        metadata = [('authorization', 'InvalidFormat token123')]
        details = self._create_handler_call_details(method, metadata)
        
        result = self.interceptor.intercept_service(self.continuation, details)
        self.assertNotEqual(result, self.continuation.return_value)
    
    def test_valid_token_passes(self):
        """測試有效 token 通過驗證"""
        token = self._generate_valid_token()
        method = "/nodepool.MasterNodeService/UploadTask"
        metadata = [('authorization', f'Bearer {token}')]
        details = self._create_handler_call_details(method, metadata)
        
        result = self.interceptor.intercept_service(self.continuation, details)
        self.assertEqual(result, self.continuation.return_value)
    
    def test_expired_token_rejected(self):
        """測試過期 token 被拒絕"""
        # 生成已過期的 token
        payload = {
            "user_id": 1,
            "exp": datetime.now(timezone.utc) - timedelta(hours=1)  # 1 小時前過期
        }
        expired_token = jwt.encode(payload, Config.SECRET_KEY, algorithm="HS256")
        
        method = "/nodepool.MasterNodeService/UploadTask"
        metadata = [('authorization', f'Bearer {expired_token}')]
        details = self._create_handler_call_details(method, metadata)
        
        result = self.interceptor.intercept_service(self.continuation, details)
        self.assertNotEqual(result, self.continuation.return_value)
    
    def test_invalid_token_rejected(self):
        """測試無效 token 被拒絕"""
        invalid_token = "invalid.token.here"
        method = "/nodepool.MasterNodeService/UploadTask"
        metadata = [('authorization', f'Bearer {invalid_token}')]
        details = self._create_handler_call_details(method, metadata)
        
        result = self.interceptor.intercept_service(self.continuation, details)
        self.assertNotEqual(result, self.continuation.return_value)
    
    def test_token_without_user_id(self):
        """測試缺少 user_id 的 token"""
        payload = {
            "some_field": "value",
            "exp": datetime.now(timezone.utc) + timedelta(hours=1)
        }
        token = jwt.encode(payload, Config.SECRET_KEY, algorithm="HS256")
        
        method = "/nodepool.MasterNodeService/UploadTask"
        metadata = [('authorization', f'Bearer {token}')]
        details = self._create_handler_call_details(method, metadata)
        
        result = self.interceptor.intercept_service(self.continuation, details)
        self.assertNotEqual(result, self.continuation.return_value)
    
    def test_multiple_concurrent_requests(self):
        """測試並發請求處理"""
        import threading
        
        token = self._generate_valid_token()
        method = "/nodepool.MasterNodeService/GetAllUserTasks"
        metadata = [('authorization', f'Bearer {token}')]
        
        results = []
        
        def make_request():
            details = self._create_handler_call_details(method, metadata)
            result = self.interceptor.intercept_service(self.continuation, details)
            results.append(result)
        
        threads = [threading.Thread(target=make_request) for _ in range(10)]
        for t in threads:
            t.start()
        for t in threads:
            t.join()
        
        # 所有請求都應該通過
        for result in results:
            self.assertEqual(result, self.continuation.return_value)


if __name__ == '__main__':
    unittest.main()
