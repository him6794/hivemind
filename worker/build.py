import os
import sys
import shutil
import subprocess
import platform

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
    print("開始打包 HiveMind Worker...")
    
    # 檢查依賴
    try:
        import nuitka
    except ImportError:
        print("安裝 Nuitka...")
        subprocess.run([sys.executable, "-m", "pip", "install", "nuitka", "ordered-set", "zstandard"])
    
    # 創建圖示檔案
    create_icon()
    
    # 清理舊的打包檔案
    if os.path.exists("dist"):
        shutil.rmtree("dist")
    if os.path.exists("build"):
        shutil.rmtree("build")
    
    # 確保必要檔案存在
    ensure_required_files()
    
    # Nuitka 打包參數
    nuitka_args = [
        sys.executable, "-m", "nuitka",
        "--standalone",
        "--onefile",
        "--enable-plugin=anti-bloat",
        "--show-progress",
        "--show-memory",
        "--assume-yes-for-downloads",
        "--output-dir=dist",
        "--output-filename=HiveMind-Worker",
        # 包含資源檔案
        "--include-data-dir=templates=templates",
        "--include-data-dir=static=static",
        "--include-data-file=run_task.sh=run_task.sh",
        "--include-data-file=requirements.txt=requirements.txt",
    ]
    
    # 平台特定參數
    if platform.system() == "Windows":
        windows_args = [
            "--windows-console-mode=force",
            "--company-name=HiveMind",
            "--product-name=HiveMind Worker",
            "--file-version=1.0.0",
            "--product-version=1.0.0",
            "--file-description=HiveMind Distributed Computing Worker Node"
        ]
        
        # 只有在圖示檔案存在時才添加圖示參數
        if os.path.exists("icon.ico"):
            windows_args.append("--windows-icon-from-ico=icon.ico")
        
        nuitka_args.extend(windows_args)
        output_name = "HiveMind-Worker.exe"
    else:
        output_name = "HiveMind-Worker"
    
    nuitka_args.append("worker_node.py")
    
    # 執行打包
    print("執行 Nuitka 打包...")
    print(f"打包命令: {' '.join(nuitka_args)}")
    
    result = subprocess.run(nuitka_args, capture_output=True, text=True)
    
    if result.returncode == 0:
        output_path = os.path.join("dist", output_name)
        if os.path.exists(output_path):
            size_mb = os.path.getsize(output_path) / (1024*1024)
            print(f"✓ 打包成功！")
            print(f"✓ 輸出檔案: {output_path}")
            print(f"✓ 檔案大小: {size_mb:.1f} MB")
            return True
        else:
            print("✗ 打包失敗：找不到輸出檔案")
            return False
    else:
        print("✗ 打包失敗")
        print("錯誤輸出:", result.stderr)
        print("標準輸出:", result.stdout)
        return False

def create_installer():
    """創建安裝器"""
    print("\n創建安裝包...")
    
    # 創建發布目錄
    release_dir = "HiveMind-Worker-Release"
    if os.path.exists(release_dir):
        shutil.rmtree(release_dir)
    os.makedirs(release_dir)
    
    # 複製必要檔案
    files_to_copy = [
        ("dist/HiveMind-Worker.exe" if platform.system() == "Windows" else "dist/HiveMind-Worker", ""),
        ("templates", "templates"),
        ("static", "static"),
        ("run_task.sh", ""),
        ("requirements.txt", ""),
        ("README.md", ""),
        ("icon.ico", ""),  # 包含圖示檔案
        ("install.bat" if platform.system() == "Windows" else "install.sh", "")
    ]
    
    for src, dst in files_to_copy:
        if os.path.exists(src):
            dst_path = os.path.join(release_dir, dst) if dst else release_dir
            if os.path.isdir(src):
                shutil.copytree(src, os.path.join(dst_path, os.path.basename(src)))
            else:
                shutil.copy2(src, dst_path)
                print(f"✓ 複製: {src}")
    
    # 創建啟動腳本
    create_launch_scripts(release_dir)
    
    print(f"✓ 發布包已創建: {release_dir}/")
    
    # 創建壓縮包
    create_archive(release_dir)

def create_launch_scripts(release_dir):
    """創建啟動腳本"""
    if platform.system() == "Windows":
        # Windows 啟動腳本
        batch_content = '''@echo off
echo HiveMind Worker Node 啟動器
echo ============================
echo.
echo 正在啟動 HiveMind Worker...
echo 請確保已安裝 Docker Desktop 並正在運行
echo.

REM 檢查管理員權限
net session >nul 2>&1
if %errorLevel% neq 0 (
    echo 警告: 建議以管理員權限運行以獲得最佳體驗
    echo.
)

REM 啟動 Worker
HiveMind-Worker.exe

echo.
echo Worker 已關閉，按任意鍵退出...
pause >nul
'''
        with open(os.path.join(release_dir, "start_worker.bat"), "w", encoding="utf-8") as f:
            f.write(batch_content)
        
    else:
        # Linux 啟動腳本
        shell_content = '''#!/bin/bash
echo "HiveMind Worker Node 啟動器"
echo "============================"
echo ""
echo "正在啟動 HiveMind Worker..."
echo "請確保已安裝 Docker 並正在運行"
echo ""

# 檢查 Docker
if ! command -v docker &> /dev/null; then
    echo "警告: 未檢測到 Docker，請先安裝 Docker"
fi

# 啟動 Worker
chmod +x HiveMind-Worker
./HiveMind-Worker

echo ""
echo "Worker 已關閉"
'''
        script_path = os.path.join(release_dir, "start_worker.sh")
        with open(script_path, "w") as f:
            f.write(shell_content)
        os.chmod(script_path, 0o755)

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

