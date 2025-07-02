import subprocess
import sys
import os
import shutil
import threading
import time

def check_python_version():
    """檢查 Python 版本和安裝來源"""
    print("檢查 Python 環境...")
    print(f"Python 版本: {sys.version}")
    print(f"Python 路徑: {sys.executable}")
    
    # 檢查是否為 Windows Store 版本
    if "WindowsApps" in sys.executable or "Microsoft\\WindowsApps" in sys.executable:
        print("❌ 檢測到 Windows Store 版本的 Python")
        print("🔧 解決方案:")
        print("   1. 從 python.org 下載並安裝標準版 Python")
        print("   2. 或者安裝 Anaconda/Miniconda")
        print("   3. 或者使用 PyInstaller 代替 Nuitka")
        print("\n詳細信息: https://nuitka.net/info/unsupported-windows-app-store-python.html")
        return False
    
    # 檢查 Python 版本
    if sys.version_info < (3, 8):
        print(f"⚠️  Python 版本過舊: {sys.version_info}")
        print("   建議使用 Python 3.8 或更新版本")
    
    print("✅ Python 環境檢查通過")
    return True

def check_path_issues():
    """檢查路徑相關問題"""
    current_path = os.getcwd()
    print(f"檢查當前路徑: {current_path}")
    
    # 檢查路徑中是否有中文字符
    try:
        current_path.encode('ascii')
        print("✅ 路徑不包含非ASCII字符")
    except UnicodeEncodeError:
        print("❌ 路徑包含中文或特殊字符，這可能導致編譯失敗")
        print("🔧 建議解決方案:")
        print("   1. 將項目移動到純英文路徑，例如: C:\\hivemind\\worker")
        print("   2. 或者使用 PyInstaller 代替 Nuitka")
        return False
    
    # 檢查路徑長度
    if len(current_path) > 100:
        print("⚠️  路徑較長，可能導致編譯問題")
        print(f"   當前路徑長度: {len(current_path)} 字符")
        print("   建議移動到較短的路徑")
    
    return True

def try_pyinstaller_build():
    """使用 PyInstaller 作為替代方案"""
    print("\n嘗試使用 PyInstaller 打包...")
    
    try:
        # 安裝 PyInstaller
        print("安裝 PyInstaller...")
        subprocess.run([
            sys.executable, "-m", "pip", "install", "--upgrade", "pyinstaller"
        ], check=True, capture_output=True)
        print("✅ PyInstaller 安裝成功")
    except subprocess.CalledProcessError:
        print("❌ PyInstaller 安裝失敗")
        return False
    
    # 準備 PyInstaller 命令
    icon_option = []
    if os.path.exists("file.ico"):
        icon_option = ["--icon=file.ico"]
    
    pyinstaller_cmd = [
        sys.executable, "-m", "PyInstaller",
        "--onefile",
        "--console",  # 保留控制台
        "--add-data", "templates;templates",
        "--add-data", "static;static", 
        "--add-data", "run_task.sh;.",
        "--add-data", "nodepool_pb2.py;.",
        "--add-data", "nodepool_pb2_grpc.py;.",
        "--distpath", "dist",
        "--workpath", "build",
        "--specpath", ".",
        "worker_node.py"
    ]
    
    if icon_option:
        pyinstaller_cmd.extend(icon_option)
    
    try:
        print("執行 PyInstaller 打包...")
        print("命令:", " ".join(pyinstaller_cmd))
        
        result = subprocess.run(
            pyinstaller_cmd, 
            check=True, 
            capture_output=True, 
            text=True
        )
        
        print("✅ PyInstaller 打包成功!")
        
        # 檢查生成的檔案
        exe_path = os.path.join("dist", "worker_node.exe")
        if os.path.exists(exe_path):
            file_size = os.path.getsize(exe_path) / 1024 / 1024
            print(f"✅ 可執行檔案: {exe_path}")
            print(f"📁 檔案大小: {file_size:.1f} MB")
            return True
        else:
            print("❌ 未找到生成的可執行檔案")
            return False
            
    except subprocess.CalledProcessError as e:
        print(f"❌ PyInstaller 打包失敗: {e}")
        if e.stderr:
            print(f"錯誤信息: {e.stderr}")
        return False

