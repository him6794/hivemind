import subprocess
import sys
import os
import shutil
import threading
import time

def build_exe():
    """使用 Nuitka 打包 worker_node.py 為 exe"""
    
    print("開始使用 Nuitka 打包工作節點...")
    
    # 確保必要的目錄存在
    output_dir = "dist"
    if os.path.exists(output_dir):
        shutil.rmtree(output_dir)
    os.makedirs(output_dir, exist_ok=True)
    
    # 簡化的 Nuitka 打包命令，避免可能的問題
    nuitka_cmd = [
        sys.executable, "-m", "nuitka",
        "--standalone",  # 獨立打包
        "--onefile",     # 打包成單一檔案
        "--windows-console-mode=attach",  # Windows 控制台模式
        "--include-package=flask",
        "--include-package=grpc",
        "--include-package=docker",
        "--include-package=psutil",
        "--include-package=concurrent.futures",
        "--include-package=zipfile",
        "--include-package=threading",
        "--include-package=datetime",
        "--include-package=secrets",
        "--include-package=tempfile",
        "--include-package=subprocess",
        "--include-package=platform",
        "--include-package=socket",
        "--include-package=uuid",
        "--include-package=webbrowser",
        "--include-data-dir=templates=templates",  # 包含模板目錄
        "--include-data-dir=static=static",        # 包含靜態檔案目錄
        "--include-data-file=run_task.sh=run_task.sh",  # 包含腳本檔案
        "--include-data-files=nodepool_pb2.py=nodepool_pb2.py",
        "--include-data-files=nodepool_pb2_grpc.py=nodepool_pb2_grpc.py",
        "--output-dir=dist",
        "--remove-output",  # 清理臨時檔案
        "--assume-yes-for-downloads",
        "--show-progress",  # 顯示進度
        "--verbose",        # 詳細輸出
        "worker_node.py"
    ]
    
    try:
        # 執行打包
        print("執行 Nuitka 打包命令...")
        print("命令:", " ".join(nuitka_cmd))
        print("\n正在打包，這可能需要幾分鐘時間...")
        print("如果長時間無反應，請按 Ctrl+C 中斷\n")
        
        # 使用實時輸出的方式執行
        process = subprocess.Popen(
            nuitka_cmd, 
            stdout=subprocess.PIPE, 
            stderr=subprocess.STDOUT,
            text=True,
            bufsize=1,
            universal_newlines=True
        )
        
        # 實時顯示輸出
        start_time = time.time()
        last_output_time = start_time
        
        while True:
            output = process.stdout.readline()
            if output == '' and process.poll() is not None:
                break
            if output:
                print(output.strip())
                last_output_time = time.time()
            
            # 檢查是否超時（30分鐘無輸出）
            if time.time() - last_output_time > 1800:  # 30分鐘
                print("\n❌ 超時：30分鐘無輸出，正在終止打包程序...")
                process.terminate()
                process.wait()
                return False
        
        # 等待程序結束
        return_code = process.poll()
        
        if return_code == 0:
            print("\n✅ 打包成功!")
            
            # 檢查生成的檔案
            exe_path = os.path.join(output_dir, "worker_node.exe")
            if os.path.exists(exe_path):
                file_size = os.path.getsize(exe_path) / 1024 / 1024
                print(f"✅ 可執行檔案已生成: {exe_path}")
                print(f"📁 檔案大小: {file_size:.1f} MB")
                print(f"⏱️  總耗時: {time.time() - start_time:.1f} 秒")
                return True
            else:
                print("❌ 未找到生成的可執行檔案")
                return False
        else:
            print(f"\n❌ 打包失敗，返回碼: {return_code}")
            return False
            
    except KeyboardInterrupt:
        print("\n\n⚠️  用戶中斷打包程序")
        return False
    except Exception as e:
        print(f"❌ 打包過程中發生錯誤: {e}")
        return False

