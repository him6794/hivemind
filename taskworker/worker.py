import asyncio
import hashlib
import json
import time
from typing import Dict, Any, Callable, Optional
import grpc
from concurrent import futures
import logging

from .storage import FileStorage
from .rpc_service import RPCService
from .dns_proxy import DNSProxy
from .protos import taskworker_pb2_grpc

logger = logging.getLogger(__name__)

class TaskWorker:
    """主要的 TaskWorker 類別，提供分散式任務執行功能"""
    
    def __init__(self, worker_id: str, node_pool_address: str = "localhost:50051"):
        self.worker_id = worker_id
        self.node_pool_address = node_pool_address
        self.storage = FileStorage(worker_id)
        self.rpc_service = RPCService(self)
        self.dns_proxy = DNSProxy(self)
        self.functions: Dict[str, Callable] = {}
        self.server: Optional[grpc.Server] = None
        
    def register_function(self, name: str, func: Callable):
        """註冊可遠程調用的函數"""
        self.functions[name] = func
        logger.info(f"已註冊函數: {name}")
        
    def function(self, name: str = None):
        """裝飾器：自動註冊函數"""
        def decorator(func: Callable):
            func_name = name or func.__name__
            self.register_function(func_name, func)
            return func
        return decorator
        
    async def start_server(self, port: int = 50052):
        """啟動 gRPC 服務器"""
        self.server = grpc.aio.server(futures.ThreadPoolExecutor(max_workers=10))
        
        # 添加服務
        taskworker_pb2_grpc.add_FileServiceServicer_to_server(
            self.storage, self.server
        )
        taskworker_pb2_grpc.add_RPCServiceServicer_to_server(
            self.rpc_service, self.server
        )
        taskworker_pb2_grpc.add_DNSServiceServicer_to_server(
            self.dns_proxy, self.server
        )
        
        listen_addr = f'[::]:{port}'
        self.server.add_insecure_port(listen_addr)
        
        logger.info(f"TaskWorker 服務器啟動於 {listen_addr}")
        await self.server.start()
        await self.server.wait_for_termination()
        
    async def stop_server(self):
        """停止服務器"""
        if self.server:
            await self.server.stop(5)
            
    def get_secret(self, key: str) -> str:
        """從節點池獲取密鑰"""
        # 這裡應該通過 gRPC 向節點池請求密鑰
        # 實際實現需要與節點池的接口對接
        pass
        
    def call_external_api(self, url: str, **kwargs) -> Any:
        """調用外部 API（通過節點池代理）"""
        # 這裡應該通過節點池代理外部請求
        # 實際實現需要與節點池的接口對接
        pass
