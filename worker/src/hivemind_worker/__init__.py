"""
HiveMind Worker Node Package

Exports the main entrypoints and types after modularization.
"""

# Core orchestrator and entrypoint
from .worker_node import WorkerNode, run_worker_node

# gRPC servicer lives in its own module after refactor
from .grpc_servicer import WorkerNodeServicer

# Optional: expose web helpers for embedding
try:
	from .webapp import register_routes, start_flask
except Exception:  # pragma: no cover - optional
	register_routes = None
	start_flask = None

__all__ = [
	'WorkerNode',
	'WorkerNodeServicer',
	'run_worker_node',
	'register_routes',
	'start_flask',
]