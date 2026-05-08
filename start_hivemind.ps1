#!/usr/bin/env pwsh
# HiveMind 一鍵啟動腳本

Write-Host "==================================" -ForegroundColor Cyan
Write-Host "  HiveMind 快速啟動腳本" -ForegroundColor Cyan
Write-Host "==================================" -ForegroundColor Cyan
Write-Host ""

# 檢查 Docker
Write-Host "[1/6] 檢查 Docker..." -ForegroundColor Yellow
if (!(Get-Command docker -ErrorAction SilentlyContinue)) {
    Write-Host "✗ Docker 未安裝，請先安裝 Docker Desktop" -ForegroundColor Red
    exit 1
}
Write-Host "✓ Docker 已安裝" -ForegroundColor Green

# 啟動 Redis
Write-Host ""
Write-Host "[2/6] 啟動 Redis..." -ForegroundColor Yellow
docker stop redis-hivemind 2>$null
docker rm redis-hivemind 2>$null
docker run -d -p 6379:6379 --name redis-hivemind redis:7-alpine | Out-Null
Start-Sleep -Seconds 2

# 測試 Redis 連接
$redisTest = docker exec redis-hivemind redis-cli ping 2>$null
if ($redisTest -eq "PONG") {
    Write-Host "✓ Redis 已啟動 (localhost:6379)" -ForegroundColor Green
} else {
    Write-Host "✗ Redis 啟動失敗" -ForegroundColor Red
    exit 1
}

# 啟動 PostgreSQL
Write-Host ""
Write-Host "[3/6] 啟動 PostgreSQL..." -ForegroundColor Yellow
docker stop pg-hivemind 2>$null
docker rm pg-hivemind 2>$null
docker run -d -p 5432:5432 --name pg-hivemind `
  -e POSTGRES_USER=hivemind `
  -e POSTGRES_PASSWORD=hivemind `
  -e POSTGRES_DB=hivemind `
  postgres:16-alpine | Out-Null
Start-Sleep -Seconds 3
Write-Host "✓ PostgreSQL 已啟動 (localhost:5432)" -ForegroundColor Green

# 啟動 Nodepool
Write-Host ""
Write-Host "[4/6] 啟動 Nodepool..." -ForegroundColor Yellow
$nodepoolJob = Start-Job -ScriptBlock {
    cd $using:PWD/services/nodepool/cmd/server
    $env:NODEPOOL_POSTGRES_DSN = "postgres://hivemind:hivemind@localhost:5432/hivemind?sslmode=disable"
    $env:NODEPOOL_REDIS_ADDR = "localhost:6379"
    $env:NODEPOOL_GRPC_PORT = "50051"
    go run . 2>&1
}
Start-Sleep -Seconds 3
Write-Host "✓ Nodepool 已啟動 (localhost:50051)" -ForegroundColor Green

# 啟動 Master
Write-Host ""
Write-Host "[5/6] 啟動 Master..." -ForegroundColor Yellow
$masterJob = Start-Job -ScriptBlock {
    cd $using:PWD/services/master/cmd/server
    $env:MASTER_HTTP_PORT = "8082"
    $env:NODEPOOL_ADDR = "localhost:50051"
    go run . 2>&1
}
Start-Sleep -Seconds 3
Write-Host "✓ Master 已啟動 (http://localhost:8082)" -ForegroundColor Green

# 啟動 Worker
Write-Host ""
Write-Host "[6/6] 啟動 Worker..." -ForegroundColor Yellow
$workerJob = Start-Job -ScriptBlock {
    cd $using:PWD/services/worker/cmd/server
    $env:WORKER_ID = "worker-001"
    $env:NODEPOOL_ADDR = "localhost:50051"
    $env:WORKER_GRPC_PORT = "50053"
    go run . 2>&1
}
Start-Sleep -Seconds 3
Write-Host "✓ Worker 已啟動 (localhost:50053)" -ForegroundColor Green

# 顯示狀態
Write-Host ""
Write-Host "==================================" -ForegroundColor Cyan
Write-Host "  HiveMind 已成功啟動！" -ForegroundColor Green
Write-Host "==================================" -ForegroundColor Cyan
Write-Host ""
Write-Host "服務狀態:" -ForegroundColor Yellow
Write-Host "  • Redis:      localhost:6379" -ForegroundColor White
Write-Host "  • PostgreSQL: localhost:5432" -ForegroundColor White
Write-Host "  • Nodepool:   localhost:50051 (gRPC)" -ForegroundColor White
Write-Host "  • Master:     http://localhost:8082" -ForegroundColor White
Write-Host "  • Worker:     localhost:50053 (gRPC)" -ForegroundColor White
Write-Host ""
Write-Host "快速測試:" -ForegroundColor Yellow
Write-Host "  1. 建立測試任務: echo 'print(\"Hello HiveMind\")' > test.py" -ForegroundColor White
Write-Host "  2. 打包任務: Compress-Archive -Path test.py -DestinationPath task.zip" -ForegroundColor White
Write-Host "  3. 提交任務: curl -X POST http://localhost:8082/api/tasks -F 'file=@task.zip'" -ForegroundColor White
Write-Host ""
Write-Host "查看日誌:" -ForegroundColor Yellow
Write-Host "  • Nodepool: Receive-Job $($nodepoolJob.Id)" -ForegroundColor White
Write-Host "  • Master:   Receive-Job $($masterJob.Id)" -ForegroundColor White
Write-Host "  • Worker:   Receive-Job $($workerJob.Id)" -ForegroundColor White
Write-Host ""
Write-Host "停止服務: 按 Ctrl+C 或執行 ./stop_hivemind.ps1" -ForegroundColor Yellow
Write-Host ""

# 保持運行
Write-Host "按 Ctrl+C 停止所有服務..." -ForegroundColor Cyan
try {
    while ($true) {
        Start-Sleep -Seconds 1
    }
} finally {
    Write-Host ""
    Write-Host "正在停止服務..." -ForegroundColor Yellow

    # 停止 Jobs
    Stop-Job $nodepoolJob, $masterJob, $workerJob -ErrorAction SilentlyContinue
    Remove-Job $nodepoolJob, $masterJob, $workerJob -ErrorAction SilentlyContinue

    # 停止 Docker 容器
    docker stop redis-hivemind pg-hivemind 2>$null | Out-Null

    Write-Host "✓ 所有服務已停止" -ForegroundColor Green
}
