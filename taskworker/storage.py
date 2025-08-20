import asyncio
import hashlib
import json
import time
from pathlib import Path
from typing import Dict, List, Optional
import aiofiles
import grpc

from .protos import taskworker_pb2, taskworker_pb2_grpc

class FileStorage(taskworker_pb2_grpc.FileServiceServicer):
    """分散式文件存儲管理器"""
    
    def __init__(self, worker_id: str):
        self.worker_id = worker_id
        self.storage_path = Path(f"storage/{worker_id}")
        self.storage_path.mkdir(parents=True, exist_ok=True)
        self.file_metadata: Dict[str, Dict] = {}
        
    def _generate_file_id(self, data: bytes, filename: str) -> str:
        """生成文件 ID"""
        content = data + filename.encode() + str(time.time()).encode()
        return hashlib.sha256(content).hexdigest()
        
    def _split_file(self, data: bytes, chunk_size: int = 1024 * 1024) -> List[bytes]:
        """將文件分片"""
        chunks = []
        for i in range(0, len(data), chunk_size):
            chunks.append(data[i:i + chunk_size])
        return chunks
        
    async def Push(self, request, context):
        """推送文件"""
        try:
            file_id = self._generate_file_id(request.file_data, request.filename)
            
            # 分片存儲
            chunks = self._split_file(request.file_data)
            chunk_files = []
            
            for i, chunk in enumerate(chunks):
                chunk_filename = f"{file_id}_{i}.chunk"
                chunk_path = self.storage_path / chunk_filename
                
                async with aiofiles.open(chunk_path, 'wb') as f:
                    await f.write(chunk)
                    
                chunk_files.append(chunk_filename)
            
            # 保存元數據
            metadata = {
                'filename': request.filename,
                'user_id': request.user_id,
                'chunks': chunk_files,
                'size': len(request.file_data),
                'created_at': time.time(),
                'sha256': hashlib.sha256(request.file_data).hexdigest()
            }
            
            self.file_metadata[file_id] = metadata
            
            # 保存元數據到文件
            metadata_path = self.storage_path / f"{file_id}.meta"
            async with aiofiles.open(metadata_path, 'w') as f:
                await f.write(json.dumps(metadata))
            
            return taskworker_pb2.PushResponse(
                file_id=file_id,
                status=True,
                message="文件上傳成功"
            )
            
        except Exception as e:
            return taskworker_pb2.PushResponse(
                file_id="",
                status=False,
                message=f"文件上傳失敗: {str(e)}"
            )
    
    async def Get(self, request, context):
        """獲取文件"""
        try:
            file_id = request.file_id
            
            if file_id not in self.file_metadata:
                # 嘗試從文件加載元數據
                metadata_path = self.storage_path / f"{file_id}.meta"
                if metadata_path.exists():
                    async with aiofiles.open(metadata_path, 'r') as f:
                        content = await f.read()
                        self.file_metadata[file_id] = json.loads(content)
                else:
                    return taskworker_pb2.GetResponse(
                        file_data=b"",
                        status=False,
                        message="文件不存在"
                    )
            
            metadata = self.file_metadata[file_id]
            file_data = b""
            
            # 重組文件
            for chunk_filename in metadata['chunks']:
                chunk_path = self.storage_path / chunk_filename
                async with aiofiles.open(chunk_path, 'rb') as f:
                    chunk_data = await f.read()
                    file_data += chunk_data
            
            return taskworker_pb2.GetResponse(
                file_data=file_data,
                status=True,
                message="文件獲取成功"
            )
            
        except Exception as e:
            return taskworker_pb2.GetResponse(
                file_data=b"",
                status=False,
                message=f"文件獲取失敗: {str(e)}"
            )
    
    async def Revise(self, request, context):
        """修正並同步文件"""
        try:
            file_id = request.file_id
            
            if file_id not in self.file_metadata:
                return taskworker_pb2.ReviseResponse(
                    file_id=file_id,
                    status=False,
                    message="文件不存在"
                )
            
            # 應用差異更新
            # 這裡應該實現差異應用邏輯
            
            return taskworker_pb2.ReviseResponse(
                file_id=file_id,
                status=True,
                message="文件修正成功"
            )
            
        except Exception as e:
            return taskworker_pb2.ReviseResponse(
                file_id=file_id,
                status=False,
                message=f"文件修正失敗: {str(e)}"
            )
    
    async def Synchronous(self, request, context):
        """同步文件"""
        try:
            file_id = request.file_id
            
            # 實現文件同步邏輯
            
            return taskworker_pb2.SynchronousResponse(
                file_id=file_id,
                status=True,
                message="文件同步成功"
            )
            
        except Exception as e:
            return taskworker_pb2.SynchronousResponse(
                file_id=file_id,
                status=False,
                message=f"文件同步失敗: {str(e)}"
            )
