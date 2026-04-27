"""Docker initialization utilities for WorkerNode.

Encapsulates Docker client creation, connectivity test, and image presence.
This function mutates the provided `worker` to set docker_client,
docker_available, docker_status and logs via worker._log.
"""

from __future__ import annotations

def cleanup_old_containers(worker):
    """清理所有舊的 HiveMind 相關容器（包括運行中和停止的）"""
    try:
        if not hasattr(worker, 'docker_client') or worker.docker_client is None:
            worker._log("[Docker Cleanup] Docker client not available, skipping cleanup")
            return
        
        worker._log("[Docker Cleanup] Checking for old HiveMind containers...")
        
        # 獲取所有容器（包括停止的）
        all_containers = worker.docker_client.containers.list(all=True)
        hivemind_containers = [
            c for c in all_containers 
            if c.name.startswith('task-')  # 匹配 task-{task_id}-{random} 格式
        ]
        
        if not hivemind_containers:
            worker._log("[Docker Cleanup] No old HiveMind containers found")
            return
        
        worker._log(f"[Docker Cleanup] Found {len(hivemind_containers)} old container(s), cleaning up...")
        
        for container in hivemind_containers:
            try:
                container_name = container.name
                container_status = container.status
                
                # 如果容器正在運行，先停止
                if container_status == 'running':
                    worker._log(f"[Docker Cleanup] Stopping running container: {container_name}")
                    container.stop(timeout=5)
                
                # 移除容器
                worker._log(f"[Docker Cleanup] Removing container: {container_name} (status: {container_status})")
                container.remove(force=True)
                worker._log(f"[Docker Cleanup] Successfully removed container: {container_name}")
                
            except Exception as e:
                worker._log(f"[Docker Cleanup] Failed to remove container {container.name}: {e}", level=30)
        
        worker._log("[Docker Cleanup] Container cleanup completed")
        
    except Exception as e:
        worker._log(f"[Docker Cleanup] Error during container cleanup: {e}", level=30)

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
        
        # 清理所有舊容器
        cleanup_old_containers(worker)

    except Exception as e:
        worker._log(f"[Worker] Docker initialization failed: {e}", level=30)
        worker.docker_available = False
        worker.docker_client = None
        worker.docker_status = "unavailable"
