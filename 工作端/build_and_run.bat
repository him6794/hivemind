REM filepath: d:\hivemind\工作端\build_and_run.bat
@echo off
echo === HiveMind 工作節點 一鍵打包執行 ===
echo.

echo 步驟 1: 檢查 Python 環境
python --version
if errorlevel 1 (
    echo 錯誤: 未找到 Python，請確保 Python 已安裝並加入 PATH
    pause
    exit /b 1
)

echo.
echo 步驟 2: 開始打包
python build_exe.py
if errorlevel 1 (
    echo 打包失敗，請檢查錯誤訊息
    pause
    exit /b 1
)

echo.
echo 步驟 3: 啟動工作節點
call start_worker.bat