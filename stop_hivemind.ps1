#!/usr/bin/env pwsh
# HiveMind 停止腳本

Write-Host "正在停止 HiveMind 服務..." -ForegroundColor Yellow

# 停止所有 Go 進程
Write-Host "停止 Go 進程..." -ForegroundColor Cyan
Get-Process | Where-Object {$_.ProcessName -like "*go*"} | Stop-Process -Force -ErrorAction SilentlyContinue

# 停止 Docker 容器
Write-Host "停止 Docker 容器..." -ForegroundColor Cyan
docker stop redis-hivemind pg-hivemind 2>$null | Out-Null
docker rm redis-hivemind pg-hivemind 2>$null | Out-Null

Write-Host "✓ 所有服務已停止" -ForegroundColor Green