def build_exe():
    """使用 Nuitka 打包 worker_node.py 為 exe"""
    
    print("開始使用 Nuitka 打包工作節點...")
    
    # 確保必要的目錄存在
    output_dir = "dist"
    if os.path.exists(output_dir):
        shutil.rmtree(output_dir)
    os.makedirs(output_dir, exist_ok=True)
    
    # 檢查圖標文件是否存在
    icon_path = "file.ico"
    icon_option = []
    if os.path.exists(icon_path):
        icon_option = [f"--windows-icon-from-ico={icon_path}"]
        print(f"✅ 找到圖標文件: {icon_path}")
    else:
        print("⚠️  未找到圖標文件 file.ico，將使用默認圖標")
    
    # 優化的 Nuitka 打包命令
    nuitka_cmd = [
        sys.executable, "-m", "nuitka",
        "--standalone",
        "--onefile",
        "--windows-console-mode=force",
        "--disable-console",  # 減少控制台相關問題
        "--enable-console",   # 重新啟用控制台
        "--mingw64",         # 使用 MinGW 編譯器避免 MSVC 問題
        "--include-package=flask",
        "--include-package=grpc", 
        "--include-package=docker",
        "--include-package=psutil",
        "--include-data-dir=templates=templates",
        "--include-data-dir=static=static",
        "--include-data-file=run_task.sh=run_task.sh",
        "--include-data-files=nodepool_pb2.py=nodepool_pb2.py",
        "--include-data-files=nodepool_pb2_grpc.py=nodepool_pb2_grpc.py",
        "--output-dir=dist",
        "--remove-output",
        "--assume-yes-for-downloads",
        "--show-progress",
        "worker_node.py"
    ]
    
    # 添加圖標選項（如果存在）
    if icon_option:
        nuitka_cmd.extend(icon_option)
    
    try:
        print("執行 Nuitka 打包命令...")
        print("如果遇到編譯錯誤，將自動嘗試 PyInstaller...")
        
        process = subprocess.Popen(
            nuitka_cmd,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT, 
            text=True,
            bufsize=1,
            universal_newlines=True
        )
        
        start_time = time.time()
        output_lines = []
        
        while True:
            output = process.stdout.readline()
            if output == '' and process.poll() is not None:
                break
            if output:
                print(output.strip())
                output_lines.append(output.strip())
        
        return_code = process.poll()
        
        if return_code == 0:
            exe_path = os.path.join(output_dir, "worker_node.exe")
            if os.path.exists(exe_path):
                file_size = os.path.getsize(exe_path) / 1024 / 1024
                print(f"\n✅ Nuitka 打包成功!")
                print(f"📁 檔案: {exe_path}")
                print(f"📏 大小: {file_size:.1f} MB")
                return True
        
        # 檢查是否是路徑相關錯誤
        error_output = "\n".join(output_lines)
        if any(keyword in error_output for keyword in ["LNK1104", "無法開啟檔案", "編碼", "中文"]):
            print("\n❌ 檢測到路徑或編碼相關錯誤")
            return False
        
        print(f"\n❌ Nuitka 打包失敗，返回碼: {return_code}")
        return False
        
    except Exception as e:
        print(f"❌ Nuitka 打包過程出錯: {e}")
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
    
    # 檢查圖標文件（可選）
    if os.path.exists("file.ico"):
        print("✅ 圖標文件 file.ico 存在")
    else:
        print("⚠️  圖標文件 file.ico 不存在，將使用默認圖標")
    
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
    
    # 檢查圖標文件
    icon_option = []
    if os.path.exists("file.ico"):
        icon_option = ["--windows-icon-from-ico=file.ico"]
    
    simple_cmd = [
        sys.executable, "-m", "nuitka",
        "--onefile",
        "--windows-console-mode=force",  # 保留控制台
        "--assume-yes-for-downloads",
        "--show-progress",
        "--output-dir=dist",
        "worker_node.py"
    ]
    
    # 添加圖標選項（如果存在）
    if icon_option:
        simple_cmd.extend(icon_option)
    
    try:
        print("執行簡化打包命令...")
        result = subprocess.run(simple_cmd, check=True, capture_output=True, text=True)
        print("✅ 簡化版本打包成功!")
        
        exe_path = os.path.join("dist", "worker_node.exe")
        if os.path.exists(exe_path):
            print(f"✅ 可執行檔案: {exe_path}")
            print("✅ 控制台終端已保留，運行時會顯示終端窗口")
            if icon_option:
                print("✅ 自定義圖標已應用")
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
    
    # 檢查 Python 版本
    if not check_python_version():
        print("\n❌ Python 環境檢查失敗")
        print("🔧 建議使用 PyInstaller 替代方案")
        if input("是否嘗試 PyInstaller? (y/n): ").lower().startswith('y'):
            if try_pyinstaller_build():
                print("🎉 PyInstaller 打包成功!")
                sys.exit(0)
        sys.exit(1)
    
    # 檢查路徑問題
    if not check_path_issues():
        print("\n❌ 路徑檢查失敗")
        print("🔧 建議使用 PyInstaller 替代方案")
        if input("是否嘗試 PyInstaller? (y/n): ").lower().startswith('y'):
            if try_pyinstaller_build():
                print("🎉 PyInstaller 打包成功!")
                sys.exit(0)
        sys.exit(1)
    
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
    
    # 嘗試 Nuitka 打包
    if build_exe():
        print("\n🎉 打包完成！")
        print("📁 可執行檔案位於 dist/worker_node.exe")
        print("✅ 控制台終端已保留，運行時會顯示狀態信息")
    else:
        print("\n❌ Nuitka 打包失敗")
        print("🔧 是否嘗試 PyInstaller 替代方案? (y/n): ", end="")
        
        try:
            choice = input().lower().strip()
            if choice == 'y' or choice == 'yes':
                if try_pyinstaller_build():
                    print("\n🎉 PyInstaller 打包成功！")
                    print("📁 可執行檔案位於 dist/worker_node.exe")
                else:
                    print("\n❌ 所有打包方式都失敗了")
                    print("💡 建議:")
                    print("   1. 將項目移動到純英文路徑")
                    print("   2. 使用標準版 Python（非 Windows Store 版本）")
                    print("   3. 檢查系統是否安裝了 Visual Studio Build Tools")
                    sys.exit(1)
            else:
                print("\n❌ 打包失敗")
                sys.exit(1)
        except KeyboardInterrupt:
            print("\n\n用戶取消操作")
            sys.exit(1)
