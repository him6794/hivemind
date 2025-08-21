import ctypes
import os
import sys
import requests
import zipfile
import tempfile
from pathlib import Path

class WireGuardManager:
    def __init__(self, lib_path):
        """
        初始化 WireGuard 管理器
        
        Args:
            lib_path: WireGuard Go 庫的路徑
        """
        # 檢查並確保 wintun.dll 存在
        self.ensure_wintun_dll()
        
        # 獲取庫所在目錄
        lib_dir = os.path.dirname(os.path.abspath(lib_path))
        
        # 如果是Windows，將庫所在目錄添加到PATH環境變量中
        if sys.platform == "win32":
            os.environ['PATH'] = lib_dir + os.pathsep + os.environ.get('PATH', '')
        
        try:
            self.lib = ctypes.CDLL(lib_path)
            
            # 定義函數簽名
            self.lib.StartWireGuard.argtypes = [ctypes.c_char_p]
            self.lib.StartWireGuard.restype = ctypes.c_char_p
            
            self.lib.StopWireGuard.argtypes = [ctypes.c_char_p]
            self.lib.StopWireGuard.restype = ctypes.c_char_p
            
            self.lib.GetStatus.argtypes = [ctypes.c_char_p]
            self.lib.GetStatus.restype = ctypes.c_char_p
            
        except Exception as e:
            print(f"加載 WireGuard 庫失敗: {e}")
            sys.exit(1)
    
    def ensure_wintun_dll(self):
        """確保 wintun.dll 存在於當前目錄"""
        if sys.platform != "win32":
            return  # 只在 Windows 上需要
        
        wintun_path = os.path.join(os.path.dirname(__file__), "wintun.dll")
        if os.path.exists(wintun_path):
            return
        
        print("wintun.dll 未找到，嘗試下載...")
        
        # 從官方來源下載 wintun.dll
        try:
            # 這裡使用 WireGuard 官方提供的下載鏈接
            url = "https://download.wireguard.com/windows-client/wireguard-installer.exe"
            
            # 創建臨時文件
            temp_dir = tempfile.gettempdir()
            temp_exe = os.path.join(temp_dir, "wireguard_temp.exe")
            
            # 下載文件
            print("正在下載 WireGuard 安裝程序...")
            response = requests.get(url, stream=True)
            with open(temp_exe, 'wb') as f:
                for chunk in response.iter_content(chunk_size=8192):
                    if chunk:
                        f.write(chunk)
            
            # 使用 7z 提取 wintun.dll（如果已安裝）
            try:
                import subprocess
                # 嘗試使用 7z 提取
                subprocess.run([
                    "7z", "e", temp_exe, "wintun.dll", 
                    f"-o{os.path.dirname(__file__)}", "-y"
                ], check=True, capture_output=True)
                print("成功提取 wintun.dll")
            except (subprocess.CalledProcessError, FileNotFoundError):
                # 如果 7z 不可用，提示用戶手動安裝
                print("無法自動提取 wintun.dll，請手動安裝 WireGuard for Windows")
                print("或從 https://www.wireguard.com/install/ 下載並安裝")
                input("按 Enter 鍵繼續...")
                
        except Exception as e:
            print(f"下載 wintun.dll 失敗: {e}")
            print("請手動從 https://www.wireguard.com/install/ 下載並安裝 WireGuard for Windows")
            input("按 Enter 鍵繼續...")
    
    def start_connection(self, config_path):
        """
        啟動 WireGuard 連接
        
        Args:
            config_path: WireGuard 配置文件路徑
            
        Returns:
            str: 操作結果消息
        """
        if not os.path.exists(config_path):
            return f"錯誤: 配置文件不存在 {config_path}"
            
        result = self.lib.StartWireGuard(config_path.encode('utf-8'))
        return result.decode('utf-8')
    
    def stop_connection(self, config_path):
        """
        停止 WireGuard 連接
        
        Args:
            config_path: WireGuard 配置文件路徑
            
        Returns:
            str: 操作結果消息
        """
        result = self.lib.StopWireGuard(config_path.encode('utf-8'))
        return result.decode('utf-8')
    
    def get_status(self, config_path):
        """
        獲取連接狀態
        
        Args:
            config_path: WireGuard 配置文件路徑
            
        Returns:
            str: 連接狀態 (CONNECTED 或 DISCONNECTED)
        """
        result = self.lib.GetStatus(config_path.encode('utf-8'))
        return result.decode('utf-8')

# 使用示例
if __name__ == "__main__":
    # 加載庫 (假設庫文件命名為 wireguardlib.dll 或 wireguardlib.so)
    if sys.platform == "win32":
        lib_name = "wireguardlib.dll"
    else:
        lib_name = "wireguardlib.so"
    
    lib_path = os.path.join(os.path.dirname(__file__), lib_name)
    
    if not os.path.exists(lib_path):
        print(f"錯誤: 找不到庫文件 {lib_path}")
        print("請先編譯 Go 代碼為共享庫")
        sys.exit(1)
    
    # 創建管理器實例
    wg_manager = WireGuardManager(lib_path)
    
    # 配置文件路徑
    config_path = input("請輸入 WireGuard 配置文件路徑: ").strip()
    
    if not os.path.exists(config_path):
        print(f"錯誤: 配置文件不存在 {config_path}")
        sys.exit(1)
    
    # 啟動連接
    print("正在啟動 WireGuard 連接...")
    result = wg_manager.start_connection(config_path)
    print(f"結果: {result}")
    
    # 檢查狀態
    status = wg_manager.get_status(config_path)
    print(f"狀態: {status}")
    
    if "SUCCESS" in result:
        input("按 Enter 鍵停止連接...")
        
        # 停止連接
        print("正在停止 WireGuard 連接...")
        result = wg_manager.stop_connection(config_path)
        print(f"結果: {result}")