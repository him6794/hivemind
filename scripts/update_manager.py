"""
HiveMind 自動更新模組
"""
import requests
import os
import hashlib
import shutil
import tempfile
from urllib.parse import urlparse

class UpdateManager:
    def __init__(self, update_server_url, channel='worker', current_version=None):
        self.update_server_url = update_server_url.rstrip('/')
        self.channel = channel
        self.current_version = current_version
    
    def check_for_updates(self):
        """檢查是否有可用更新"""
        try:
            response = requests.get(f'{self.update_server_url}/{self.channel}/manifest')
            response.raise_for_status()
            manifest = response.json()
            
            latest_version = manifest.get('latest')
            if not latest_version:
                return None
                
            if self.current_version and latest_version == self.current_version:
                return None  # 已是最新版本
                
            # 取得對應平台的下載資訊
            versions = manifest.get('versions', {})
            if latest_version in versions:
                artifacts = versions[latest_version].get('artifacts', [])
                platform_info = self._get_platform_info()
                
                for artifact in artifacts:
                    if (artifact.get('os') == platform_info['os'] and 
                        artifact.get('arch') == platform_info['arch']):
                        return {
                            'version': latest_version,
                            'download_url': artifact['download_url'],
                            'filename': artifact['filename'],
                            'size': artifact.get('size', 0),
                            'sha256': artifact.get('sha256', '')
                        }
            return None
        except Exception as e:
            print(f"檢查更新時發生錯誤: {e}")
            return None
    
    def download_update(self, update_info, progress_callback=None):
        """下載更新檔案"""
        try:
            response = requests.get(update_info['download_url'], stream=True)
            response.raise_for_status()
            
            temp_file = tempfile.NamedTemporaryFile(delete=False)
            total_size = int(response.headers.get('content-length', 0))
            downloaded = 0
            
            hash_sha256 = hashlib.sha256()
            
            for chunk in response.iter_content(chunk_size=8192):
                if chunk:
                    temp_file.write(chunk)
                    hash_sha256.update(chunk)
                    downloaded += len(chunk)
                    
                    if progress_callback:
                        progress_callback(downloaded, total_size)
            
            temp_file.close()
            
            # 驗證檔案完整性
            if update_info.get('sha256'):
                calculated_hash = hash_sha256.hexdigest()
                if calculated_hash != update_info['sha256']:
                    os.unlink(temp_file.name)
                    raise ValueError("檔案 SHA256 驗證失敗")
            
            return temp_file.name
        except Exception as e:
            print(f"下載更新時發生錯誤: {e}")
            return None
    
    def _get_platform_info(self):
        """取得當前平台資訊"""
        import platform
        
        system = platform.system().lower()
        machine = platform.machine().lower()
        
        # 標準化作業系統名稱
        os_map = {
            'windows': 'windows',
            'linux': 'linux', 
            'darwin': 'darwin'
        }
        
        # 標準化架構名稱
        arch_map = {
            'x86_64': 'x86_64',
            'amd64': 'x86_64',
            'arm64': 'arm64',
            'aarch64': 'arm64',
            'armv7l': 'armv7'
        }
        
        return {
            'os': os_map.get(system, system),
            'arch': arch_map.get(machine, machine)
        }

# 使用範例
if __name__ == "__main__":
    updater = UpdateManager(
        update_server_url="https://your-worker-domain.com",
        channel="worker",
        current_version="1.0.0"
    )
    
    update_info = updater.check_for_updates()
    if update_info:
        print(f"發現新版本: {update_info['version']}")
        
        def progress_callback(downloaded, total):
            if total > 0:
                percent = (downloaded / total) * 100
                print(f"下載進度: {percent:.1f}% ({downloaded}/{total} bytes)")
        
        downloaded_file = updater.download_update(update_info, progress_callback)
        if downloaded_file:
            print(f"更新檔案已下載至: {downloaded_file}")
            # 在這裡執行安裝邏輯
    else:
        print("目前已是最新版本")