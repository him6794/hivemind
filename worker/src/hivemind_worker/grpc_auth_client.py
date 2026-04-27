"""gRPC Authentication Client Utilities.

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


def get_metadata_for_call(worker) -> Tuple[Tuple[str, str], ...]:
    """
    從 worker 物件獲取當前的認證 metadata。
    
    這是一個便利函數，自動從 worker.token 讀取並格式化為 gRPC metadata。
    
    Args:
        worker: WorkerNode 實例
        
    Returns:
        gRPC metadata tuple
        
    Example:
        >>> metadata = get_metadata_for_call(self.worker_node)
        >>> stub.RegisterWorkerNode(request, metadata=metadata)
    """
    token = getattr(worker, 'token', None)
    return add_token_to_metadata(token)

