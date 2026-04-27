#!/usr/bin/env python3
"""
測試 HiveMind 更新檢查功能
"""
import requests
import json
import hashlib
import os
import tempfile
from update_manager import UpdateManager

def test_update_check():
    """測試更新檢查功能"""
    print("=" * 60)
    print("HiveMind 更新測試")
    print("=" * 60)
    
    # 設定更新服務器 URL
    update_server_url = "https://hivemind.jack0916295614.workers.dev"
    
    print(f"📡 連接到更新服務器: {update_server_url}")
    
    # 測試 1: 檢查 manifest 端點
    print("\n🔍 測試 1: 檢查 Worker manifest")
    try:
        response = requests.get(f"{update_server_url}/worker/manifest")
        response.raise_for_status()
        manifest = response.json()
        
        print("✅ Worker manifest 獲取成功!")
        print(f"   最新版本: {manifest.get('latest', 'None')}")
        print(f"   可用版本數: {len(manifest.get('versions', {}))}")
        
        if manifest.get('versions'):
            for version, info in manifest['versions'].items():
                print(f"   📦 版本 {version}:")
                for artifact in info.get('artifacts', []):
                    print(f"      - {artifact['os']}/{artifact['arch']}: {artifact['filename']} ({artifact.get('size', 0)} bytes)")
        
    except Exception as e:
        print(f"❌ Worker manifest 檢查失敗: {e}")
        return False
    
    # 測試 2: 檢查 Master manifest  
    print("\n🔍 測試 2: 檢查 Master manifest")
    try:
        response = requests.get(f"{update_server_url}/master/manifest")
        response.raise_for_status()
        manifest = response.json()
        
        print("✅ Master manifest 獲取成功!")
        print(f"   最新版本: {manifest.get('latest', 'None')}")
        print(f"   可用版本數: {len(manifest.get('versions', {}))}")
        
    except Exception as e:
        print(f"❌ Master manifest 檢查失敗: {e}")
    
    # 測試 3: 使用 UpdateManager 檢查更新
    print("\n🔍 測試 3: 使用 UpdateManager 檢查更新")
    try:
        updater = UpdateManager(
            update_server_url=update_server_url,
            channel="worker", 
            current_version="0.9.0"  # 假設當前版本較舊
        )
        
        update_info = updater.check_for_updates()
        if update_info:
            print("✅ 發現可用更新!")
            print(f"   新版本: {update_info['version']}")
            print(f"   檔案名: {update_info['filename']}")
            print(f"   檔案大小: {update_info['size']} bytes")
            print(f"   下載連結: {update_info['download_url']}")
            print(f"   SHA256: {update_info['sha256']}")
            
            # 測試 4: 下載更新檔案
            if update_info['download_url']:
                print("\n🔍 測試 4: 下載更新檔案")
                
                def progress_callback(downloaded, total):
                    if total > 0:
                        percent = (downloaded / total) * 100
                        print(f"   📥 下載進度: {percent:.1f}% ({downloaded}/{total} bytes)")
                
                downloaded_file = updater.download_update(update_info, progress_callback)
                if downloaded_file:
                    print("✅ 檔案下載成功!")
                    print(f"   臨時檔案路徑: {downloaded_file}")
                    
                    # 驗證檔案內容
                    with open(downloaded_file, 'rb') as f:
                        content = f.read()
                        actual_hash = hashlib.sha256(content).hexdigest()
                        
                    print(f"   檔案大小: {len(content)} bytes")
                    print(f"   計算的 SHA256: {actual_hash}")
                    print(f"   預期的 SHA256: {update_info['sha256']}")
                    
                    if actual_hash == update_info['sha256']:
                        print("✅ SHA256 驗證通過!")
                        
                        # 如果是文本檔案，顯示內容
                        try:
                            content_str = content.decode('utf-8')
                            print("   📄 檔案內容:")
                            for line in content_str.split('\n')[:5]:  # 只顯示前5行
                                print(f"      {line}")
                        except:
                            print("   📄 檔案為二進位格式")
                    else:
                        print("❌ SHA256 驗證失敗!")
                    
                    # 清理臨時檔案
                    os.unlink(downloaded_file)
                    print("   🗑️ 臨時檔案已清理")
                else:
                    print("❌ 檔案下載失敗")
            else:
                print("⚠️ 沒有可用的下載連結")
                
        else:
            print("ℹ️ 沒有可用更新 (可能已是最新版本)")
            
    except Exception as e:
        print(f"❌ UpdateManager 測試失敗: {e}")
        import traceback
        traceback.print_exc()
    
    # 測試 5: 測試平台資訊檢測
    print("\n🔍 測試 5: 平台資訊檢測")
    updater = UpdateManager(update_server_url)
    platform_info = updater._get_platform_info()
    print(f"   檢測到的作業系統: {platform_info['os']}")
    print(f"   檢測到的架構: {platform_info['arch']}")
    
    print("\n" + "=" * 60)
    print("測試完成!")
    print("=" * 60)

if __name__ == "__main__":
    test_update_check()