def ensure_required_files():
    """確保所有必要的檔案存在"""
    
    # 創建 templates 目錄和基本模板
    if not os.path.exists("templates"):
        os.makedirs("templates")
        print("✓ 創建 templates 目錄")
    
    # 檢查並複製模板檔案
    release_templates = "HiveMind-Worker-Release/templates"
    if os.path.exists(release_templates):
        # 從 Release 目錄複製模板
        for template_file in ["login.html", "monitor.html"]:
            src = os.path.join(release_templates, template_file)
            dst = os.path.join("templates", template_file)
            if os.path.exists(src) and not os.path.exists(dst):
                shutil.copy2(src, dst)
                print(f"✓ 複製模板檔案: {template_file}")
    
    # 如果還是沒有 login.html，創建基本版本
    if not os.path.exists("templates/login.html"):
        create_basic_login_template()
    
    # 如果還是沒有 monitor.html，創建基本版本
    if not os.path.exists("templates/monitor.html"):
        create_basic_monitor_template()
    
    # 創建 static 目錄
    if not os.path.exists("static"):
        os.makedirs("static")
        os.makedirs("static/css", exist_ok=True)
        os.makedirs("static/js", exist_ok=True)
        print("✓ 創建 static 目錄")
    
    # 複製 static 檔案
    release_static = "HiveMind-Worker-Release/static"
    if os.path.exists(release_static):
        for root, dirs, files in os.walk(release_static):
            for file in files:
                src_file = os.path.join(root, file)
                rel_path = os.path.relpath(src_file, release_static)
                dst_file = os.path.join("static", rel_path)
                
                # 確保目標目錄存在
                os.makedirs(os.path.dirname(dst_file), exist_ok=True)
                
                if not os.path.exists(dst_file):
                    shutil.copy2(src_file, dst_file)
                    print(f"✓ 複製靜態檔案: {rel_path}")

def create_basic_login_template():
    """創建基本的登入模板"""
    login_html = '''<!DOCTYPE html>
<html lang="zh-TW">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>HiveMind Worker 登入</title>
    <style>
        body { font-family: Arial, sans-serif; max-width: 400px; margin: 50px auto; padding: 20px; background: #f5f5f5; }
        .container { background: white; padding: 30px; border-radius: 10px; box-shadow: 0 2px 10px rgba(0,0,0,0.1); }
        .form-group { margin-bottom: 15px; }
        label { display: block; margin-bottom: 5px; font-weight: bold; }
        input, select { width: 100%; padding: 10px; border: 1px solid #ddd; border-radius: 4px; box-sizing: border-box; }
        button { width: 100%; padding: 12px; background: #007bff; color: white; border: none; border-radius: 4px; cursor: pointer; font-size: 16px; }
        button:hover { background: #0056b3; }
        .error { color: red; margin-bottom: 10px; padding: 10px; background: #ffe6e6; border-radius: 4px; }
        .status { margin-bottom: 15px; padding: 10px; background: #e6f3ff; border-radius: 4px; }
        h2 { text-align: center; margin-bottom: 20px; color: #333; }
    </style>
</head>
<body>
    <div class="container">
        <h2>HiveMind Worker 登入</h2>
        
        {% if current_status %}
        <div class="status">
            <strong>節點狀態:</strong> {{ current_status }}
        </div>
        {% endif %}
        
        {% if error %}
        <div class="error">{{ error }}</div>
        {% endif %}
        
        <form method="post">
            <div class="form-group">
                <label for="username">用戶名:</label>
                <input type="text" id="username" name="username" required>
            </div>
            <div class="form-group">
                <label for="password">密碼:</label>
                <input type="password" id="password" name="password" required>
            </div>
            <div class="form-group">
                <label for="location">地區:</label>
                <select id="location" name="location">
                    <option value="{{ current_location }}">{{ current_location }}</option>
                    {% for loc in available_locations %}
                        {% if loc != current_location %}
                            <option value="{{ loc }}">{{ loc }}</option>
                        {% endif %}
                    {% endfor %}
                </select>
            </div>
            <button type="submit">登入並註冊節點</button>
        </form>
    </div>
</body>
</html>'''
    
    with open("templates/login.html", "w", encoding="utf-8") as f:
        f.write(login_html)
    print("✓ 創建基本登入模板")

