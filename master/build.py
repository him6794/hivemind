import os
import sys
import shutil
import subprocess
import platform
import threading
import time

def show_progress_bar(message, duration=None):
    """顯示進度條"""
    def animate():
        chars = "|/-\\"
        idx = 0
        start_time = time.time()
        
        while getattr(animate, 'running', True):
            if duration and time.time() - start_time > duration:
                break
            print(f"\r{message} {chars[idx % len(chars)]}", end="", flush=True)
            idx += 1
            time.sleep(0.1)
        print(f"\r{message} ✓", flush=True)
    
    animate.running = True
    thread = threading.Thread(target=animate, daemon=True)
    thread.start()
    return thread

def stop_progress(thread):
    """停止進度條"""
    if hasattr(thread, 'running'):
        thread.running = False
    time.sleep(0.2)

def create_icon():
    """創建簡單的 ico 檔案（如果不存在）"""
    if not os.path.exists("icon.ico"):
        print("創建預設圖示檔案...")
        # 創建一個簡單的 16x16 像素 ICO 檔案
        ico_data = b'\x00\x00\x01\x00\x01\x00\x10\x10\x00\x00\x01\x00\x20\x00\x68\x04\x00\x00\x16\x00\x00\x00' + b'\x00' * 1128
        try:
            with open("icon.ico", "wb") as f:
                f.write(ico_data)
            print("✓ 預設圖示檔案已創建")
        except Exception as e:
            print(f"⚠ 創建圖示檔案失敗: {e}")

def build_executable():
    """使用 Nuitka 打包可執行檔"""
    print("開始打包 HiveMind Master...")
    
    # 檢查依賴
    progress = show_progress_bar("檢查 Nuitka 依賴")
    try:
        import nuitka
        stop_progress(progress)
        print("✓ Nuitka 已安裝")
    except ImportError:
        stop_progress(progress)
        print("安裝 Nuitka...")
        progress = show_progress_bar("安裝 Nuitka 和相關依賴")
        subprocess.run([sys.executable, "-m", "pip", "install", "nuitka", "ordered-set", "zstandard"])
        stop_progress(progress)
        print("✓ Nuitka 安裝完成")
    
    # 創建圖示檔案
    progress = show_progress_bar("準備資源檔案")
    create_icon()
    stop_progress(progress)
    
    # 清理舊的打包檔案
    progress = show_progress_bar("清理舊檔案")
    if os.path.exists("dist"):
        shutil.rmtree("dist")
    if os.path.exists("build"):
        shutil.rmtree("build")
    stop_progress(progress)
    
    # 確保必要檔案存在
    progress = show_progress_bar("檢查必要檔案")
    ensure_required_files()
    stop_progress(progress)
    
    # Nuitka 打包參數 - 必須包含 HTML 模板
    nuitka_args = [
        sys.executable, "-m", "nuitka",
        "--standalone",
        "--onefile",
        "--enable-plugin=anti-bloat",
        "--show-progress",
        "--show-memory",
        "--assume-yes-for-downloads",
        "--output-dir=dist",
        "--output-filename=HiveMind-Master",
        # 包含所有資源檔案（HTML 模板是必需的）
        "--include-data-dir=templates_master=templates_master",
        "--include-data-dir=static_master=static_master",
        "--include-data-dir=data=data",
        "--include-data-file=requirements.txt=requirements.txt",
        # 包含 protobuf 檔案
        "--include-data-file=nodepool_pb2.py=nodepool_pb2.py",
        "--include-data-file=nodepool_pb2_grpc.py=nodepool_pb2_grpc.py",
    ]
    
    # 平台特定參數
    if platform.system() == "Windows":
        windows_args = [
            "--windows-console-mode=force",
            "--company-name=HiveMind",
            "--product-name=HiveMind Master",
            "--file-version=1.0.0",
            "--product-version=1.0.0",
            "--file-description=HiveMind Distributed Computing Master Node"
        ]
        
        # 只有在圖示檔案存在時才添加圖示參數
        if os.path.exists("icon.ico"):
            windows_args.append("--windows-icon-from-ico=icon.ico")
        
        nuitka_args.extend(windows_args)
        output_name = "HiveMind-Master.exe"
    else:
        output_name = "HiveMind-Master"
    
    nuitka_args.append("master_node.py")  # 主控端主檔案
    
    # 執行打包
    print("\n開始 Nuitka 打包過程...")
    print("這可能需要幾分鐘時間，請耐心等待...")
    print(f"打包命令: {' '.join(nuitka_args[:10])}...")  # 只顯示前面部分命令
    
    # 啟動打包進度提示
    print("\n正在編譯...")
    
    result = subprocess.run(nuitka_args, capture_output=True, text=True)
    
    if result.returncode == 0:
        output_path = os.path.join("dist", output_name)
        if os.path.exists(output_path):
            size_mb = os.path.getsize(output_path) / (1024*1024)
            print(f"\n✓ 打包成功！")
            print(f"✓ 輸出檔案: {output_path}")
            print(f"✓ 檔案大小: {size_mb:.1f} MB")
            return True
        else:
            print("\n✗ 打包失敗：找不到輸出檔案")
            return False
    else:
        print("\n✗ 打包失敗")
        print("錯誤輸出:", result.stderr[-1000:])  # 只顯示最後1000字符
        return False

