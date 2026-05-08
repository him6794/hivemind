#!/usr/bin/env pwsh
# HiveMind 測試腳本

Write-Host "==================================" -ForegroundColor Cyan
Write-Host "  HiveMind 功能測試" -ForegroundColor Cyan
Write-Host "==================================" -ForegroundColor Cyan
Write-Host ""

# 檢查服務是否運行
Write-Host "[1/5] 檢查服務狀態..." -ForegroundColor Yellow

# 檢查 Redis
$redisRunning = docker ps | Select-String "redis-hivemind"
if ($redisRunning) {
    Write-Host "✓ Redis 正在運行" -ForegroundColor Green
} else {
    Write-Host "✗ Redis 未運行，請先執行 ./start_hivemind.ps1" -ForegroundColor Red
    exit 1
}

# 檢查 Master
try {
    $response = Invoke-WebRequest -Uri "http://localhost:8082/health" -TimeoutSec 2 -ErrorAction Stop
    Write-Host "✓ Master 正在運行" -ForegroundColor Green
} catch {
    Write-Host "✗ Master 未運行，請先執行 ./start_hivemind.ps1" -ForegroundColor Red
    exit 1
}

# 建立測試任務
Write-Host ""
Write-Host "[2/5] 建立測試任務..." -ForegroundColor Yellow

# 建立測試目錄
$testDir = "test_task_$(Get-Date -Format 'yyyyMMdd_HHmmss')"
New-Item -ItemType Directory -Path $testDir -Force | Out-Null

# 建立簡單的 Python 腳本
$pythonScript = @"
print("=" * 50)
print("HiveMind 測試任務")
print("=" * 50)
print("")

# 基礎計算
result = sum(range(1, 101))
print(f"計算結果: 1+2+...+100 = {result}")

# 字串處理
message = "Hello from HiveMind!"
print(f"訊息: {message}")
print(f"訊息長度: {len(message)}")

# 列表操作
numbers = [1, 2, 3, 4, 5]
print(f"數字列表: {numbers}")
print(f"列表總和: {sum(numbers)}")

print("")
print("=" * 50)
print("任務執行成功！")
print("=" * 50)
"@

Set-Content -Path "$testDir/main.py" -Value $pythonScript -Encoding UTF8
Write-Host "✓ 測試任務已建立: $testDir/main.py" -ForegroundColor Green

# 打包任務
Write-Host ""
Write-Host "[3/5] 打包任務..." -ForegroundColor Yellow
$zipPath = "$testDir.zip"
Compress-Archive -Path "$testDir/*" -DestinationPath $zipPath -Force
Write-Host "✓ 任務已打包: $zipPath" -ForegroundColor Green

# 提交任務
Write-Host ""
Write-Host "[4/5] 提交任務到 HiveMind..." -ForegroundColor Yellow

try {
    $response = curl.exe -X POST http://localhost:8082/api/tasks `
        -F "file=@$zipPath" `
        -F "cpu_score=2" `
        -F "memory_gb=1" `
        --silent

    $taskInfo = $response | ConvertFrom-Json
    $taskId = $taskInfo.task_id

    if ($taskInfo.success) {
        Write-Host "✓ 任務已提交" -ForegroundColor Green
        Write-Host "  任務 ID: $taskId" -ForegroundColor White
    } else {
        Write-Host "✗ 任務提交失敗: $($taskInfo.message)" -ForegroundColor Red
        exit 1
    }
} catch {
    Write-Host "✗ 提交任務時發生錯誤: $_" -ForegroundColor Red
    exit 1
}

# 等待任務完成
Write-Host ""
Write-Host "[5/5] 等待任務執行..." -ForegroundColor Yellow
Write-Host "  (最多等待 30 秒)" -ForegroundColor Gray

$maxWait = 30
$waited = 0
$completed = $false

while ($waited -lt $maxWait) {
    Start-Sleep -Seconds 2
    $waited += 2

    try {
        $statusResponse = curl.exe "http://localhost:8082/api/tasks/$taskId" --silent
        $status = $statusResponse | ConvertFrom-Json

        Write-Host "  [$waited 秒] 狀態: $($status.status)" -ForegroundColor Gray

        if ($status.status -eq "COMPLETED") {
            $completed = $true
            break
        } elseif ($status.status -eq "FAILED") {
            Write-Host "✗ 任務執行失敗" -ForegroundColor Red
            Write-Host "  錯誤: $($status.error)" -ForegroundColor Red
            exit 1
        }
    } catch {
        Write-Host "  查詢狀態時發生錯誤: $_" -ForegroundColor Red
    }
}

if ($completed) {
    Write-Host "✓ 任務執行完成！" -ForegroundColor Green

    # 顯示結果
    Write-Host ""
    Write-Host "==================================" -ForegroundColor Cyan
    Write-Host "  任務執行結果" -ForegroundColor Cyan
    Write-Host "==================================" -ForegroundColor Cyan
    Write-Host ""
    Write-Host "任務 ID: $taskId" -ForegroundColor White
    Write-Host "狀態: $($status.status)" -ForegroundColor Green
    Write-Host "Worker: $($status.worker_ip)" -ForegroundColor White
    Write-Host ""
    Write-Host "標準輸出:" -ForegroundColor Yellow
    Write-Host $status.stdout -ForegroundColor White

    if ($status.stderr) {
        Write-Host ""
        Write-Host "標準錯誤:" -ForegroundColor Yellow
        Write-Host $status.stderr -ForegroundColor Red
    }

    # 資源使用統計
    if ($status.resource_usage) {
        Write-Host ""
        Write-Host "資源使用:" -ForegroundColor Yellow
        Write-Host "  CPU: $($status.resource_usage.cpu_percent)%" -ForegroundColor White
        Write-Host "  記憶體: $($status.resource_usage.memory_mb) MB" -ForegroundColor White
    }
} else {
    Write-Host "✗ 任務執行超時（超過 $maxWait 秒）" -ForegroundColor Red
    exit 1
}

# 清理
Write-Host ""
Write-Host "清理測試檔案..." -ForegroundColor Gray
Remove-Item -Path $testDir -Recurse -Force -ErrorAction SilentlyContinue
Remove-Item -Path $zipPath -Force -ErrorAction SilentlyContinue

Write-Host ""
Write-Host "==================================" -ForegroundColor Cyan
Write-Host "  測試完成！" -ForegroundColor Green
Write-Host "==================================" -ForegroundColor Cyan
Write-Host ""
Write-Host "下一步:" -ForegroundColor Yellow
Write-Host "  1. 嘗試提交自己的 Python 任務" -ForegroundColor White
Write-Host "  2. 測試資源限制功能" -ForegroundColor White
Write-Host "  3. 啟動多個 Worker 測試負載均衡" -ForegroundColor White
Write-Host "  4. 查看詳細文檔: QUICK_START_GUIDE.md" -ForegroundColor White
Write-Host ""
