"""
HiveMind Worker 建置腳本
"""

import os
import sys
import shutil
import subprocess
import urllib.request
import zipfile
from pathlib import Path

def run_command(cmd, cwd=None):
    """執行命令"""
    print(f"執行: {cmd}")
    result = subprocess.run(cmd, shell=True, cwd=cwd, capture_output=True, text=True)
    if result.returncode != 0:
        print(f"錯誤: {result.stderr}")
        return False
    print(result.stdout)
    return True

def download_file(url, dest):
    """下載檔案"""
    print(f"下載: {url} -> {dest}")
    try:
        urllib.request.urlretrieve(url, dest)
        return True
    except Exception as e:
        print(f"下載失敗: {e}")
        return False

def build_package():
    """建置 Python 套件"""
    print("🔨 建置 Python 套件...")
    
    # 清理舊的建置檔案
    for dir_name in ["build", "dist", "*.egg-info"]:
        if os.path.exists(dir_name):
            shutil.rmtree(dir_name, ignore_errors=True)
    
    # 生成 protobuf 檔案
    if not run_command("python -m grpc_tools.protoc --proto_path=hivemind_worker/proto --python_out=hivemind_worker --grpc_python_out=hivemind_worker hivemind_worker/proto/nodepool.proto"):
        return False
    
    # 建置套件
    if not run_command("python setup.py sdist bdist_wheel"):
        return False
    
    print("✅ Python 套件建置完成")
    return True

def download_dependencies():
    """下載安裝程式依賴"""
    print("📥 下載安裝程式依賴...")
    
    installer_dir = Path("installer")
    installer_dir.mkdir(exist_ok=True)
    
    # 下載 Python 嵌入式版本
    python_dir = installer_dir / "python"
    if not python_dir.exists():
        print("下載 Python 3.11 嵌入式版本...")
        python_url = "https://www.python.org/ftp/python/3.11.7/python-3.11.7-embed-amd64.zip"
        python_zip = installer_dir / "python-embed.zip"
        
        if download_file(python_url, python_zip):
            with zipfile.ZipFile(python_zip, 'r') as zip_ref:
                zip_ref.extractall(python_dir)
            python_zip.unlink()
            print("✅ Python 嵌入式版本下載完成")
        else:
            return False
    
    # 下載 WireGuard
    wireguard_dir = installer_dir / "wireguard"
    wireguard_dir.mkdir(exist_ok=True)
    
    if not (wireguard_dir / "wireguard-installer.msi").exists():
        print("下載 WireGuard...")
        wg_url = "https://download.wireguard.com/windows-client/wireguard-installer.msi"
        if not download_file(wg_url, wireguard_dir / "wireguard-installer.msi"):
            return False
        print("✅ WireGuard 下載完成")
    
    # 下載 Docker Desktop
    docker_dir = installer_dir / "docker"
    docker_dir.mkdir(exist_ok=True)
    
    if not (docker_dir / "Docker Desktop Installer.exe").exists():
        print("⚠️  請手動下載 Docker Desktop Installer.exe 到 installer/docker/ 目錄")
        print("   下載地址: https://desktop.docker.com/win/main/amd64/Docker%20Desktop%20Installer.exe")
    
    return True

def build_installer():
    """建置安裝程式"""
    print("📦 建置 Windows 安裝程式...")
    
    # 檢查 Inno Setup 是否安裝
    inno_setup_path = r"C:\Program Files (x86)\Inno Setup 6\ISCC.exe"
    if not os.path.exists(inno_setup_path):
        print("❌ 請先安裝 Inno Setup 6")
        print("   下載地址: https://jrsoftware.org/isdl.php")
        return False
    
    # 建置安裝程式
    iss_file = "installer/setup.iss"
    if not run_command(f'"{inno_setup_path}" "{iss_file}"'):
        return False
    
    print("✅ Windows 安裝程式建置完成")
    return True

def main():
    """主函數"""
    print("🚀 HiveMind Worker 建置流程開始...")
    
    # 檢查當前目錄
    if not os.path.exists("setup.py"):
        print("❌ 請在包含 setup.py 的目錄中執行此腳本")
        sys.exit(1)
    
    # 建置套件
    if not build_package():
        print("❌ 套件建置失敗")
        sys.exit(1)
    
    # 下載依賴
    if not download_dependencies():
        print("❌ 依賴下載失敗")
        sys.exit(1)
    
    # 建置安裝程式
    if not build_installer():
        print("❌ 安裝程式建置失敗")
        sys.exit(1)
    
    print("🎉 建置完成！")
    print("📦 Python 套件: dist/")
    print("💿 Windows 安裝程式: installer/dist/HiveMind-Worker-Setup.exe")

if __name__ == "__main__":
    main()
