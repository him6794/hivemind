# 新增config.py集中管理配置
import os

class Config:
    SECRET_KEY = os.environ.get('JWT_SECRET_KEY', 'your-secure-secret-key')
    # Linux下正確路徑
    DB_PATH = os.getenv("DB_PATH", r"D:\hivemind\節點池\users.db")
    REDIS_HOST = os.getenv("REDIS_HOST", "localhost")
    REDIS_PORT = int(os.getenv("REDIS_PORT", 6379))
    REDIS_DB = int(os.getenv("REDIS_DB", 0))
    TOKEN_EXPIRY = int(os.getenv("TOKEN_EXPIRY_MINUTES", 60))
    # 新增：任務存儲目錄
    TASK_STORAGE_PATH = os.getenv("TASK_STORAGE_PATH", "/mnt/myusb/hivemind/task_storage")
    REDIS_DB = int(os.getenv("REDIS_DB", 0))
    TOKEN_EXPIRY = int(os.getenv("TOKEN_EXPIRY_MINUTES", 60))
