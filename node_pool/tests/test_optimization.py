#!/usr/bin/env python3
"""
NodePool 優化版本快速測試腳本
用於驗證所有優化功能是否正常運作
"""

import sys
import os
import time
import subprocess

# 添加 node_pool 到路徑
sys.path.insert(0, os.path.dirname(__file__))

def test_python_imports():
    """測試 Python 模組是否可正常導入"""
    print("=" * 60)
    print("1. 測試 Python 模組導入...")
    print("=" * 60)
    
    try:
        from auth_interceptor import AuthInterceptor
        print("✅ auth_interceptor 導入成功")
        
        from master_node_service import TaskManager
        print("✅ master_node_service 導入成功")
        
        from node_pool_server import serve
        print("✅ node_pool_server 導入成功")
        
        print("\n所有 Python 模組導入成功！\n")
        return True
    except Exception as e:
        print(f"❌ 導入失敗: {e}")
        return False

def run_python_tests():
    """運行 Python 單元測試"""
    print("=" * 60)
    print("2. 運行 Python 單元測試...")
    print("=" * 60)
    
    try:
        result = subprocess.run(
            [sys.executable, "-m", "pytest", "tests/", "-v", "--tb=short"],
            cwd=os.path.dirname(__file__),
            capture_output=True,
            text=True,
            timeout=60
        )
        
        print(result.stdout)
        if result.stderr:
            print(result.stderr)
        
        if result.returncode == 0:
            print("\n✅ 所有 Python 測試通過！\n")
            return True
        else:
            print(f"\n⚠️  某些測試失敗（返回碼: {result.returncode}）\n")
            return False
    except subprocess.TimeoutExpired:
        print("❌ 測試超時（>60秒）")
        return False
    except FileNotFoundError:
        print("⚠️  pytest 未安裝，跳過 Python 測試")
        print("   請運行: pip install pytest")
        return None
    except Exception as e:
        print(f"❌ 測試執行錯誤: {e}")
        return False

def run_go_tests():
    """運行 Go 單元測試"""
    print("=" * 60)
    print("3. 運行 Go 單元測試...")
    print("=" * 60)
    
    resource_manager_path = os.path.join(os.path.dirname(__file__), "resource_manager")
    
    if not os.path.exists(resource_manager_path):
        print("⚠️  resource_manager 目錄不存在，跳過 Go 測試")
        return None
    
    try:
        # 檢查 Go 是否安裝
        result = subprocess.run(
            ["go", "version"],
            capture_output=True,
            text=True,
            timeout=10
        )
        
        if result.returncode != 0:
            print("⚠️  Go 未安裝，跳過 Go 測試")
            return None
        
        print(f"使用 {result.stdout.strip()}\n")
        
        # 運行測試
        result = subprocess.run(
            ["go", "test", "-v"],
            cwd=resource_manager_path,
            capture_output=True,
            text=True,
            timeout=120
        )
        
        print(result.stdout)
        if result.stderr:
            print(result.stderr)
        
        if result.returncode == 0:
            print("\n✅ 所有 Go 測試通過！\n")
            return True
        else:
            print(f"\n⚠️  某些測試失敗（返回碼: {result.returncode}）\n")
            return False
            
    except subprocess.TimeoutExpired:
        print("❌ 測試超時（>120秒）")
        return False
    except FileNotFoundError:
        print("⚠️  Go 未安裝，跳過 Go 測試")
        return None
    except Exception as e:
        print(f"❌ 測試執行錯誤: {e}")
        return False

def check_redis_connection():
    """檢查 Redis 連接"""
    print("=" * 60)
    print("4. 檢查 Redis 連接...")
    print("=" * 60)
    
    try:
        import redis
        from config import Config
        
        client = redis.Redis(
            host=Config.REDIS_HOST,
            port=Config.REDIS_PORT,
            db=Config.REDIS_DB,
            decode_responses=True,
            socket_timeout=3
        )
        
        client.ping()
        print(f"✅ Redis 連接成功 ({Config.REDIS_HOST}:{Config.REDIS_PORT})")
        
        # 測試基本操作
        client.set("test_key", "test_value", ex=10)
        value = client.get("test_key")
        
        if value == "test_value":
            print("✅ Redis 讀寫測試成功\n")
            return True
        else:
            print("⚠️  Redis 讀寫測試失敗\n")
            return False
            
    except ImportError:
        print("⚠️  redis-py 未安裝，跳過 Redis 檢查")
        print("   請運行: pip install redis\n")
        return None
    except Exception as e:
        print(f"❌ Redis 連接失敗: {e}")
        print("   請確保 Redis 正在運行\n")
        return False

def print_summary(results):
    """打印測試總結"""
    print("=" * 60)
    print("測試總結")
    print("=" * 60)
    
    total = len(results)
    passed = sum(1 for r in results.values() if r is True)
    failed = sum(1 for r in results.values() if r is False)
    skipped = sum(1 for r in results.values() if r is None)
    
    for name, result in results.items():
        if result is True:
            status = "✅ PASS"
        elif result is False:
            status = "❌ FAIL"
        else:
            status = "⚠️  SKIP"
        print(f"{status}  {name}")
    
    print("\n" + "=" * 60)
    print(f"總計: {total} | 通過: {passed} | 失敗: {failed} | 跳過: {skipped}")
    print("=" * 60)
    
    if failed > 0:
        print("\n⚠️  某些測試失敗，請檢查上方錯誤信息")
        return False
    elif passed == total:
        print("\n🎉 所有測試通過！優化功能運作正常！")
        return True
    else:
        print("\n✅ 已通過的測試均正常，部分測試已跳過")
        return True

def main():
    """主函數"""
    print("\n" + "=" * 60)
    print("HiveMind NodePool 優化版本測試")
    print("=" * 60)
    print()
    
    results = {}
    
    # 1. Python 導入測試
    results["Python 模組導入"] = test_python_imports()
    time.sleep(1)
    
    # 2. Python 單元測試
    results["Python 單元測試"] = run_python_tests()
    time.sleep(1)
    
    # 3. Go 單元測試
    results["Go 單元測試"] = run_go_tests()
    time.sleep(1)
    
    # 4. Redis 連接測試
    results["Redis 連接"] = check_redis_connection()
    
    # 打印總結
    success = print_summary(results)
    
    print("\n提示:")
    print("- 詳細文檔請參閱: OPTIMIZATION_README.md")
    print("- 完整報告請參閱: OPTIMIZATION_SUMMARY.md")
    print("- 啟動 NodePool: python node_pool_server.py")
    print()
    
    return 0 if success else 1

if __name__ == "__main__":
    sys.exit(main())
