"""gRPC Authentication Client Utilities for Master.

提供 JWT token 認證相關的工具函數，用於在 gRPC 呼叫中添加認證 metadata。
"""

from __future__ import annotations
from typing import TYPE_CHECKING, Tuple

if TYPE_CHECKING:
    from typing import Optional


def add_token_to_metadata(token: Optional[str]) -> Tuple[Tuple[str, str], ...]:
    """
    創建包含 JWT token 的 metadata tuple。
    
    Args:
        token: JWT token 字串
        
    Returns:
        gRPC metadata tuple，格式為 (('authorization', 'Bearer <token>'),)
        
    Example:
        >>> metadata = add_token_to_metadata("your_jwt_token")
        >>> stub.SomeMethod(request, metadata=metadata)
    """
    if token:
        return (('authorization', f'Bearer {token}'),)
    return ()


def get_metadata_for_user(user_dict: dict) -> Tuple[Tuple[str, str], ...]:
    """
    從用戶字典獲取當前的認證 metadata。
    
    這是一個便利函數，自動從 user['token'] 讀取並格式化為 gRPC metadata。
    
    Args:
        user_dict: 用戶資料字典，包含 'token' 鍵
        
    Returns:
        gRPC metadata tuple
        
    Example:
        >>> user = self.get_user(username)
        >>> metadata = get_metadata_for_user(user)
        >>> stub.SomeMethod(request, metadata=metadata)
    """
    token = user_dict.get('token') if user_dict else None
    return add_token_to_metadata(token)


def get_metadata_list(token: Optional[str]) -> list:
    """
    創建 metadata 列表格式（某些程式碼使用列表而非 tuple）。
    
    Args:
        token: JWT token 字串
        
    Returns:
        metadata 列表，格式為 [('authorization', 'Bearer <token>')]
        
    Example:
        >>> metadata = get_metadata_list(user['token'])
        >>> stub.SomeMethod(request, metadata=metadata)
    """
    if token:
        return [('authorization', f'Bearer {token}')]
    return []
