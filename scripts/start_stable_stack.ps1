param(
    [string]$RepoRoot = "D:\hivemind",
    [string]$LogDir = "D:\hivemind\test_logs\live\stable-stack",
    [switch]$RebuildBinaries
)

$ErrorActionPreference = "Stop"

function Ensure-Dir([string]$Path) {
    if (!(Test-Path $Path)) {
        New-Item -ItemType Directory -Path $Path | Out-Null
    }
}

function Wait-Port([int]$Port, [int]$TimeoutSec = 30) {
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    while ((Get-Date) -lt $deadline) {
        $line = netstat -ano | Select-String -Pattern (":$Port\s+.*LISTENING")
        if ($line) { return $true }
        Start-Sleep -Milliseconds 400
    }
    return $false
}

function Wait-DockerReady([string]$Name, [scriptblock]$Probe, [int]$TimeoutSec = 45) {
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    while ((Get-Date) -lt $deadline) {
        try {
            & $Probe | Out-Null
            if ($LASTEXITCODE -eq 0) {
                return $true
            }
        } catch {}
        Start-Sleep -Milliseconds 800
    }
    return $false
}

function Stop-ListeningProcesses([int[]]$Ports) {
    $ids = New-Object System.Collections.Generic.HashSet[int]
    foreach ($p in $Ports) {
        $lines = netstat -ano | Select-String -Pattern (":$p\s+.*LISTENING")
        foreach ($line in $lines) {
            $parts = ($line -split "\s+") | Where-Object { $_ -ne "" }
            if ($parts.Length -gt 0) {
                $procId = 0
                if ([int]::TryParse($parts[-1], [ref]$procId)) {
                    if ($procId -gt 0) {
                        [void]$ids.Add($procId)
                    }
                }
            }
        }
    }
    foreach ($id in $ids) {
        Stop-Process -Id $id -Force -ErrorAction SilentlyContinue
    }
}

Ensure-Dir $LogDir
$pidFile = Join-Path $LogDir "pids.json"

# Dependencies
docker rm -f hivemind-rel-pg hivemind-rel-redis 2>$null | Out-Null
docker run -d --name hivemind-rel-pg -e POSTGRES_USER=hivemind -e POSTGRES_PASSWORD=hivemind -e POSTGRES_DB=hivemind -p 25432:5432 postgres:16-alpine | Out-Null
docker run -d --name hivemind-rel-redis -p 26379:6379 redis:7-alpine | Out-Null

if (!(Wait-DockerReady "postgres" { docker exec hivemind-rel-pg pg_isready -U hivemind -d hivemind } 60)) {
    throw "postgres not ready within timeout"
}
if (!(Wait-DockerReady "redis" { docker exec hivemind-rel-redis redis-cli ping } 40)) {
    throw "redis not ready within timeout"
}

$nodepoolBin = Join-Path $RepoRoot "services\nodepool\nodepool.exe"
$masterBin = Join-Path $RepoRoot "services\master\master.exe"
$workerBin = Join-Path $RepoRoot "services\worker\worker.exe"
$executorBin = Join-Path $RepoRoot "services\worker\reliability-executor.exe"

if ($RebuildBinaries) {
    $env:GOTELEMETRY = "off"
    Push-Location (Join-Path $RepoRoot "services\nodepool")
    & go build -o $nodepoolBin .\cmd\server | Out-Null
    Pop-Location
    Push-Location (Join-Path $RepoRoot "services\master")
    & go build -o $masterBin .\cmd\server | Out-Null
    Pop-Location
    Push-Location (Join-Path $RepoRoot "services\worker")
    & go build -o $workerBin .\cmd\server | Out-Null
    & go build -o $executorBin .\cmd\reliability-executor | Out-Null
    Pop-Location
}

if (!(Test-Path $executorBin)) {
    Push-Location (Join-Path $RepoRoot "services\worker")
    & go build -o $executorBin .\cmd\reliability-executor | Out-Null
    Pop-Location
}

if (!(Test-Path $nodepoolBin) -or !(Test-Path $masterBin) -or !(Test-Path $workerBin) -or !(Test-Path $executorBin)) {
    throw "missing binaries: nodepool/master/worker/reliability-executor"
}

# Ensure target service ports are clean before launching.
Stop-ListeningProcesses @(18081, 18082, 50051, 51053, 51054, 51055)

# Stop old processes if we own them from previous run
if (Test-Path $pidFile) {
    try {
        $old = Get-Content $pidFile -Raw | ConvertFrom-Json
        foreach ($k in @("nodepool","master","worker1","worker2","worker3")) {
            $pid = $old.$k
            if ($pid) {
                Stop-Process -Id $pid -Force -ErrorAction SilentlyContinue
            }
        }
    } catch {}
}

