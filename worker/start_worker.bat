REM filepath: d:\hivemind\工作端\start_worker.bat
@echo off
echo 正在啟動 HiveMind 工作節點...
echo.

REM 檢查可執行檔案是否存在
if not exist "dist\worker_node.exe" (
    echo 錯誤: 未找到 dist\worker_node.exe
    echo 請先執行 python build_exe.py 進行打包
    pause
    exit /b 1
)

REM 設置環境變數（可選）
set NODE_PORT=50053
set FLASK_PORT=5000
set MASTER_ADDRESS=192.168.2.52:50051

echo 啟動參數:
echo   節點端口: %NODE_PORT%
echo   Web端口: %FLASK_PORT%
echo   主控端地址: %MASTER_ADDRESS%
echo.

REM 啟動工作節點
echo 正在啟動工作節點，瀏覽器將自動開啟 http://127.0.0.1:5000
echo 按 Ctrl+C 停止服務
echo.

dist\worker_node.exe

echo.
echo 工作節點已停止
pause