def create_basic_monitor_template():
    """創建基本的監控模板"""
    monitor_html = '''<!DOCTYPE html>
<html lang="zh-TW">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>HiveMind Worker 監控</title>
    <style>
        body { font-family: Arial, sans-serif; margin: 20px; background: #f5f5f5; }
        .header { background: white; padding: 20px; border-radius: 10px; margin-bottom: 20px; box-shadow: 0 2px 5px rgba(0,0,0,0.1); display: flex; justify-content: space-between; align-items: center; }
        .card { background: white; padding: 20px; border-radius: 10px; margin-bottom: 20px; box-shadow: 0 2px 5px rgba(0,0,0,0.1); }
        .status-table { width: 100%; border-collapse: collapse; }
        .status-table th, .status-table td { padding: 10px; text-align: left; border-bottom: 1px solid #eee; }
        .status-table th { background: #f8f9fa; }
        .logs { background: #1a1a1a; color: #00ff00; padding: 15px; border-radius: 5px; height: 300px; overflow-y: auto; font-family: monospace; font-size: 14px; }
        .btn { padding: 10px 20px; background: #007bff; color: white; text-decoration: none; border-radius: 5px; border: none; cursor: pointer; }
        .btn:hover { background: #0056b3; }
        h2 { margin-top: 0; }
    </style>
</head>
<body>
    <div class="header">
        <h1>HiveMind Worker 監控</h1>
        <div>
            <span>用戶: {{ username }}</span>
            <a href="/logout" class="btn" style="margin-left: 10px;">登出</a>
        </div>
    </div>
    
    <div class="card">
        <h2>節點狀態</h2>
        <table class="status-table">
            <tr><th>節點 ID</th><td id="node-id">{{ node_id }}</td></tr>
            <tr><th>狀態</th><td id="status">{{ initial_status }}</td></tr>
            <tr><th>當前任務</th><td id="task-id">None</td></tr>
            <tr><th>CPU 使用率</th><td id="cpu">0%</td></tr>
            <tr><th>記憶體使用率</th><td id="memory">0%</td></tr>
            <tr><th>CPT 餘額</th><td id="balance">0</td></tr>
            <tr><th>本機 IP</th><td id="ip">檢測中...</td></tr>
        </table>
    </div>
    
    <div class="card">
        <h2>系統日誌 <button class="btn" onclick="refreshLogs()" style="float: right;">刷新</button></h2>
        <div id="logs" class="logs">載入日誌中...</div>
    </div>
    
    <script>
        function updateStatus() {
            fetch('/api/status')
                .then(response => response.json())
                .then(data => {
                    if (data.error) return;
                    document.getElementById('status').textContent = data.status;
                    document.getElementById('task-id').textContent = data.current_task_id || 'None';
                    document.getElementById('cpu').textContent = data.cpu_percent + '%';
                    document.getElementById('memory').textContent = data.memory_percent + '%';
                    document.getElementById('balance').textContent = data.cpt_balance;
                    document.getElementById('ip').textContent = data.ip;
                })
                .catch(error => console.error('Error:', error));
        }
        
        function updateLogs() {
            fetch('/api/logs')
                .then(response => response.json())
                .then(data => {
                    if (data.error) return;
                    const logsDiv = document.getElementById('logs');
                    logsDiv.innerHTML = data.logs.join('\\n');
                    logsDiv.scrollTop = logsDiv.scrollHeight;
                })
                .catch(error => console.error('Error:', error));
        }
        
        function refreshLogs() {
            updateLogs();
        }
        
        // 每 5 秒更新一次
        setInterval(() => {
            updateStatus();
            updateLogs();
        }, 5000);
        
        // 初始載入
        updateStatus();
        updateLogs();
    </script>
</body>
</html>'''
    
    with open("templates/monitor.html", "w", encoding="utf-8") as f:
        f.write(monitor_html)
    print("✓ 創建基本監控模板")

if __name__ == "__main__":
    print("HiveMind Worker 發布打包工具")
    print("=" * 50)
    
    if build_executable():
        create_installer()
        print("\n🎉 發布包創建完成！")
        print("\n使用說明：")
        print("1. 將發布包複製到目標機器")
        print("2. 解壓後執行 start_worker.bat (Windows) 或 start_worker.sh (Linux)")
        print("3. 或直接運行 HiveMind-Worker.exe")
        print("4. 按照提示完成 VPN 和 Docker 設置")
        print("\n注意事項：")
        print("- Windows 用戶建議以管理員權限運行")
        print("- 確保 Docker 已安裝並正在運行")
        print("- 首次運行時需要配置 WireGuard VPN")
    else:
        print("\n❌ 發布包創建失敗")
        sys.exit(1)
    print("\n感謝使用 HiveMind Worker 發布打包工具！")
    print("如有任何問題，請聯繫開發者或參考官方文檔。")
