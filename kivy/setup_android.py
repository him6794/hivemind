#!/usr/bin/env python3
"""
Android 版本設置腳本
"""
import os
import shutil
from pathlib import Path

def setup_android_build():
    """設置 Android 建置環境"""
    
    # 創建必要目錄
    os.makedirs("android_build", exist_ok=True)
    
    # 複製必要文件
    files_to_copy = [
        "android_worker_node.py",
        "nodepool_pb2.py",
        "nodepool_pb2_grpc.py",
        "buildozer.spec"
    ]
    
    for file in files_to_copy:
        if os.path.exists(file):
            shutil.copy2(file, "android_build/")
            print(f"Copied {file}")
        else:
            print(f"Warning: {file} not found")
    
    # 創建 main.py (Kivy 應用入口)
    main_py_content = '''
from android_worker_node import AndroidWorkerApp

if __name__ == "__main__":
    AndroidWorkerApp().run()
'''
    
    with open("android_build/main.py", "w") as f:
        f.write(main_py_content)
    
    print("Android build environment setup complete!")
    print("Next steps:")
    print("1. Install buildozer: pip install buildozer")
    print("2. cd android_build")
    print("3. buildozer android debug")
    print("4. Your APK will be in bin/ directory")

if __name__ == "__main__":
    setup_android_build()