def ensure_required_files():
    """確保所有必要的檔案存在"""
    
    # 檢查主要目錄
    required_dirs = ["templates_master", "static_master", "data"]
    for dir_name in required_dirs:
        if not os.path.exists(dir_name):
            os.makedirs(dir_name)
            print(f"✓ 創建目錄: {dir_name}")
    
    # 檢查模板檔案 - 這些是必需的！
    template_files = ["templates_master/master_upload.html"]
    for template_file in template_files:
        if not os.path.exists(template_file):
            print(f"⚠ 警告：缺少模板檔案 {template_file}")
            print("Flask 應用需要 HTML 模板才能正常運行！")
    
    # 檢查主檔案是否存在
    main_files = ["master_node.py"]
    missing_files = [f for f in main_files if not os.path.exists(f)]
    if missing_files:
        print(f"⚠ 缺少主要檔案: {', '.join(missing_files)}")
        print("請確認主控端檔案結構正確")
        
        # 嘗試查找可能的主檔案
        possible_files = ["flask_master.py", "grpc_master.py", "app.py", "main.py"]
        found_files = [f for f in possible_files if os.path.exists(f)]
        if found_files:
            print(f"發現可能的主檔案: {', '.join(found_files)}")
            print("請確認哪個是正確的主入口檔案")
    
    print("✓ 必要檔案檢查完成")

def create_installer():
    """創建安裝器"""
    print("\n創建安裝包...")
    
    progress = show_progress_bar("創建發布目錄")
    
    # 創建發布目錄
    release_dir = "HiveMind-Master-Release"
    if os.path.exists(release_dir):
        shutil.rmtree(release_dir)
    os.makedirs(release_dir)
    
    stop_progress(progress)
    
    # 複製必要檔案
    files_to_copy = [
        ("dist/HiveMind-Master.exe" if platform.system() == "Windows" else "dist/HiveMind-Master", ""),
        ("templates_master", "templates_master"),  # HTML 模板必須包含
        ("static_master", "static_master"),
        ("data", "data"),
        ("requirements.txt", ""),
        ("icon.ico", ""),
        ("config.ini", ""),
    ]
    
    progress = show_progress_bar("複製檔案")
    
    for src, dst in files_to_copy:
        if os.path.exists(src):
            dst_path = os.path.join(release_dir, dst) if dst else release_dir
            if os.path.isdir(src):
                shutil.copytree(src, os.path.join(dst_path, os.path.basename(src)))
            else:
                shutil.copy2(src, dst_path)
    
    stop_progress(progress)
    
    # 創建啟動腳本
    progress = show_progress_bar("創建啟動腳本")
    create_launch_scripts(release_dir)
    stop_progress(progress)
    
    # 創建使用說明
    progress = show_progress_bar("創建使用說明")
    create_usage_readme(release_dir)
    stop_progress(progress)
    
    print(f"✓ 發布包已創建: {release_dir}/")
    
    # 創建壓縮包
    progress = show_progress_bar("創建壓縮包")
    create_archive(release_dir)
    stop_progress(progress)

def create_launch_scripts(release_dir):
    """創建啟動腳本"""
    if platform.system() == "Windows":
        # Windows 啟動腳本
        batch_content = '''@echo off
echo HiveMind Master Node 啟動器
echo =============================
echo.
echo 正在啟動 HiveMind Master...
echo 請確保已安裝 Docker Desktop 並正在運行
echo.

REM 檢查管理員權限
net session >nul 2>&1
if %errorLevel% neq 0 (
    echo 警告: 建議以管理員權限運行以獲得最佳體驗
    echo.
)

REM 啟動 Master
HiveMind-Master.exe

echo.
echo Master 已關閉，按任意鍵退出...
pause >nul
'''
        with open(os.path.join(release_dir, "start_master.bat"), "w", encoding="utf-8") as f:
            f.write(batch_content)
        
    else:
        # Linux 啟動腳本
        shell_content = '''#!/bin/bash
echo "HiveMind Master Node 啟動器"
echo "============================"
echo ""
echo "正在啟動 HiveMind Master..."
echo "請確保已安裝 Docker 並正在運行"
echo ""

# 檢查 Docker
if ! command -v docker &> /dev/null; then
    echo "警告: 未檢測到 Docker，請先安裝 Docker"
fi

# 啟動 Master
chmod +x HiveMind-Master
./HiveMind-Master

echo ""
echo "Master 已關閉"
'''
        script_path = os.path.join(release_dir, "start_master.sh")
        with open(script_path, "w") as f:
            f.write(shell_content)
        os.chmod(script_path, 0o755)

