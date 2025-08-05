import os
from dotenv import load_dotenv

# 載入 .env 文件
env_path = os.path.join(os.path.dirname(__file__), '.env')
load_dotenv(dotenv_path=env_path)

class Config:
    # === 資料庫配置 ===
    DB_PATH = os.getenv('DB_PATH', os.path.join(os.path.dirname(__file__), 'db', 'nodepool.db'))
    
    # === JWT 安全配置 ===
    SECRET_KEY = os.getenv('SECRET_KEY', 'dev-secret-key-change-in-production')
    TOKEN_EXPIRY = int(os.getenv('TOKEN_EXPIRY', '60'))  # 分鐘
    
    # === 電子郵件服務配置 ===
    RESEND_API_KEY = os.getenv('RESEND_API_KEY', '')
    FROM_EMAIL = os.getenv('FROM_EMAIL', 'noreply@hivemind.justin0711.com')
    BASE_URL = os.getenv('BASE_URL', 'http://localhost:5000')
    
    # === Web 服務配置 ===
    FLASK_HOST = os.getenv('FLASK_HOST', '0.0.0.0')
    FLASK_PORT = int(os.getenv('FLASK_PORT', '5000'))
    FLASK_DEBUG = os.getenv('FLASK_DEBUG', 'False').lower() in ('true', '1', 'yes')
    FLASK_ENV = os.getenv('FLASK_ENV', 'production')
    
    # === Cloudflare Turnstile 配置 ===
    TURNSTILE_SECRET_KEY = os.getenv('TURNSTILE_SECRET_KEY', '')
    TURNSTILE_SITE_KEY = os.getenv('TURNSTILE_SITE_KEY', '0x4AAAAAABkJQaM8US5k-aWw')
    
    # === VPN 服務配置 ===
    VPN_SERVICE_HOST = os.getenv('VPN_SERVICE_HOST', '127.0.0.1')
    VPN_SERVICE_PORT = int(os.getenv('VPN_SERVICE_PORT', '5008'))
    VPN_SERVICE_TIMEOUT = int(os.getenv('VPN_SERVICE_TIMEOUT', '10'))
    
    # === 安全配置 ===
    RATE_LIMIT_SECONDS = int(os.getenv('RATE_LIMIT_SECONDS', '5'))
    MAX_CLIENTS_PER_USER = int(os.getenv('MAX_CLIENTS_PER_USER', '5'))
    JWT_SECRET_KEY = os.getenv('JWT_SECRET_KEY', 'jwt-secret-change-this')
    TOKEN_EXPIRATION_HOURS = int(os.getenv('TOKEN_EXPIRATION_HOURS', '24'))
    
    # === 儲存配置 ===
    LOG_DIR = os.getenv('LOG_DIR', '/mnt/myusb/hivemind/vpn/logs')
    UPLOAD_DIR = os.getenv('UPLOAD_DIR', '/mnt/myusb/hivemind/uploads')
    TASK_STORAGE_PATH = os.getenv('TASK_STORAGE_PATH', '/mnt/myusb/hivemind/task_storage')
    MAX_FILE_SIZE = int(os.getenv('MAX_FILE_SIZE', '10485760'))  # 10MB
    
    # === 節點池服務配置 ===
    GRPC_SERVER_HOST = os.getenv('GRPC_SERVER_HOST', '0.0.0.0')
    GRPC_SERVER_PORT = int(os.getenv('GRPC_SERVER_PORT', '50051'))
    REDIS_HOST = os.getenv('REDIS_HOST', 'localhost')
    REDIS_PORT = int(os.getenv('REDIS_PORT', '6379'))
    REDIS_DB = int(os.getenv('REDIS_DB', '0'))
    
    # === 開發/測試配置 ===
    DEV_MODE = os.getenv('DEV_MODE', 'False').lower() in ('true', '1', 'yes')
    SKIP_EMAIL_VERIFICATION = os.getenv('SKIP_EMAIL_VERIFICATION', 'False').lower() in ('true', '1', 'yes')
    
    @classmethod
    def is_development(cls):
        """檢查是否為開發環境"""
        return cls.FLASK_ENV == 'development' or cls.DEV_MODE or cls.FLASK_DEBUG
    
    @classmethod
    def get_database_url(cls):
        """獲取完整的資料庫路徑"""
        if os.path.isabs(cls.DB_PATH):
            return cls.DB_PATH
        else:
            return os.path.join(os.path.dirname(__file__), cls.DB_PATH)
    
    @classmethod
    def validate_config(cls):
        """驗證必要的配置是否已設定"""
        required_configs = []
        
        if not cls.SECRET_KEY or cls.SECRET_KEY == 'dev-secret-key-change-in-production':
            required_configs.append('SECRET_KEY')
        
        if not cls.RESEND_API_KEY and not cls.SKIP_EMAIL_VERIFICATION:
            required_configs.append('RESEND_API_KEY (電子郵件功能需要)')
        
        if not cls.TURNSTILE_SECRET_KEY and not cls.is_development():
            required_configs.append('TURNSTILE_SECRET_KEY (生產環境需要)')
        
        return required_configs
