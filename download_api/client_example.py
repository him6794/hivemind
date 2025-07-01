import requests
import platform
import os
import json

class HiveMindUpdater:
    def __init__(self, api_base_url="http://localhost:5000/api"):
        self.api_base_url = api_base_url
        self.current_version = "1.0.0"  # 當前客戶端版本
        self.platform = self.detect_platform()
        self.components = ["worker", "master"]  # 支援的組件
    
    def detect_platform(self):
        """檢測當前平台"""
        system = platform.system().lower()
        if system == "windows":
            return "windows"
        elif system == "darwin":
            return "macos"
        elif system == "linux":
            return "linux"
        else:
            return "windows"  # 默認
    
    def check_update(self, component="worker"):
        """檢查指定組件是否有更新"""
        try:
            response = requests.post(f"{self.api_base_url}/check-update", 
                                   json={
                                       "current_version": self.current_version,
                                       "platform": self.platform,
                                       "component": component
                                   })
            
            if response.status_code == 200:
                return response.json()
            else:
                print(f"檢查 {component} 組件更新失敗: {response.status_code}")
                return None
                
        except Exception as e:
            print(f"檢查 {component} 組件更新錯誤: {e}")
            return None
    
    def download_component(self, component, version=None, save_path="./", force=True):
        """下載指定組件（支援強制下載）"""
        try:
            # 構建下載URL
            if version:
                url = f"{self.api_base_url}/download/{component}/{version}"
            else:
                url = f"{self.api_base_url}/download/{component}"
            
            params = {"platform": self.platform}
            
            response = requests.get(url, params=params, stream=True)
            
            if response.status_code == 200:
                # 從Content-Disposition獲取文件名，否則使用默認名稱
                filename = f"hivemind-{component}.zip"
                content_disposition = response.headers.get('Content-Disposition')
                if content_disposition and 'filename=' in content_disposition:
                    filename = content_disposition.split('filename=')[-1].strip('"')
                
                file_path = os.path.join(save_path, filename)
                
                # 如果文件已存在且不是強制下載，詢問是否覆蓋
                if os.path.exists(file_path) and not force:
                    choice = input(f"文件 {filename} 已存在，是否覆蓋？(y/n): ")
                    if choice.lower() != 'y':
                        print("下載已取消")
                        return None
                
                # 下載文件
                total_size = int(response.headers.get('content-length', 0))
                downloaded = 0
                
                with open(file_path, 'wb') as f:
                    for chunk in response.iter_content(chunk_size=8192):
                        if chunk:
                            f.write(chunk)
                            downloaded += len(chunk)
                            
                            # 顯示下載進度
                            if total_size > 0:
                                progress = (downloaded / total_size) * 100
                                print(f"\r下載進度: {progress:.1f}% ({downloaded}/{total_size} bytes)", end='')
                
                print(f"\n下載完成: {file_path}")
                return file_path
            else:
                error_msg = f"下載 {component} 組件失敗: {response.status_code}"
                try:
                    error_data = response.json()
                    if 'message' in error_data:
                        error_msg += f" - {error_data['message']}"
                except:
                    pass
                print(error_msg)
                return None
                
        except Exception as e:
            print(f"下載 {component} 組件錯誤: {e}")
            return None
    
    def download_latest(self, save_path="./", force=True):
        """下載所有組件的最新版本"""
        results = {}
        for component in self.components:
            print(f"下載 {component} 組件...")
            file_path = self.download_component(component, save_path=save_path, force=force)
            results[component] = file_path
        return results
    
    def download_all_components(self, version=None, save_path="./", force=True):
        """下載指定版本的所有組件"""
        results = {}
        for component in self.components:
            print(f"下載 {component} 組件 (版本: {version or '最新'})...")
            file_path = self.download_component(component, version=version, save_path=save_path, force=force)
            results[component] = file_path
        return results
    
    def get_version_info(self):
        """獲取版本信息"""
        try:
            response = requests.get(f"{self.api_base_url}/version/info")
            
            if response.status_code == 200:
                return response.json()
            else:
                print(f"獲取版本信息失敗: {response.status_code}")
                return None
                
        except Exception as e:
            print(f"獲取版本信息錯誤: {e}")
            return None

# 使用示例
if __name__ == "__main__":
    updater = HiveMindUpdater()
    
    print("=== HiveMind 組件下載器 ===")
    print(f"當前版本: {updater.current_version}")
    print(f"檢測到的平台: {updater.platform}")
    print(f"支援的組件: {', '.join(updater.components)}")
    print()
    
    # 獲取版本信息
    print("1. 獲取版本信息...")
    version_info = updater.get_version_info()
    if version_info:
        print(json.dumps(version_info, indent=2, ensure_ascii=False))
    print()
    
    # 檢查各組件更新
    print("2. 檢查各組件更新狀態...")
    for component in updater.components:
        print(f"檢查 {component} 組件...")
        update_info = updater.check_update(component)
        if update_info:
            has_update = update_info.get('has_update', False)
            latest_version = update_info.get('latest_version', 'unknown')
            print(f"  {component}: {'有更新' if has_update else '已是最新'} (最新版本: {latest_version})")
        else:
            print(f"  {component}: 檢查失敗")
    print()
    
    # 選擇下載方式
    print("3. 選擇下載方式:")
    print("  1 - 下載所有組件的最新版本")
    print("  2 - 下載指定組件")
    print("  3 - 下載指定版本的所有組件")
    print("  4 - 下載指定組件的指定版本")
    print("  0 - 退出")
    
    choice = input("請選擇 (0-4): ").strip()
    
    if choice == "1":
        print("\n開始下載所有組件的最新版本...")
        results = updater.download_latest(force=True)
        print("\n下載結果:")
        for component, file_path in results.items():
            status = "成功" if file_path else "失敗"
            print(f"  {component}: {status}")
            if file_path:
                print(f"    文件: {file_path}")
    
    elif choice == "2":
        print(f"\n可用組件: {', '.join(updater.components)}")
        component = input("請輸入組件名稱: ").strip()
        if component in updater.components:
            print(f"開始下載 {component} 組件...")
            file_path = updater.download_component(component, force=True)
            if file_path:
                print(f"下載成功: {file_path}")
            else:
                print("下載失敗")
        else:
            print(f"無效的組件名稱: {component}")
    
    elif choice == "3":
        version = input("請輸入版本號 (例如: 1.0.0): ").strip()
        if version:
            print(f"\n開始下載版本 {version} 的所有組件...")
            results = updater.download_all_components(version=version, force=True)
            print("\n下載結果:")
            for component, file_path in results.items():
                status = "成功" if file_path else "失敗"
                print(f"  {component}: {status}")
                if file_path:
                    print(f"    文件: {file_path}")
        else:
            print("版本號不能為空")
    
    elif choice == "4":
        print(f"\n可用組件: {', '.join(updater.components)}")
        component = input("請輸入組件名稱: ").strip()
        version = input("請輸入版本號 (例如: 1.0.0): ").strip()
        
        if component in updater.components and version:
            print(f"開始下載 {component} 組件版本 {version}...")
            file_path = updater.download_component(component, version=version, force=True)
            if file_path:
                print(f"下載成功: {file_path}")
            else:
                print("下載失敗")
        else:
            print("組件名稱或版本號無效")
    
    elif choice == "0":
        print("退出")
    
    else:
        print("無效選擇")
    
    print("\n操作完成。")
