import json
import traceback
from typing import Any
import grpc

from .protos import taskworker_pb2, taskworker_pb2_grpc

class RPCService(taskworker_pb2_grpc.RPCServiceServicer):
    """RPC 函數調用服務"""
    
    def __init__(self, worker):
        self.worker = worker
        
    async def CallFunction(self, request, context):
        """調用註冊的函數"""
        try:
            function_name = request.function_name
            
            if function_name not in self.worker.functions:
                return taskworker_pb2.FunctionCallResponse(
                    result="",
                    status=False,
                    error=f"函數 {function_name} 未註冊"
                )
            
            func = self.worker.functions[function_name]
            
            # 解析參數
            args = list(request.args)
            kwargs = dict(request.kwargs)
            
            # 調用函數
            if asyncio.iscoroutinefunction(func):
                result = await func(*args, **kwargs)
            else:
                result = func(*args, **kwargs)
            
            # 序列化結果
            result_str = json.dumps(result) if result is not None else ""
            
            return taskworker_pb2.FunctionCallResponse(
                result=result_str,
                status=True,
                error=""
            )
            
        except Exception as e:
            error_msg = f"{str(e)}\n{traceback.format_exc()}"
            return taskworker_pb2.FunctionCallResponse(
                result="",
                status=False,
                error=error_msg
            )
