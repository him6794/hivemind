import grpc
import rpc_pb2
import rpc_pb2_grpc
import logging
import time

# 設置日志
logging.basicConfig(level=logging.INFO)
logger = logging.getLogger(__name__)

class WorkerNodeServiceServicer(rpc_pb2_grpc.WorkerNodeServiceServicer):
    def __init__(self, node_id, node_ip, cpu_score, gpu_score):
        self.node_id = node_id
        self.node_ip = node_ip
        self.cpu_score = cpu_score
        self.gpu_score = gpu_score
        self.registered = False
        self.master_connected = False

    def RegisterToPool(self):
        while not self.registered:
            try:
                with grpc.insecure_channel("localhost:50051") as channel:
                    stub = rpc_pb2_grpc.NodePoolStub(channel)
                    response = stub.RegisterNode(rpc_pb2.NodeRegistration(
                        node_id=self.node_id,
                        ip=self.node_ip,
                        cpu_score=self.cpu_score,
                        gpu_score=self.gpu_score
                    ))
                    if response.success:
                        self.registered = True
                        logger.info(f"Node {self.node_id} registered to Node Pool successfully.")
                    else:
                        logger.warning(f"Failed to register node {self.node_id} to Node Pool, retrying...")
            except grpc.RpcError as e:
                logger.error(f"Error registering to Node Pool: {e}, retrying...")
            time.sleep(5)  # 等待5秒後重試

    def ConnectToMaster(self, request, context):
        if not self.master_connected:
            master_address = request.master_address
            try:
                with grpc.insecure_channel(master_address) as channel:
                    stub = rpc_pb2_grpc.MasterNodeServiceStub(channel)
                    response = stub.ConnectNode(rpc_pb2.ConnectNodeRequest(
                        node_id=self.node_id,
                        ip=self.node_ip
                    ))
                if response.success:
                    self.master_connected = True
                    logger.info(f"Node {self.node_id} connected to master at {master_address}")
                    return rpc_pb2.ConnectToMasterResponse(success=True, message="Connected to master")
                else:
                    logger.error(f"Node {self.node_id} failed to connect to master: {response.message}")
                    return rpc_pb2.ConnectToMasterResponse(success=False, message=response.message)
            except grpc.RpcError as e:
                logger.error(f"Error connecting to master: {e}")
                return rpc_pb2.ConnectToMasterResponse(success=False, message=str(e))
        else:
            return rpc_pb2.ConnectToMasterResponse(success=True, message="Already connected to master")

def serve(node_id, node_ip, cpu_score, gpu_score):
    worker = WorkerNodeServiceServicer(node_id, node_ip, cpu_score, gpu_score)
    server = grpc.server(futures.ThreadPoolExecutor(max_workers=10))
    rpc_pb2_grpc.add_WorkerNodeServiceServicer_to_server(worker, server)
    server.add_insecure_port(f'[::]:{50050 + int(node_id)}')
    logger.info(f"Worker Node {node_id} is running...")
    worker.RegisterToPool()  # 註冊到節點池
    server.start()
    server.wait_for_termination()

if __name__ == "__main__":
    # 示例節點信息
    serve("1", "192.168.1.1", 5000, 4000)