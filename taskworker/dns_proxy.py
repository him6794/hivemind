import grpc
from .protos import taskworker_pb2, taskworker_pb2_grpc

class DNSProxy(taskworker_pb2_grpc.DNSServiceServicer):
    """DNS 代理服務"""
    
    def __init__(self, worker):
        self.worker = worker
        
    async def ResolveDomain(self, request, context):
        """解析域名"""
        try:
            domain = request.domain
            
            # 將域名解析請求轉發到節點池
            # 實際實現需要與節點池的 DNS 服務對接
            
            return taskworker_pb2.DNSResponse(
                resolved_ip="127.0.0.1",  # 實際實現時將由節點池提供正確的 IP
                status=True,
                message="域名解析成功"
            )
            
        except Exception as e:
            return taskworker_pb2.DNSResponse(
                resolved_ip="",
                status=False,
                message=f"域名解析失敗: {str(e)}"
            )
