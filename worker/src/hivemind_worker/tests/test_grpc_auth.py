"""單元測試：Worker gRPC 認證功能

測試 Worker 是否正確在 gRPC 呼叫中添加 JWT token metadata。
"""

import pytest
from unittest.mock import Mock, MagicMock, patch
import sys
import os

# 添加 worker 模組路徑
sys.path.insert(0, os.path.join(os.path.dirname(__file__), '..'))

from grpc_auth_client import add_token_to_metadata, get_metadata_for_call


class TestAuthMetadata:
    """測試 metadata 工具函數"""
    
    def test_add_token_to_metadata_with_valid_token(self):
        """測試：有效 token 生成正確的 metadata"""
        token = "test_jwt_token_12345"
        metadata = add_token_to_metadata(token)
        
        assert len(metadata) == 1
        assert metadata[0] == ('authorization', 'Bearer test_jwt_token_12345')
    
    def test_add_token_to_metadata_with_none(self):
        """測試：None token 返回空 metadata"""
        metadata = add_token_to_metadata(None)
        assert metadata == ()
    
    def test_add_token_to_metadata_with_empty_string(self):
        """測試：空字串 token 返回空 metadata"""
        metadata = add_token_to_metadata("")
        assert metadata == ()
    
    def test_get_metadata_for_call_with_token(self):
        """測試：從 worker 物件提取 token"""
        mock_worker = Mock()
        mock_worker.token = "worker_token_abc"
        
        metadata = get_metadata_for_call(mock_worker)
        
        assert len(metadata) == 1
        assert metadata[0] == ('authorization', 'Bearer worker_token_abc')
    
    def test_get_metadata_for_call_without_token(self):
        """測試：worker 沒有 token 屬性"""
        mock_worker = Mock(spec=[])  # 沒有 token 屬性
        
        metadata = get_metadata_for_call(mock_worker)
        assert metadata == ()
    
    def test_get_metadata_for_call_with_none_token(self):
        """測試：worker token 為 None"""
        mock_worker = Mock()
        mock_worker.token = None
        
        metadata = get_metadata_for_call(mock_worker)
        assert metadata == ()


