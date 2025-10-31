"""Docker initialization utilities for WorkerNode.

Encapsulates Docker client creation, connectivity test, and image presence.
This function mutates the provided `worker` to set docker_client,
docker_available, docker_status and logs via worker._log.
"""

from __future__ import annotations

def init_docker(worker):
    try:
        worker._log("[Worker] Initializing Docker client...")
        try:
            from docker import from_env
            from docker.errors import ImageNotFound
        except Exception:
            # docker SDK not installed
            worker._log("Docker SDK not installed; marking Docker unavailable", level=30)
            worker.docker_available = False
            worker.docker_client = None
            worker.docker_status = "unavailable"
            return

        worker.docker_client = from_env(timeout=10)
        worker._log("[Worker] Docker client created, testing connection...")
        worker.docker_client.ping()
        worker.docker_available = True
        worker.docker_status = "available"
        worker._log("[Worker] Docker connection successful!")

        # Check or pull image
        try:
            worker.docker_client.images.get("justin308/hivemind-worker:latest")
            worker._log("Docker image 'justin308/hivemind-worker:latest' found")
        except ImageNotFound:
            worker._log("Docker image not found, pulling justin308/hivemind-worker:latest")
            try:
                worker.docker_client.images.pull("justin308/hivemind-worker:latest")
                worker._log("Docker image pulled successfully")
            except Exception as e:
                worker._log(f"Failed to pull docker image: {e}", level=30)

        worker._log("[Worker] Docker initialization completed successfully!")

    except Exception as e:
        worker._log(f"[Worker] Docker initialization failed: {e}", level=30)
        worker.docker_available = False
        worker.docker_client = None
        worker.docker_status = "unavailable"
