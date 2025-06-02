import grpc
from concurrent import futures
import rpc_pb2
import rpc_pb2_grpc
import logging

# 設置日志
logging.basicConfig(level=logging.INFO)
logger = logging.getLogger(__name__)

class MasterNodeServiceServicer(rpc_pb2_grpc.MasterNodeServiceServicer):
    def __init__(self):
        # 這裡可以初始化一些狀態或連接池
        self.connected_nodes = {}

    def ConnectNode(self, request, context):
        node_id = request.node_id
        ip = request.ip
        # 這裡可以進行一些驗證或登記工作節點
        self.connected_nodes[node_id] = ip
        logger.info(f"Node {node_id} at {ip} has connected to master node.")
        return rpc_pb2.ConnectNodeResponse(success=True, message="Node connected successfully")

    def RequestResources(self, request, context):
        cpu_score = request.cpu_score
        gpu_score = request.gpu_score

        try:
            with grpc.insecure_channel("localhost:50051") as channel:
                stub = rpc_pb2_grpc.NodePoolStub(channel)
                node = stub.GetNode(rpc_pb2.NodeRequest(cpu_score=cpu_score, gpu_score=gpu_score))
                if node.node_id:
                    # 這裡我們通知節點連接到主控端
                    with grpc.insecure_channel(f"localhost:{50050 + int(node.node_id)}") as worker_channel:
                        worker_stub = rpc_pb2_grpc.WorkerNodeServiceStub(worker_channel)
                        connect_response = worker_stub.ConnectToMaster(rpc_pb2.ConnectToMasterRequest(
                            master_address="localhost:50052"  # 主控端地址
                        ))
                    return rpc_pb2.NodeResponse(node_id=node.node_id, ip=node.ip, cpu_score=node.cpu_score, gpu_score=node.gpu_score)
                else:
                    return rpc_pb2.NodeResponse(node_id="", ip="", error_message="No suitable node found.")
        except grpc.RpcError as e:
            return rpc_pb2.NodeResponse(node_id="", ip="", error_message=str(e))

def serve():
    server = grpc.server(futures.ThreadPoolExecutor(max_workers=10))
    rpc_pb2_grpc.add_MasterNodeServiceServicer_to_server(MasterNodeServiceServicer(), server)
    server.add_insecure_port('[::]:50052')
    print("Master Node is running on port 50052...")
    server.start()
    server.wait_for_termination()

if __name__ == "__main__":
    serve()