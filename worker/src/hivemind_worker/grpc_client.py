"""gRPC client initialization utilities for WorkerNode."""

from __future__ import annotations

def init_grpc(worker):
    try:
        import grpc
        worker.channel = grpc.insecure_channel(worker.master_address)
        grpc.channel_ready_future(worker.channel).result(timeout=10)

        # Delay import of generated stubs
        try:
            from . import nodepool_pb2_grpc as pb2_grpc
        except Exception:
            import nodepool_pb2_grpc as pb2_grpc

        worker.user_stub = pb2_grpc.UserServiceStub(worker.channel)
        worker.node_stub = pb2_grpc.NodeManagerServiceStub(worker.channel)
        worker.master_stub = pb2_grpc.MasterNodeServiceStub(worker.channel)
        # Worker service stub (for uploading logs/results to node pool)
        try:
            worker.worker_stub = pb2_grpc.WorkerNodeServiceStub(worker.channel)
        except Exception:
            worker.worker_stub = None

        worker._log(f"Connected to master at {worker.master_address}")
        return
    except Exception as e:
        worker._log(f"gRPC connection failed: {e}", level=40)
        worker.channel = None
        worker.user_stub = None
        worker.node_stub = None
        worker.master_stub = None
        worker.worker_stub = None
