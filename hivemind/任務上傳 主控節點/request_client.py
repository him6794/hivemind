import grpc
import nodepool_pb2
import nodepool_pb2_grpc

def get_node(cpu_score, gpu_score):
    with grpc.insecure_channel('localhost:50051') as channel:
        stub = nodepool_pb2_grpc.NodePoolStub(channel)
        node_request = nodepool_pb2.NodeRequest(
            cpu_score=cpu_score,
            gpu_score=gpu_score
        )
        try:
            response = stub.Get(node_request)
            print(f"Got node: IP={response.ip}, CPU={response.cpu_score}, GPU={response.gpu_score}")
        except grpc.RpcError as e:
            print(f"Error: {e.details()}")

if __name__ == '__main__':
    # 模擬請求節點
    get_node(200, 700)  # 請求一個符合條件的節點