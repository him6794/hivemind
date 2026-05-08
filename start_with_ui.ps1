# HiveMind Complete System Startup (with Web UI)

Write-Host "==================================" -ForegroundColor Cyan
Write-Host "  HiveMind System Startup" -ForegroundColor Cyan
Write-Host "  (with Web UI)" -ForegroundColor Cyan
Write-Host "==================================" -ForegroundColor Cyan
Write-Host ""

# Check Docker
Write-Host "[1/8] Checking Docker..." -ForegroundColor Yellow
if (!(Get-Command docker -ErrorAction SilentlyContinue)) {
    Write-Host "X Docker not installed" -ForegroundColor Red
    exit 1
}
Write-Host "OK Docker installed" -ForegroundColor Green

# Check Node.js
Write-Host ""
Write-Host "[2/8] Checking Node.js..." -ForegroundColor Yellow
if (!(Get-Command node -ErrorAction SilentlyContinue)) {
    Write-Host "X Node.js not installed" -ForegroundColor Red
    exit 1
}
$nodeVersion = node --version
Write-Host "OK Node.js installed ($nodeVersion)" -ForegroundColor Green

# Start Redis
Write-Host ""
Write-Host "[3/8] Starting Redis..." -ForegroundColor Yellow
docker stop redis-hivemind 2>$null
docker rm redis-hivemind 2>$null
docker run -d -p 6379:6379 --name redis-hivemind redis:7-alpine | Out-Null
Start-Sleep -Seconds 2
Write-Host "OK Redis started (localhost:6379)" -ForegroundColor Green

# Start PostgreSQL
Write-Host ""
Write-Host "[4/8] Starting PostgreSQL..." -ForegroundColor Yellow
docker stop pg-hivemind 2>$null
docker rm pg-hivemind 2>$null
docker run -d -p 5432:5432 --name pg-hivemind `
  -e POSTGRES_USER=hivemind `
  -e POSTGRES_PASSWORD=hivemind `
  -e POSTGRES_DB=hivemind `
  postgres:16-alpine | Out-Null
Start-Sleep -Seconds 3
Write-Host "OK PostgreSQL started (localhost:5432)" -ForegroundColor Green

# Start Nodepool
Write-Host ""
Write-Host "[5/8] Starting Nodepool..." -ForegroundColor Yellow
$nodepoolJob = Start-Job -ScriptBlock {
    cd $using:PWD/services/nodepool/cmd/server
    $env:NODEPOOL_POSTGRES_DSN = "postgres://hivemind:hivemind@localhost:5432/hivemind?sslmode=disable"
    $env:NODEPOOL_REDIS_ADDR = "localhost:6379"
    $env:NODEPOOL_GRPC_PORT = "50051"
    go run . 2>&1
}
Start-Sleep -Seconds 3
Write-Host "OK Nodepool started (localhost:50051)" -ForegroundColor Green

# Start Master
Write-Host ""
Write-Host "[6/8] Starting Master..." -ForegroundColor Yellow
$masterJob = Start-Job -ScriptBlock {
    cd $using:PWD/services/master/cmd/server
    $env:MASTER_HTTP_PORT = "8082"
    $env:NODEPOOL_ADDR = "localhost:50051"
    go run . 2>&1
}
Start-Sleep -Seconds 3
Write-Host "OK Master started (http://localhost:8082)" -ForegroundColor Green

# Start Worker
Write-Host ""
Write-Host "[7/8] Starting Worker..." -ForegroundColor Yellow
$workerJob = Start-Job -ScriptBlock {
    cd $using:PWD/services/worker/cmd/server
    $env:WORKER_ID = "worker-001"
    $env:NODEPOOL_ADDR = "localhost:50051"
    $env:WORKER_GRPC_PORT = "50053"
    go run . 2>&1
}
Start-Sleep -Seconds 3
Write-Host "OK Worker started (localhost:50053)" -ForegroundColor Green

# Start Master UI
Write-Host ""
Write-Host "[8/8] Starting Web UI..." -ForegroundColor Yellow

# Check if dependencies need to be installed
if (!(Test-Path "frontend/master-ui/node_modules")) {
    Write-Host "  Installing Master UI dependencies..." -ForegroundColor Gray
    cd frontend/master-ui
    npm install --silent 2>&1 | Out-Null
    cd ../..
}

$masterUIJob = Start-Job -ScriptBlock {
    cd $using:PWD/frontend/master-ui
    npm run dev 2>&1
}

Start-Sleep -Seconds 5
Write-Host "OK Master UI started (http://localhost:3000)" -ForegroundColor Green

# Display status
Write-Host ""
Write-Host "==================================" -ForegroundColor Cyan
Write-Host "  HiveMind Started Successfully!" -ForegroundColor Green
Write-Host "==================================" -ForegroundColor Cyan
Write-Host ""
Write-Host "Backend Services:" -ForegroundColor Yellow
Write-Host "  * Redis:      localhost:6379" -ForegroundColor White
Write-Host "  * PostgreSQL: localhost:5432" -ForegroundColor White
Write-Host "  * Nodepool:   localhost:50051 (gRPC)" -ForegroundColor White
Write-Host "  * Master:     http://localhost:8082" -ForegroundColor White
Write-Host "  * Worker:     localhost:50053 (gRPC)" -ForegroundColor White
Write-Host ""
Write-Host "Web UI:" -ForegroundColor Yellow
Write-Host "  * Master UI:  http://localhost:3000" -ForegroundColor Cyan
Write-Host ""
Write-Host "==================================" -ForegroundColor Cyan
Write-Host "  Getting Started" -ForegroundColor Green
Write-Host "==================================" -ForegroundColor Cyan
Write-Host ""
Write-Host "1. Open browser: http://localhost:3000" -ForegroundColor White
Write-Host ""
Write-Host "2. Login:" -ForegroundColor White
Write-Host "   Username: testuser" -ForegroundColor Gray
Write-Host "   Password: testpass123" -ForegroundColor Gray
Write-Host ""
Write-Host "3. Upload task ZIP file" -ForegroundColor White
Write-Host "   (ZIP must contain main.py or other .py files)" -ForegroundColor Gray
Write-Host ""
Write-Host "4. View task status and results" -ForegroundColor White
Write-Host ""
Write-Host "==================================" -ForegroundColor Cyan
Write-Host ""
Write-Host "Test Tasks:" -ForegroundColor Yellow
Write-Host "  See test_tasks/ directory for sample tasks" -ForegroundColor White
Write-Host ""
Write-Host "Stop Services:" -ForegroundColor Yellow
Write-Host "  Press Ctrl+C or run ./stop_hivemind.ps1" -ForegroundColor White
Write-Host ""

# Auto open browser
Write-Host "Opening browser..." -ForegroundColor Cyan
Start-Sleep -Seconds 2
Start-Process "http://localhost:3000"

# Keep running
Write-Host ""
Write-Host "System is running... (Press Ctrl+C to stop)" -ForegroundColor Cyan
Write-Host ""

try {
    while ($true) {
        Start-Sleep -Seconds 1
    }
} finally {
    Write-Host ""
    Write-Host "Stopping services..." -ForegroundColor Yellow

    # Stop Jobs
    Stop-Job $nodepoolJob, $masterJob, $workerJob, $masterUIJob -ErrorAction SilentlyContinue
    Remove-Job $nodepoolJob, $masterJob, $workerJob, $masterUIJob -ErrorAction SilentlyContinue

    # Stop Docker containers
    docker stop redis-hivemind pg-hivemind 2>$null | Out-Null

    Write-Host "OK All services stopped" -ForegroundColor Green
}
