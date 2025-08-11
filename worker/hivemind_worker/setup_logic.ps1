# 接收從 Inno Setup 傳來的安裝路徑參數
param(
    [string]$AppDir
)

# 如果沒有收到 AppDir 參數，就中止腳本
if ([string]::IsNullOrEmpty($AppDir)) {
    Write-Error "Error: AppDir parameter is missing."
    exit 1
}

$VenvDir = Join-Path $AppDir 'venv'
$PythonExe = Join-Path $VenvDir 'Scripts\python.exe'

# 步驟 A: 創建虛擬環境 (如果不存在)
if (-not (Test-Path $VenvDir)) {
    Write-Host '正在創建 Python 虛擬環境...'
    python -m venv $VenvDir
} else {
    Write-Host '虛擬環境已存在，跳過創建步驟'
}

# 步驟 B: 在虛擬環境中安裝 hivemind_worker (直接使用 venv 裡的 python.exe，更可靠)
Write-Host '正在虛擬環境中安裝 hivemind_worker...'
& $PythonExe -m pip install hivemind_worker

# 步驟 C: 創建啟動批次檔 (使用 Here-String 語法來避免所有引號和特殊字元問題)
$BatContent = @"
@echo off
echo Changing directory to worker folder...
cd /d "$AppDir"

echo.
echo Activating virtual environment...
call "venv\Scripts\activate.bat"

if %errorlevel% neq 0 (
    echo.
    echo ERROR: Failed to activate virtual environment.
    pause
    exit /b 1
)

echo.
echo Starting HiveMind Worker...
echo (To stop the worker, simply close this window)
echo.
python -c "from hivemind_worker import worker_node; worker_node.run_worker_node()"

pause
"@
$BatContent | Out-File -FilePath (Join-Path $AppDir 'start_hivemind.bat') -Encoding oem