def check_dependencies():
    """檢查必要的檔案是否存在"""
    print("檢查必要檔案...")
    
    required_files = [
        "worker_node.py",
        "nodepool_pb2.py", 
        "nodepool_pb2_grpc.py",
        "run_task.sh",
        "templates",
        "static"
    ]
    
    missing_files = []
    for file_path in required_files:
        if not os.path.exists(file_path):
            missing_files.append(file_path)
    
    if missing_files:
        print("❌ 缺少必要檔案:")
        for file in missing_files:
            print(f"   - {file}")
        return False
    
    print("✅ 所有必要檔案都存在")
    return True

def install_requirements():
    """安裝打包所需的依賴"""
    print("檢查並安裝打包依賴...")
    
    packages = ["nuitka>=2.0", "ordered-set>=4.1.0"]
    
    for package in packages:
        try:
            result = subprocess.run([
                sys.executable, "-m", "pip", "install", "--upgrade", package
            ], capture_output=True, text=True, check=True)
            print(f"✅ {package.split('>=')[0]} 安裝/更新成功")
        except subprocess.CalledProcessError as e:
            print(f"❌ {package} 安裝失敗")
            print(f"錯誤: {e.stderr}")
            return False
    
    return True

def create_simple_build():
    """創建簡化版本的打包，如果完整版失敗"""
    print("\n嘗試簡化版本打包...")
    
    simple_cmd = [
        sys.executable, "-m", "nuitka",
        "--onefile",
        "--windows-console-mode=attach",
        "--assume-yes-for-downloads",
        "--show-progress",
        "--output-dir=dist",
        "worker_node.py"
    ]
    
    try:
        print("執行簡化打包命令...")
        result = subprocess.run(simple_cmd, check=True, capture_output=True, text=True)
        print("✅ 簡化版本打包成功!")
        
        exe_path = os.path.join("dist", "worker_node.exe")
        if os.path.exists(exe_path):
            print(f"✅ 可執行檔案: {exe_path}")
            print("⚠️  注意: 簡化版本可能缺少某些資源檔案，請手動複製 templates 和 static 目錄到 exe 同一目錄")
            return True
        
    except subprocess.CalledProcessError as e:
        print(f"❌ 簡化版本也失敗: {e}")
        print("STDERR:", e.stderr)
    
    return False

if __name__ == "__main__":
    print("=== HiveMind Worker Node 打包工具 ===")
    print(f"Python 版本: {sys.version}")
    print(f"工作目錄: {os.getcwd()}")
    print()
    
    # 檢查當前目錄
    if not os.path.exists("worker_node.py"):
        print("❌ 未找到 worker_node.py，請在工作端目錄執行此腳本")
        sys.exit(1)
    
    # 檢查依賴檔案
    if not check_dependencies():
        print("❌ 缺少必要檔案，無法進行打包")
        sys.exit(1)
    
    # 安裝依賴
    if not install_requirements():
        print("❌ 依賴安裝失敗")
        sys.exit(1)
    
    print()
    
    # 開始打包
    if build_exe():
        print("\n🎉 打包完成！")
        print("📁 可執行檔案位於 dist/worker_node.exe")
        print("🚀 執行以下命令啟動:")
        print("   cd dist")
        print("   worker_node.exe")
        print("\n💡 提示: exe 檔案會自動開啟瀏覽器到 http://127.0.0.1:5000")
    else:
        print("\n❓ 完整版本打包失敗，是否嘗試簡化版本? (y/n): ", end="")
        try:
            choice = input().lower().strip()
            if choice == 'y' or choice == 'yes':
                if create_simple_build():
                    print("\n🎉 簡化版本打包完成！")
                    print("⚠️  請手動複製 templates 和 static 目錄到 dist/ 目錄")
                else:
                    print("\n❌ 所有打包方式都失敗了")
                    sys.exit(1)
            else:
                print("\n❌ 打包失敗")
                sys.exit(1)
        except KeyboardInterrupt:
            print("\n\n用戶取消操作")
            sys.exit(1)
