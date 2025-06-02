import grpc
import nodepool_pb2
import nodepool_pb2_grpc

def run():
    channel = grpc.insecure_channel('localhost:50051')
    stub = nodepool_pb2_grpc.NodePoolStub(channel)

    # 請求範例，這部分可以省略，由 UI 呼叫
    node_request = nodepool_pb2.NodeRequest(cpu_score=5000, gpu_score=4000)
    response = stub.Get(node_request)

    print(f"Node response: ip={response.ip}, cpu_score={response.cpu_score}, "
          f"gpu_score={response.gpu_score}, memory={response.memory}, "
          f"network_delay={response.network_delay}, geographic_location={response.geographic_location}")


if __name__ == '__main__':
    run()
