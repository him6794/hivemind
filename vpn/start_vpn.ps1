# 啟動 VPN 的 PowerShell 腳本
$scriptPath = Split-Path -Parent $MyInvocation.MyCommand.Path
$pythonPath = "python"  # 或完整路徑如 "C:\Python39\python.exe"

# 切換到腳本目錄
Set-Location $scriptPath

# 運行 VPN 啟動腳本
& $pythonPath "startup_vpn.py"