#!/usr/bin/env python3
"""
HiveMind 簡單更新檢查器
"""
from update_manager import UpdateManager
import sys

def main():
    # 設定參數
    update_server = "https://hivemind.jack0916295614.workers.dev"
    channel = "worker"  # 或 "master"
    current_version = "0.9.0"  # 你的當前版本
    
    print(f"🔄 檢查 {channel} 更新...")
    
    updater = UpdateManager(update_server, channel, current_version)
    update_info = updater.check_for_updates()
    
    if update_info:
        print(f"🆕 發現新版本: {update_info['version']}")
        print(f"📦 檔案: {update_info['filename']} ({update_info['size']} bytes)")
        
        response = input("是否要下載更新? (y/n): ")
        if response.lower() == 'y':
            print("⬇️ 開始下載...")
            
            def show_progress(downloaded, total):
                if total > 0:
                    percent = (downloaded / total) * 100
                    print(f"\r進度: {percent:.1f}%", end="", flush=True)
            
            file_path = updater.download_update(update_info, show_progress)
            if file_path:
                print(f"\n✅ 下載完成: {file_path}")
                print("🔧 請手動安裝更新")
            else:
                print("\n❌ 下載失敗")
    else:
        print("✅ 已是最新版本")

if __name__ == "__main__":
    main()