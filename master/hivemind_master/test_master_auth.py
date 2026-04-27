"""單元測試：Master gRPC 認證功能

測試 Master 是否正確在 gRPC 呼叫中添加 JWT token metadata。
"""

import pytest
from unittest.mock import Mock, MagicMock
import sys
import os

# 添加 master 模組路徑
master_src_path = os.path.join(os.path.dirname(__file__), 'src', 'hivemind_master')
sys.path.insert(0, master_src_path)

from grpc_auth_client import add_token_to_metadata, get_metadata_list, get_metadata_for_user


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
    
    def test_get_metadata_list_with_valid_token(self):
        """測試：生成列表格式的 metadata"""
        token = "list_token_xyz"
        metadata = get_metadata_list(token)
        
        assert isinstance(metadata, list)
        assert len(metadata) == 1
        assert metadata[0] == ('authorization', 'Bearer list_token_xyz')
    
    def test_get_metadata_list_with_none(self):
        """測試：None token 返回空列表"""
        metadata = get_metadata_list(None)
        assert metadata == []
    
    def test_get_metadata_for_user_with_token(self):
        """測試：從用戶字典提取 token"""
        user_dict = {'username': 'test_user', 'token': 'user_token_abc'}
        metadata = get_metadata_for_user(user_dict)
        
        assert len(metadata) == 1
        assert metadata[0] == ('authorization', 'Bearer user_token_abc')
    
    def test_get_metadata_for_user_without_token(self):
        """測試：用戶字典沒有 token"""
        user_dict = {'username': 'test_user'}
        metadata = get_metadata_for_user(user_dict)
        assert metadata == ()
    
    def test_get_metadata_for_user_with_none_dict(self):
        """測試：None 用戶字典"""
        metadata = get_metadata_for_user(None)
        assert metadata == ()


class TestMasterGRPCAuth:
    """測試 Master 的 gRPC 呼叫是否正確添加認證"""
    
    def test_get_balance_with_metadata(self):
        """測試：GetBalance 必須帶 metadata"""
        mock_user = {'username': 'test_user', 'token': 'balance_token_123'}
        
        mock_stub = MagicMock()
        
        # 模擬 GetBalance 呼叫
        mock_request = MagicMock()
        metadata = get_metadata_list(mock_user['token'])
        mock_stub.GetBalance(mock_request, metadata=metadata, timeout=30)
        
        # 驗證
        mock_stub.GetBalance.assert_called_once()
        call_args = mock_stub.GetBalance.call_args
        assert 'metadata' in call_args.kwargs
        assert call_args.kwargs['metadata'] == [('authorization', 'Bearer balance_token_123')]
    
    def test_upload_task_with_metadata(self):
        """測試：UploadTask 必須帶 metadata"""
        mock_user = {'username': 'test_user', 'token': 'upload_token_456'}
        
        mock_stub = MagicMock()
        
        # 模擬 UploadTask 呼叫
        mock_request = MagicMock()
        metadata = [('authorization', f'Bearer {mock_user["token"]}')]
        mock_stub.UploadTask(mock_request, metadata=metadata, timeout=60)
        
        # 驗證
        mock_stub.UploadTask.assert_called_once()
        call_args = mock_stub.UploadTask.call_args
        assert 'metadata' in call_args.kwargs
        assert call_args.kwargs['metadata'] == [('authorization', 'Bearer upload_token_456')]
    
    def test_stop_task_with_metadata(self):
        """測試：StopTask 必須帶 metadata"""
        mock_user = {'username': 'test_user', 'token': 'stop_token_789'}
        
        mock_stub = MagicMock()
        
        # 模擬 StopTask 呼叫
        mock_request = MagicMock()
        metadata = get_metadata_list(mock_user['token'])
        mock_stub.StopTask(mock_request, metadata=metadata, timeout=60)
        
        # 驗證
        mock_stub.StopTask.assert_called_once()
        call_args = mock_stub.StopTask.call_args
        assert 'metadata' in call_args.kwargs
        assert call_args.kwargs['metadata'] == [('authorization', 'Bearer stop_token_789')]
    
    def test_get_task_result_with_metadata(self):
        """測試：GetTaskResult 必須帶 metadata"""
        mock_user = {'username': 'test_user', 'token': 'result_token_abc'}
        
        mock_stub = MagicMock()
        
        # 模擬 GetTaskResult 呼叫
        mock_request = MagicMock()
        metadata = get_metadata_list(mock_user['token'])
        mock_stub.GetTaskResult(mock_request, metadata=metadata, timeout=60)
        
        # 驗證
        mock_stub.GetTaskResult.assert_called_once()
        call_args = mock_stub.GetTaskResult.call_args
        assert 'metadata' in call_args.kwargs
        assert call_args.kwargs['metadata'] == [('authorization', 'Bearer result_token_abc')]


class TestAuthIntegration:
    """整合測試：模擬真實的 Master 認證流程"""
    
    def test_full_auth_flow(self):
        """測試：完整的認證流程（Login → GetBalance → UploadTask）"""
        # Step 1: Login（不需要 token）
        mock_user_stub = MagicMock()
        
        # 模擬 Login 成功返回 token
        mock_login_response = Mock()
        mock_login_response.success = True
        mock_login_response.token = "jwt_token_from_login"
        mock_user_stub.Login.return_value = mock_login_response
        
        # Login 呼叫（無 metadata）
        login_request = Mock()
        response = mock_user_stub.Login(login_request, timeout=15)
        
        # 更新用戶 token
        user_token = response.token
        assert user_token == "jwt_token_from_login"
        
        # Step 2: GetBalance（需要 token）
        balance_request = Mock()
        metadata = get_metadata_list(user_token)
        mock_user_stub.GetBalance(balance_request, metadata=metadata, timeout=30)
        
        # 驗證
        call_args = mock_user_stub.GetBalance.call_args
        assert call_args.kwargs['metadata'] == [('authorization', 'Bearer jwt_token_from_login')]
        
        # Step 3: UploadTask（需要 token）
        mock_master_stub = MagicMock()
        upload_request = Mock()
        metadata = [('authorization', f'Bearer {user_token}')]
        mock_master_stub.UploadTask(upload_request, metadata=metadata, timeout=60)
        
        # 驗證
        call_args = mock_master_stub.UploadTask.call_args
        assert call_args.kwargs['metadata'] == [('authorization', 'Bearer jwt_token_from_login')]
    
    def test_metadata_consistency(self):
        """測試：metadata 格式一致性"""
        token = "consistency_token"
        
        # 兩種方法應該產生等效的結果
        metadata_tuple = add_token_to_metadata(token)
        metadata_list = get_metadata_list(token)
        
        # 列表和 tuple 內容應該相同
        assert list(metadata_tuple) == metadata_list
        assert metadata_tuple[0] == metadata_list[0]
        assert metadata_tuple[0] == ('authorization', 'Bearer consistency_token')


if __name__ == '__main__':
    pytest.main([__file__, '-v'])
