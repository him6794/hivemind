REM filepath: d:\hivemind\工作端\setup_venv.bat
@echo off
echo === HiveMind 工作節點虛擬環境設置 ===
echo.

REM 檢查 Python 是否可用
python --version >nul 2>&1
if errorlevel 1 (
    echo ❌ 錯誤: 未找到 Python，請確保 Python 已安裝並加入 PATH
    pause
    exit /b 1
)

echo ✅ Python 已安裝
echo.

REM 創建虛擬環境
echo 正在創建虛擬環境...
if exist "venv" (
    echo 虛擬環境已存在，正在刪除舊環境...
    rmdir /s /q venv
)

python -m venv venv
if errorlevel 1 (
    echo ❌ 創建虛擬環境失敗
    pause
    exit /b 1
)

echo ✅ 虛擬環境創建成功
echo.

REM 激活虛擬環境並安裝依賴
echo 正在激活虛擬環境並安裝依賴...
call venv\Scripts\activate.bat

echo 升級 pip...
python -m pip install --upgrade pip

echo 安裝基礎依賴...
pip install flask grpcio grpcio-tools docker psutil

echo 安裝打包工具...
pip install nuitka ordered-set

echo.
echo ✅ 虛擬環境設置完成！
echo.
echo 💡 使用說明:
echo   1. 激活環境: call venv\Scripts\activate.bat
echo   2. 運行程式: python worker_node.py
echo   3. 打包程式: python build_exe.py
echo   4. 退出環境: deactivate
echo.
pause