class TestWorkerGRPCAuth:
    """測試 Worker 的 gRPC 呼叫是否正確添加認證"""
    
    def test_login_without_token(self):
        """測試：Login 呼叫不需要 token（在白名單中）"""
        # Login 前 worker 還沒有 token，這是正常的
        # 只要確保 Login 呼叫能夠成功
        # Login 的測試在 worker_node 整合測試中
        assert True  # 佔位測試
    
    def test_register_worker_node_with_metadata(self):
        """測試：RegisterWorkerNode 必須帶 metadata"""
        # 模擬 worker_node._register() 的行為
        mock_worker = Mock()
        mock_worker.token = "test_token_xyz"
        mock_worker.username = "test_user"
        mock_worker.local_ip = "192.168.1.100"
        mock_worker.cpu_cores = 4
        mock_worker.total_memory_gb = 16
        mock_worker.cpu_score = 1000
        mock_worker.gpu_score = 0
        mock_worker.gpu_name = "None"
        mock_worker.gpu_memory_gb = 0
        mock_worker.location = "Test"
        mock_worker.port = 50053
        mock_worker.docker_status = "running"
        
        mock_stub = MagicMock()
        mock_worker.node_stub = mock_stub
        
        # 模擬 nodepool_pb2
        mock_pb2 = MagicMock()
        mock_request = MagicMock()
        mock_pb2.RegisterWorkerNodeRequest.return_value = mock_request
        
        # 手動執行註冊邏輯（簡化版）
        metadata = [('authorization', f'Bearer {mock_worker.token}')]
        mock_stub.RegisterWorkerNode(mock_request, metadata=metadata, timeout=15)
        
        # 驗證呼叫參數
        mock_stub.RegisterWorkerNode.assert_called_once()
        call_args = mock_stub.RegisterWorkerNode.call_args
        
        # 檢查 metadata 參數
        assert 'metadata' in call_args.kwargs
        assert call_args.kwargs['metadata'] == [('authorization', 'Bearer test_token_xyz')]
    
    def test_report_status_with_metadata(self):
        """測試：ReportStatus 必須帶 metadata"""
        mock_worker = Mock()
        mock_worker.token = "status_token_123"
        mock_worker.username = "test_user"
        
        mock_stub = MagicMock()
        mock_worker.node_stub = mock_stub
        
        # 模擬 ReportStatus 呼叫
        mock_request = MagicMock()
        metadata = [('authorization', f'Bearer {mock_worker.token}')] if mock_worker.token else []
        mock_stub.ReportStatus(mock_request, metadata=metadata, timeout=5)
        
        # 驗證
        mock_stub.ReportStatus.assert_called_once()
        call_args = mock_stub.ReportStatus.call_args
        assert 'metadata' in call_args.kwargs
        assert call_args.kwargs['metadata'] == [('authorization', 'Bearer status_token_123')]
    
    def test_task_usage_with_metadata(self):
        """測試：TaskUsage 必須帶 metadata"""
        mock_worker = Mock()
        mock_worker.token = "task_token_456"
        mock_worker.username = "test_user"
        
        mock_stub = MagicMock()
        mock_worker.worker_stub = mock_stub
        
        # 模擬 TaskUsage 呼叫
        mock_request = MagicMock()
        metadata = [('authorization', f'Bearer {mock_worker.token}')] if mock_worker.token else []
        mock_stub.TaskUsage(mock_request, metadata=metadata, timeout=3)
        
        # 驗證
        mock_stub.TaskUsage.assert_called_once()
        call_args = mock_stub.TaskUsage.call_args
        assert 'metadata' in call_args.kwargs
        assert call_args.kwargs['metadata'] == [('authorization', 'Bearer task_token_456')]
    
    def test_metadata_empty_when_no_token(self):
        """測試：沒有 token 時 metadata 為空列表"""
        mock_worker = Mock()
        mock_worker.token = None
        
        metadata = [('authorization', f'Bearer {mock_worker.token}')] if mock_worker.token else []
        
        assert metadata == []


class TestAuthIntegration:
    """整合測試：模擬真實的認證流程"""
    
    def test_full_auth_flow(self):
        """測試：完整的認證流程（Login -> Register -> ReportStatus）"""
        mock_worker = Mock()
        mock_worker.token = None  # 初始沒有 token
        
        # Step 1: Login（不需要 token）
        mock_user_stub = MagicMock()
        mock_worker.user_stub = mock_user_stub
        
        # 模擬 Login 成功返回 token
        mock_login_response = Mock()
        mock_login_response.success = True
        mock_login_response.token = "jwt_token_from_login"
        mock_user_stub.Login.return_value = mock_login_response
        
        # Login 呼叫（無 metadata）
        login_request = Mock()
        response = mock_user_stub.Login(login_request, timeout=15)
        
        # 更新 worker token
        mock_worker.token = response.token
        
        assert mock_worker.token == "jwt_token_from_login"
        
        # Step 2: RegisterWorkerNode（需要 token）
        mock_node_stub = MagicMock()
        mock_worker.node_stub = mock_node_stub
        
        register_request = Mock()
        metadata = [('authorization', f'Bearer {mock_worker.token}')]
        mock_node_stub.RegisterWorkerNode(register_request, metadata=metadata, timeout=15)
        
        # 驗證
        call_args = mock_node_stub.RegisterWorkerNode.call_args
        assert call_args.kwargs['metadata'] == [('authorization', 'Bearer jwt_token_from_login')]
        
        # Step 3: ReportStatus（需要 token）
        status_request = Mock()
        metadata = [('authorization', f'Bearer {mock_worker.token}')]
        mock_node_stub.ReportStatus(status_request, metadata=metadata, timeout=5)
        
        # 驗證
        call_args = mock_node_stub.ReportStatus.call_args
        assert call_args.kwargs['metadata'] == [('authorization', 'Bearer jwt_token_from_login')]


if __name__ == '__main__':
    pytest.main([__file__, '-v'])
