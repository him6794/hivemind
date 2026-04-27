"""gRPC client initialization utilities for WorkerNode."""

from __future__ import annotations

def init_grpc(worker):
    try:
        import grpc
        # Prefer nodepool_address (correct service for workers). Fall back to master_address for compatibility.
        addr = getattr(worker, 'nodepool_address', None) or getattr(worker, 'master_address', None)
        if not addr:
            raise ValueError('No nodepool address configured on worker (nodepool_address or master_address expected)')
        addr = str(addr).strip()
        timeout_s = 10
        worker._log(f"Attempting gRPC connect to nodepool address: {repr(addr)} (timeout={timeout_s}s)")
        
        # 使用普通 insecure channel（認證通過 metadata 手動添加）
        worker.channel = grpc.insecure_channel(addr)
        grpc.channel_ready_future(worker.channel).result(timeout=timeout_s)

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

        worker._log(f"Connected to nodepool at {addr}")
        return
    except Exception as e:
        try:
            import traceback
            tb = traceback.format_exc()
            target = repr(addr) if 'addr' in locals() else 'unknown'
            hint = (
                "Possible causes: nodepool server not running, wrong NODEPOOL_ADDRESS, "
                "port blocked by firewall, or no route to 10.0.0.x network."
            )
            worker._log(f"gRPC connection failed to {target}: {e}\n{hint}\n{tb}", level=40)
        except Exception:
            worker._log(f"gRPC connection failed: {e}", level=40)
        worker.channel = None
        worker.user_stub = None
        worker.node_stub = None
        worker.master_stub = None
        worker.worker_stub = None
