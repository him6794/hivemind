from os import environ
from platform import node

# 服務配置
NODE_PORT = int(environ.get("NODE_PORT", 50053))
FLASK_PORT = int(environ.get("FLASK_PORT", 5001))
MASTER_ADDRESS = environ.get("MASTER_ADDRESS", "127.0.0.1:50051")
NODE_ID = environ.get("NODE_ID", f"worker-{node().split('.')[0]}-{NODE_PORT}")

# 資源限制
MAX_TASK_FILE_SIZE_MB = 100
TASK_TIMEOUT_SECONDS = 3600
MAX_CONCURRENT_TASKS = 5
