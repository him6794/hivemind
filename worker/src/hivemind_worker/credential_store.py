"""
憑證儲存模組 - 安全地儲存和載入用戶帳密
使用 AES 加密儲存，密鑰基於機器特徵生成
"""
import json
import os
from pathlib import Path
import sys
from base64 import b64encode, b64decode
from hashlib import sha256
from platform import node, system
try:
    from cryptography.hazmat.primitives.ciphers import Cipher, algorithms, modes
    from cryptography.hazmat.backends import default_backend
    from cryptography.hazmat.primitives import padding
    CRYPTO_AVAILABLE = True
except ImportError:
    CRYPTO_AVAILABLE = False

def _get_credential_file() -> Path:
    """決定憑證/設定檔案位置。

    編譯版（frozen）必須以 exe 同層為準，避免寫到封裝的模組目錄（可能導致
    第二次啟動讀到不同檔案、或覆蓋掉 node_port/nodepool_address）。
    """
    home_cred = Path.home() / ".hivemind" / "worker_credentials.json"

    try:
        if getattr(sys, "frozen", False):
            exe_dir = Path(sys.executable).parent
            if exe_dir.exists() and os.access(exe_dir, os.W_OK):
                return exe_dir / "worker_credentials.json"

        module_dir = Path(__file__).parent
        if module_dir.exists() and os.access(module_dir, os.W_OK):
            return module_dir / "worker_credentials.json"
    except Exception:
        pass

    return home_cred


# 憑證檔案路徑
CREDENTIAL_FILE = _get_credential_file()


def _get_machine_key():
    """根據機器特徵生成加密密鑰"""
    # 使用主機名和系統類型生成唯一密鑰
    machine_id = f"{node()}-{system()}-hivemind-worker"
    return sha256(machine_id.encode()).digest()


def _encrypt_password(password: str) -> str:
    """使用 AES 加密密碼"""
    if not CRYPTO_AVAILABLE:
        # 如果沒有 cryptography 庫，使用簡單的 base64（不安全但至少不是明文）
        return b64encode(password.encode()).decode()
    
    key = _get_machine_key()
    iv = os.urandom(16)
    
    cipher = Cipher(algorithms.AES(key), modes.CBC(iv), backend=default_backend())
    encryptor = cipher.encryptor()
    
    # PKCS7 padding
    padder = padding.PKCS7(128).padder()
    padded_data = padder.update(password.encode()) + padder.finalize()
    
    encrypted = encryptor.update(padded_data) + encryptor.finalize()
    
    # 返回 iv + encrypted 的 base64 編碼
    return b64encode(iv + encrypted).decode()


def _decrypt_password(encrypted_password: str) -> str:
    """解密密碼"""
    if not CRYPTO_AVAILABLE:
        # 簡單 base64 解碼
        try:
            return b64decode(encrypted_password.encode()).decode()
        except:
            return ""
    
    try:
        key = _get_machine_key()
        data = b64decode(encrypted_password.encode())
        
        iv = data[:16]
        encrypted = data[16:]
        
        cipher = Cipher(algorithms.AES(key), modes.CBC(iv), backend=default_backend())
        decryptor = cipher.decryptor()
        
        padded_data = decryptor.update(encrypted) + decryptor.finalize()
        
        # PKCS7 unpadding
        unpadder = padding.PKCS7(128).unpadder()
        password = unpadder.update(padded_data) + unpadder.finalize()
        
        return password.decode()
    except Exception:
        return ""


def save_credentials(username: str, password: str, remember: bool = True):
    """
    儲存用戶憑證
    
    Args:
        username: 用戶名
        password: 密碼（會被加密）
        remember: 是否記住密碼
    """
    # 確保目錄存在
    CREDENTIAL_FILE.parent.mkdir(parents=True, exist_ok=True)

    # 讀取既有設定，避免覆蓋 node_port / nodepool_address 等其他欄位
    existing: dict = {}
    if CREDENTIAL_FILE.exists():
        try:
            with open(CREDENTIAL_FILE, 'r', encoding='utf-8') as f:
                loaded = json.load(f)
            if isinstance(loaded, dict):
                existing = loaded
        except Exception:
            existing = {}

    try:
        if remember:
            encrypted_pwd = _encrypt_password(password)
            existing["username"] = username
            existing["password"] = encrypted_pwd
            existing["remember"] = True
        else:
            # 不記住密碼：保留其他設定，只移除帳密
            existing.pop("username", None)
            existing.pop("password", None)
            existing["remember"] = False

        with open(CREDENTIAL_FILE, 'w', encoding='utf-8') as f:
            json.dump(existing, f, ensure_ascii=False, indent=2)
        
        # 設置檔案權限（僅限擁有者讀寫）
        if system() != "Windows":
            os.chmod(CREDENTIAL_FILE, 0o600)
        
        return True
    except Exception as e:
        print(f"Failed to save credentials: {e}")
        return False


def load_credentials():
    """
    載入已儲存的憑證
    
    Returns:
        tuple: (username, password, remember) 或 (None, None, False) 如果沒有儲存
    """
    if not CREDENTIAL_FILE.exists():
        return None, None, False
    
    try:
        with open(CREDENTIAL_FILE, 'r', encoding='utf-8') as f:
            credentials = json.load(f)
        
        username = credentials.get("username")
        encrypted_pwd = credentials.get("password")
        remember = credentials.get("remember", False)
        
        if not username or not encrypted_pwd:
            return None, None, False
        
        # 解密密碼
        password = _decrypt_password(encrypted_pwd)
        
        if not password:
            return None, None, False
        
        return username, password, remember
    except Exception as e:
        print(f"Failed to load credentials: {e}")
        return None, None, False


def delete_credentials():
    """刪除已儲存的憑證"""
    try:
        # 只刪除帳密欄位，保留 node_port/nodepool_address 等設定
        if not CREDENTIAL_FILE.exists():
            return True

        try:
            with open(CREDENTIAL_FILE, 'r', encoding='utf-8') as f:
                data = json.load(f)
            if not isinstance(data, dict):
                # 若格式異常，沿用舊行為直接刪除
                CREDENTIAL_FILE.unlink()
                return True
        except Exception:
            CREDENTIAL_FILE.unlink()
            return True

        data.pop("username", None)
        data.pop("password", None)
        data["remember"] = False

        with open(CREDENTIAL_FILE, 'w', encoding='utf-8') as f:
            json.dump(data, f, ensure_ascii=False, indent=2)
        return True
    except Exception as e:
        print(f"Failed to delete credentials: {e}")
        return False


def has_saved_credentials():
    """檢查是否有已儲存的憑證"""
    return CREDENTIAL_FILE.exists()