$nodepoolEnv = @(
    "NODEPOOL_POSTGRES_DSN=postgres://hivemind:hivemind@127.0.0.1:25432/hivemind?sslmode=disable",
    "NODEPOOL_REDIS_ADDR=127.0.0.1:26379",
    "NODEPOOL_ENABLE_HTTP_AUTH=1",
    "NODEPOOL_HTTP_ADDR=:18081",
    "NODEPOOL_ADDR=:50051",
    "NODEPOOL_TASK_TIMEOUT_SEC=20",
    "NODEPOOL_MAX_REDISPATCH=3",
    "NODEPOOL_WORKER_PROBE_TIMEOUT_SEC=5"
)
foreach ($e in $nodepoolEnv) {
    $k, $v = $e -split "=", 2
    Set-Item -Path ("Env:" + $k) -Value $v
}
$pNodepool = Start-Process -FilePath $nodepoolBin -WindowStyle Hidden -RedirectStandardOutput (Join-Path $LogDir "nodepool.out.log") -RedirectStandardError (Join-Path $LogDir "nodepool.err.log") -PassThru

$masterEnv = @(
    "MASTER_HTTP_ADDR=:18082",
    "NODEPOOL_GRPC_ADDR=127.0.0.1:50051",
    "NODEPOOL_HTTP_BASE=http://127.0.0.1:18081",
    "MASTER_NODEPOOL_TIMEOUT_SEC=30"
)
foreach ($e in $masterEnv) {
    $k, $v = $e -split "=", 2
    Set-Item -Path ("Env:" + $k) -Value $v
}
$pMaster = Start-Process -FilePath $masterBin -WindowStyle Hidden -RedirectStandardOutput (Join-Path $LogDir "master.out.log") -RedirectStandardError (Join-Path $LogDir "master.err.log") -PassThru

$commonWorkerEnv = @(
    "NODEPOOL_ADDR=127.0.0.1:50051",
    "WORKER_PASSWORD=worker123",
    "WORKER_AUTO_REGISTER=1",
    "WORKER_REGISTER_TIMEOUT_SEC=15",
    "WORKER_REGISTER_RETRY_INTERVAL_SEC=2",
    "WORKER_EXECUTOR_CMD=$executorBin",
    "WORKER_EXECUTOR_TIMEOUT_SEC=1800",
    "WORKER_USAGE_REPORT_INTERVAL_SEC=2"
)
foreach ($e in $commonWorkerEnv) {
    $k, $v = $e -split "=", 2
    Set-Item -Path ("Env:" + $k) -Value $v
}

$workers = @(
    @{ Name = "worker1"; Addr=":51053"; Public="127.0.0.1:51053"; Control=":18080"; Out="worker1.out.log"; Err="worker1.err.log" },
    @{ Name = "worker2"; Addr=":51054"; Public="127.0.0.1:51054"; Control=":18083"; Out="worker2.out.log"; Err="worker2.err.log" },
    @{ Name = "worker3"; Addr=":51055"; Public="127.0.0.1:51055"; Control=":18084"; Out="worker3.out.log"; Err="worker3.err.log" }
)

$pWorkers = @{}
foreach ($w in $workers) {
    $env:WORKER_USERNAME = $w.Name
    $env:WORKER_ADDR = $w.Addr
    $env:WORKER_PUBLIC_ADDR = $w.Public
    $env:WORKER_CONTROL_ADDR = $w.Control
    $pw = Start-Process -FilePath $workerBin -WindowStyle Hidden -RedirectStandardOutput (Join-Path $LogDir $w.Out) -RedirectStandardError (Join-Path $LogDir $w.Err) -PassThru
    $pWorkers[$w.Name] = $pw.Id
}

if (!(Wait-Port 18081 25) -or !(Wait-Port 18082 25) -or !(Wait-Port 50051 25) -or !(Wait-Port 51053 25) -or !(Wait-Port 51054 25) -or !(Wait-Port 51055 25)) {
    throw "stable stack start failed: some ports not listening"
}

$pids = [ordered]@{
    nodepool = $pNodepool.Id
    master   = $pMaster.Id
    worker1  = $pWorkers["worker1"]
    worker2  = $pWorkers["worker2"]
    worker3  = $pWorkers["worker3"]
}
$pids | ConvertTo-Json | Set-Content -Path $pidFile -Encoding UTF8

Write-Output "stable stack started"
Write-Output (ConvertTo-Json $pids -Compress)
