import grpc
from concurrent import futures
import time
import nodepool_pb2
import nodepool_pb2_grpc
import logging
import math

# 設置日志
logging.basicConfig(level=logging.INFO)
logger = logging.getLogger(__name__)

class NodePoolServicer(nodepool_pb2_grpc.NodePoolServicer):
    def __init__(self):
        # 初始化時可以載入節點數據或連接到數據庫
        self.nodes = {}

    def Register(self, request, context):
        # 註冊節點的邏輯
        logger.info(f"Node registered: {request.ip}, CPU: {request.cpu_score}, GPU: {request.gpu_score}")
        self.nodes[request.ip] = request
        return nodepool_pb2.RegisterResponse(message="Node registered")

    def Get(self, request, context):
        # 根據請求的條件來回傳節點，執行最佳匹配策略
        logger.info(f"Get request received for CPU: {request.cpu_score}, GPU: {request.gpu_score}")

        # 篩選符合基本要求的節點
        eligible_nodes = [
            node for node in self.nodes.values() 
            if node.cpu_score >= request.cpu_score and node.gpu_score >= request.gpu_score
        ]

        if not eligible_nodes:
            context.set_code(grpc.StatusCode.NOT_FOUND)
            context.set_details("No suitable node found.")
            return nodepool_pb2.Node()

        # 計算每個節點與請求的差距
        def distance_from_request(node):
            cpu_diff = (node.cpu_score - request.cpu_score) / request.cpu_score
            gpu_diff = (node.gpu_score - request.gpu_score) / request.gpu_score
            # 使用歐幾里得距離的概念來衡量匹配度
            return math.sqrt(cpu_diff**2 + gpu_diff**2)

        # 選擇與請求最接近的節點
        best_match = min(eligible_nodes, key=distance_from_request)
        return best_match

    def UpdateStatus(self, request, context):
        # 這個方法保持不變
        if request.ip in self.nodes:
            logger.info(f"UpdateStatus: Node {request.ip} status updated to {request.status}")
            return nodepool_pb2.UpdateResponse(message="Status updated")
        else:
            context.set_code(grpc.StatusCode.NOT_FOUND)
            context.set_details("Node not found.")
            return nodepool_pb2.UpdateResponse(message="Node not found")

def serve():
    server = grpc.server(futures.ThreadPoolExecutor(max_workers=10))
    nodepool_pb2_grpc.add_NodePoolServicer_to_server(NodePoolServicer(), server)
    server.add_insecure_port('[::]:50051')
    logger.info("Server started at port 50051...")
    server.start()

    try:
        while True:
            time.sleep(86400)  # 一天的秒數，防止主線程退出
    except KeyboardInterrupt:
        logger.info("Keyboard interrupt received, shutting down server.")
        server.stop(0)

if __name__ == '__main__':
    serve()