def create_usage_readme(release_dir):
    """創建使用說明檔案"""
    readme_content = """# HiveMind Master Node 使用說明

## 系統需求
- Windows 10/11 (64位) 或 Linux
- 4GB+ RAM
- Docker Desktop (Windows) 或 Docker CE (Linux)
- 管理員/Root 權限

## 安裝步驟

### Windows 用戶
1. 解壓 HiveMind-Master-Release.zip
2. 右鍵點擊 `start_master.bat`，選擇「以管理員身份運行」
3. 或直接運行 `HiveMind-Master.exe`

### Linux 用戶
1. 解壓 HiveMind-Master-Release.tar.gz
2. 執行 `sudo bash start_master.sh`
3. 或直接運行 `./HiveMind-Master`

## 配置說明

### 環境變數
- `MASTER_PORT`: gRPC 服務端口 (預設: 50051)
- `FLASK_PORT`: Web 界面端口 (預設: 8080)
- `DATABASE_URL`: 資料庫連接字符串

### 配置檔案
編輯 `config.ini` 檔案設定：
```ini
[master]
port = 50051
flask_port = 8080
database_path = data/hivemind.db

[vpn]
wireguard_config_path = data/wg0.conf
network_range = 10.0.0.0/24
```

## 使用方式

1. **啟動主控端**：運行可執行檔或啟動腳本
2. **開啟 Web 界面**：瀏覽器訪問 http://localhost:8080
3. **註冊用戶**：首次使用需要註冊管理員帳號
4. **管理節點**：在 Web 界面中查看和管理工作節點
5. **上傳任務**：通過 Web 界面上傳和管理計算任務

## Web 界面功能

- **儀表板**：查看系統狀態和統計信息
- **節點管理**：監控和管理工作節點
- **任務管理**：上傳、分發和監控計算任務
- **用戶管理**：管理用戶帳號和權限
- **VPN 配置**：管理 WireGuard VPN 設定

## 連接工作節點

工作節點需要連接到主控端的地址，預設為：
- gRPC 服務: `主控端IP:50051`
- VPN 網段: `10.0.0.0/24`

## 故障排除

### 常見問題
1. **端口衝突**：檢查 50051 和 8080 端口是否被佔用
2. **權限問題**：確保以管理員/Root 權限運行
3. **防火牆**：確保防火牆允許相關端口通信
4. **Docker 問題**：確保 Docker 正在運行

### 日誌檔案
- 系統日誌: `logs/master.log`
- 錯誤日誌: `logs/error.log`

## 技術支援

如遇問題請檢查：
1. 系統需求是否滿足
2. 網路連接是否正常
3. 防火牆設定是否正確
4. Docker 服務是否運行

更多詳細資訊請參考官方文檔。
"""
    
    with open(os.path.join(release_dir, "README.md"), "w", encoding="utf-8") as f:
        f.write(readme_content)

def create_archive(release_dir):
    """創建壓縮包"""
    if platform.system() == "Windows":
        try:
            import zipfile
            zip_name = f"{release_dir}.zip"
            with zipfile.ZipFile(zip_name, 'w', zipfile.ZIP_DEFLATED) as zipf:
                for root, dirs, files in os.walk(release_dir):
                    for file in files:
                        file_path = os.path.join(root, file)
                        arc_name = os.path.relpath(file_path, release_dir)
                        zipf.write(file_path, arc_name)
            print(f"✓ 壓縮包已創建: {zip_name}")
        except Exception as e:
            print(f"⚠ 創建壓縮包失敗: {e}")
    else:
        try:
            subprocess.run(["tar", "-czf", f"{release_dir}.tar.gz", release_dir])
            print(f"✓ 壓縮包已創建: {release_dir}.tar.gz")
        except Exception as e:
            print(f"⚠ 創建壓縮包失敗: {e}")

if __name__ == "__main__":
    print("HiveMind Master 發布打包工具")
    print("=" * 50)
    
    if build_executable():
        create_installer()
        print("\n🎉 發布包創建完成！")
        print("\n使用說明：")
        print("1. 將發布包複製到目標機器")
        print("2. 解壓後執行 start_master.bat (Windows) 或 start_master.sh (Linux)")
        print("3. 或直接運行 HiveMind-Master.exe")
        print("4. 開啟瀏覽器訪問 http://localhost:8080")
        print("\n注意事項：")
        print("- Windows 用戶建議以管理員權限運行")
        print("- 確保 Docker 已安裝並正在運行")
        print("- 確保端口 50051 和 8080 未被佔用")
    else:
        print("\n❌ 發布包創建失敗")
        sys.exit(1)
