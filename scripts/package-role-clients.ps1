param(
    [ValidateSet("release", "debug")]
    [string]$Configuration = "release",
    [string]$OutputDir = "dist\role-clients",
    [string]$MontyExecutable = "",
    [switch]$SkipBuild,
    [switch]$SkipFrontendBuild,
    [ValidateSet("all", "master", "worker", "nodepool")]
    [string]$Role = "all"
)

$ErrorActionPreference = "Stop"

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$rustRoot = Join-Path $repoRoot "hivemind-rs"
$outputRoot = Join-Path $repoRoot $OutputDir
$targetDir = if ($Configuration -eq "release") {
    Join-Path $rustRoot "target\release"
} else {
    Join-Path $rustRoot "target\debug"
}

function Assert-PathLeaf {
    param([Parameter(Mandatory = $true)][string]$Path)
    if (!(Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "Required file was not found: $Path"
    }
}

function Build-RoleBinary {
    param(
        [Parameter(Mandatory = $true)][string]$Bin,
        [Parameter(Mandatory = $true)][string]$Features
    )

    if ($SkipBuild) {
        $binary = Join-Path $targetDir "$Bin.exe"
        Assert-PathLeaf -Path $binary
        return $binary
    }

    Push-Location $rustRoot
    try {
        if ($Configuration -eq "release") {
            cargo build --release --no-default-features --features $Features --bin $Bin
        } else {
            cargo build --no-default-features --features $Features --bin $Bin
        }
    } finally {
        Pop-Location
    }

    $binary = Join-Path $targetDir "$Bin.exe"
    Assert-PathLeaf -Path $binary
    return $binary
}

function Build-Frontend {
    param(
        [Parameter(Mandatory = $true)][string]$RelativePath,
        [Parameter(Mandatory = $true)][string]$Destination
    )

    $appRoot = Join-Path $repoRoot $RelativePath
    $distRoot = Join-Path $appRoot "dist"
    if (!$SkipFrontendBuild) {
        Push-Location $appRoot
        try {
            if (Test-Path (Join-Path $appRoot "package-lock.json")) {
                npm ci --ignore-scripts
            } else {
                npm install --ignore-scripts
            }
            npm run build
        } finally {
            Pop-Location
        }
    }
    if (!(Test-Path -LiteralPath $distRoot -PathType Container)) {
        throw "Frontend dist was not found for $RelativePath at $distRoot. Build the UI or omit -SkipFrontendBuild."
    }
    New-Item -ItemType Directory -Force -Path $Destination | Out-Null
    Copy-Item -Recurse -Force -Path (Join-Path $distRoot "*") -Destination $Destination
}

function Write-ShaAndManifest {
    param(
        [Parameter(Mandatory = $true)][string]$PackageDir,
        [Parameter(Mandatory = $true)][string]$PackageName,
        [Parameter(Mandatory = $true)][hashtable]$Extra
    )

    $gitCommit = try { (git -C $repoRoot rev-parse HEAD 2>$null).Trim() } catch { "unknown" }
    $gitDirty = $true
    try {
        $gitDirty = -not [string]::IsNullOrWhiteSpace((git -C $repoRoot status --porcelain 2>$null))
    } catch {
        $gitDirty = $true
    }

    $artifacts = Get-ChildItem -Recurse -File $PackageDir | Where-Object {
        $_.Name -ne "SHA256SUMS" -and $_.Name -ne "manifest.json"
    } | ForEach-Object {
        $rel = $_.FullName.Substring($PackageDir.Length + 1) -replace '\\', '/'
        $hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $_.FullName).Hash.ToLowerInvariant()
        [ordered]@{
            name = $rel
            sha256 = $hash
        }
    }

    $shaLines = $artifacts | ForEach-Object { "{0} *{1}" -f $_.sha256, $_.name }
    $shaLines | Sort-Object | Set-Content -Encoding ASCII -Path (Join-Path $PackageDir "SHA256SUMS")

    $manifest = [ordered]@{
        package = $PackageName
        configuration = $Configuration
        generated_at_utc = (Get-Date).ToUniversalTime().ToString("o")
        git_commit = $gitCommit
        git_dirty = $gitDirty
        artifacts = @($artifacts)
    }
    foreach ($key in $Extra.Keys) {
        $manifest[$key] = $Extra[$key]
    }
    $manifest | ConvertTo-Json -Depth 6 | Set-Content -Encoding UTF8 -Path (Join-Path $PackageDir "manifest.json")
}

function New-CleanPackageDir {
    param([Parameter(Mandatory = $true)][string]$Name)
    $dir = Join-Path $outputRoot $Name
    if (Test-Path $dir) {
        Remove-Item -Recurse -Force -LiteralPath $dir
    }
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    return $dir
}

function Get-DotEnvHelperText {
    return @'
function Import-DotEnv {
    param([Parameter(Mandatory = $true)][string]$Path)

    $seen = @{}
    $lineNumber = 0
    foreach ($rawLine in Get-Content -LiteralPath $Path) {
        $lineNumber += 1
        $trimmed = $rawLine.Trim()
        if ($trimmed -eq "" -or $trimmed.StartsWith("#")) {
            continue
        }

        if ($rawLine -notmatch '^\s*([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(.*)\s*$') {
            throw "Invalid env file line ${lineNumber}: expected KEY=VALUE with a valid environment variable name."
        }

        $key = $matches[1]
        $value = $matches[2].Trim()
        if ($seen.ContainsKey($key)) {
            throw "Duplicate env key '$key' on line ${lineNumber}."
        }

        if ($value.Length -ge 2) {
            $first = $value.Substring(0, 1)
            $last = $value.Substring($value.Length - 1, 1)
            if (($first -eq '"' -and $last -eq '"') -or ($first -eq "'" -and $last -eq "'")) {
                $value = $value.Substring(1, $value.Length - 2)
            }
        }

        [Environment]::SetEnvironmentVariable($key, $value, "Process")
        $seen[$key] = $true
    }
}

function Assert-RequiredEnv {
    param([Parameter(Mandatory = $true)][string[]]$Names)

    foreach ($name in $Names) {
        $value = [Environment]::GetEnvironmentVariable($name, "Process")
        if ([string]::IsNullOrWhiteSpace($value)) {
            throw "Required setting $name is missing or blank."
        }
    }
}
'@
}

function Package-WorkerClient {
    if ([string]::IsNullOrWhiteSpace($MontyExecutable)) {
        $candidates = @(
            (Join-Path $repoRoot "executor-rs\target\release\monty.exe"),
            (Join-Path $repoRoot "executor-rs\target\debug\monty.exe"),
            (Join-Path $repoRoot "hivemind-rs\target\release\monty.exe")
        )
        foreach ($candidate in $candidates) {
            if (Test-Path -LiteralPath $candidate -PathType Leaf) {
                $script:MontyExecutable = $candidate
                break
            }
        }
    }
    if ([string]::IsNullOrWhiteSpace($MontyExecutable)) {
        throw "MontyExecutable is required for the worker client package. Pass -MontyExecutable path\to\monty.exe"
    }
    Assert-PathLeaf -Path $MontyExecutable

    $binary = Build-RoleBinary -Bin "hivemind-worker" -Features "worker"
    $packageDir = New-CleanPackageDir -Name "worker-client"
    $uiDir = Join-Path $packageDir "ui\worker"
    Build-Frontend -RelativePath "frontend\worker-ui" -Destination $uiDir

    Copy-Item -LiteralPath $binary -Destination (Join-Path $packageDir "hivemind-worker.exe")
    Copy-Item -LiteralPath $MontyExecutable -Destination (Join-Path $packageDir "monty.exe")

    @"
# HiveMind Windows worker client
NODEPOOL_GRPC_ENDPOINT=nodepool.example.com:50051
WORKER_GRPC_ADDR=0.0.0.0:50053
WORKER_CONTROL_HTTP_ADDR=127.0.0.1:18080
# Address reachable by nodepool / other hosts.
WORKER_ADVERTISE_ADDR=
WORKER_NODEPOOL_TOKEN=
WORKER_NODEPOOL_USERNAME=
WORKER_NODEPOOL_PASSWORD=
WORKER_ID=$env:COMPUTERNAME
WORKER_LOCATION=windows
# Must match the non-default worker-execution secret configured on nodepool.
WORKER_EXECUTION_SECRET=
MONTY_EXECUTABLE=monty.exe
WORKER_UI_DIR=.\ui\worker
EXECUTOR_SANDBOX_DIR=.\sandbox
EXECUTOR_MAX_CPU_PERCENT=80
EXECUTOR_MAX_MEMORY_MB=4096
EXECUTOR_TASK_TIMEOUT_SECS=3600
EXECUTOR_MAX_CONCURRENT_TASKS=2
EXECUTOR_SANDBOX_MODE=production
EXECUTOR_NETWORK_EGRESS_ENABLED=true
EXECUTOR_NETWORK_EGRESS_MODE=allowlist
EXECUTOR_NETWORK_EGRESS_TARGETS=127.0.0.1
TORRENT_ALLOW_LOCAL_TASK_ARTIFACTS=false
TORRENT_TASK_ARTIFACT_BASE_URL=
"@ | Set-Content -Encoding ASCII (Join-Path $packageDir ".env.worker.example")

    $helper = Get-DotEnvHelperText
    @"
`$ErrorActionPreference = "Stop"
$helper

`$envFile = Join-Path `$PSScriptRoot ".env.worker"
if (!(Test-Path `$envFile)) {
    Copy-Item (Join-Path `$PSScriptRoot ".env.worker.example") `$envFile
    Write-Host "Created .env.worker from template. Fill required values and re-run."
    exit 1
}

Import-DotEnv -Path `$envFile
Assert-RequiredEnv -Names @(
    "NODEPOOL_GRPC_ENDPOINT",
    "WORKER_GRPC_ADDR",
    "WORKER_CONTROL_HTTP_ADDR",
    "WORKER_ADVERTISE_ADDR",
    "WORKER_EXECUTION_SECRET"
)

`$workerNodepoolToken = [Environment]::GetEnvironmentVariable("WORKER_NODEPOOL_TOKEN", "Process")
`$workerNodepoolUsername = [Environment]::GetEnvironmentVariable("WORKER_NODEPOOL_USERNAME", "Process")
`$workerNodepoolPassword = [Environment]::GetEnvironmentVariable("WORKER_NODEPOOL_PASSWORD", "Process")
if ([string]::IsNullOrWhiteSpace(`$workerNodepoolToken) -and
    ([string]::IsNullOrWhiteSpace(`$workerNodepoolUsername) -or [string]::IsNullOrWhiteSpace(`$workerNodepoolPassword))) {
    throw "Set WORKER_NODEPOOL_TOKEN, or set both WORKER_NODEPOOL_USERNAME and WORKER_NODEPOOL_PASSWORD."
}

if ([string]::IsNullOrWhiteSpace([Environment]::GetEnvironmentVariable("WORKER_UI_DIR", "Process"))) {
    [Environment]::SetEnvironmentVariable("WORKER_UI_DIR", (Join-Path `$PSScriptRoot "ui\worker"), "Process")
}
if ([string]::IsNullOrWhiteSpace([Environment]::GetEnvironmentVariable("MONTY_EXECUTABLE", "Process"))) {
    [Environment]::SetEnvironmentVariable("MONTY_EXECUTABLE", (Join-Path `$PSScriptRoot "monty.exe"), "Process")
}

`$workerUiDir = [Environment]::GetEnvironmentVariable("WORKER_UI_DIR", "Process")
if (!(Test-Path -LiteralPath `$workerUiDir -PathType Container)) {
    throw "Worker UI directory was not found: `$workerUiDir"
}

Write-Host "Starting hivemind-worker with embedded Worker UI at `$workerUiDir"
Write-Host "Open http://127.0.0.1:18080/ (or WORKER_CONTROL_HTTP_ADDR) for the Worker UI."
& (Join-Path `$PSScriptRoot "hivemind-worker.exe")
"@ | Set-Content -Encoding ASCII (Join-Path $packageDir "start-worker.ps1")

    @"
# HiveMind Worker Client

This package is the multi-host worker client:

- `hivemind-worker.exe` role binary (worker feature only)
- embedded Worker UI under `ui/worker`
- pinned Monty runtime `monty.exe`

## Setup

1. Copy `.env.worker.example` to `.env.worker`.
2. Set `NODEPOOL_GRPC_ENDPOINT` to a reachable nodepool.
3. Set `WORKER_ADVERTISE_ADDR` to a routable host:port for this worker.
4. Set `WORKER_EXECUTION_SECRET` and either `WORKER_NODEPOOL_TOKEN` or both `WORKER_NODEPOOL_USERNAME` / `WORKER_NODEPOOL_PASSWORD`.
5. Run:

```powershell
.\start-worker.ps1
```

The worker process starts gRPC execution, the local control API, and serves the Worker UI from the same process. Do not ship the control-plane `JWT_SECRET` to workers.
"@ | Set-Content -Encoding UTF8 (Join-Path $packageDir "README.md")

    Write-ShaAndManifest -PackageDir $packageDir -PackageName "hivemind-worker-client" -Extra @{
        role = "worker"
        binary = "hivemind-worker.exe"
        ui = @("worker")
        serves_ui_on_start = $true
        ui_listen_hint = "WORKER_CONTROL_HTTP_ADDR"
    }
    Write-Host "Worker client package written to $packageDir"
}

function Package-MasterClient {
    $binary = Build-RoleBinary -Bin "hivemind-master" -Features "master"
    $packageDir = New-CleanPackageDir -Name "master-client"
    $uiDir = Join-Path $packageDir "ui\master"
    Build-Frontend -RelativePath "frontend\master-ui" -Destination $uiDir

    Copy-Item -LiteralPath $binary -Destination (Join-Path $packageDir "hivemind-master.exe")

    @"
# HiveMind Windows master client
JWT_SECRET=
NODEPOOL_GRPC_ENDPOINT=nodepool.example.com:50051
MASTER_HTTP_ADDR=0.0.0.0:8082
MASTER_UI_DIR=.\ui\master
HIVEMIND_ADMIN_USERS=
HIVEMIND_TASK_SUBMIT_LIMIT_PER_MINUTE=60
LOG_LEVEL=info
"@ | Set-Content -Encoding ASCII (Join-Path $packageDir ".env.master.example")

    $helper = Get-DotEnvHelperText
    @"
`$ErrorActionPreference = "Stop"
$helper

`$envFile = Join-Path `$PSScriptRoot ".env.master"
if (!(Test-Path `$envFile)) {
    Copy-Item (Join-Path `$PSScriptRoot ".env.master.example") `$envFile
    Write-Host "Created .env.master from template. Fill required values and re-run."
    exit 1
}

Import-DotEnv -Path `$envFile
Assert-RequiredEnv -Names @("JWT_SECRET", "NODEPOOL_GRPC_ENDPOINT", "MASTER_HTTP_ADDR")

if ([string]::IsNullOrWhiteSpace([Environment]::GetEnvironmentVariable("MASTER_UI_DIR", "Process"))) {
    [Environment]::SetEnvironmentVariable("MASTER_UI_DIR", (Join-Path `$PSScriptRoot "ui\master"), "Process")
}

`$masterUiDir = [Environment]::GetEnvironmentVariable("MASTER_UI_DIR", "Process")
if (!(Test-Path -LiteralPath `$masterUiDir -PathType Container)) {
    throw "Master UI directory was not found: `$masterUiDir"
}

Write-Host "Starting hivemind-master with embedded Master UI at `$masterUiDir"
Write-Host "Open http://127.0.0.1:8082/ (or MASTER_HTTP_ADDR) for the Master UI."
& (Join-Path `$PSScriptRoot "hivemind-master.exe")
"@ | Set-Content -Encoding ASCII (Join-Path $packageDir "start-master.ps1")

    @"
# HiveMind Master Client

This package is the multi-host master client:

- `hivemind-master.exe` role binary (master feature only)
- embedded Master UI under `ui/master`

## Setup

1. Copy `.env.master.example` to `.env.master`.
2. Set `JWT_SECRET` to the same non-default control-plane secret used by nodepool.
3. Set `NODEPOOL_GRPC_ENDPOINT` to a reachable nodepool.
4. Run:

```powershell
.\start-master.ps1
```

The master process starts the HTTP API and serves the Master UI from the same process.
"@ | Set-Content -Encoding UTF8 (Join-Path $packageDir "README.md")

    Write-ShaAndManifest -PackageDir $packageDir -PackageName "hivemind-master-client" -Extra @{
        role = "master"
        binary = "hivemind-master.exe"
        ui = @("master")
        serves_ui_on_start = $true
        ui_listen_hint = "MASTER_HTTP_ADDR"
    }
    Write-Host "Master client package written to $packageDir"
}

function Package-NodepoolServer {
    $binary = Build-RoleBinary -Bin "hivemind-nodepool" -Features "nodepool"
    $packageDir = New-CleanPackageDir -Name "nodepool-server"

    Copy-Item -LiteralPath $binary -Destination (Join-Path $packageDir "hivemind-nodepool.exe")

    @"
# HiveMind nodepool server
DATABASE_URL=postgres://hivemind:change-me@localhost:5432/hivemind
REDIS_URL=redis://localhost:6379
JWT_SECRET=
WORKER_EXECUTION_SECRET=
NODEPOOL_GRPC_ADDR=0.0.0.0:50051
TORRENT_API_DIR=.\api\torrents
TORRENT_BT_DIR=.\bt_torrents
TORRENT_ANNOUNCE_URL=http://nodepool.example.com:6969/announce
TORRENT_TRACKER_LISTEN_ADDR=0.0.0.0:6969
TORRENT_SEED_LISTEN_ADDR=0.0.0.0:6881
TORRENT_SEED_ADVERTISE_HOST=nodepool.example.com
HIVEMIND_SEED_DEFAULT_USER=false
HIVEMIND_ADMIN_USERS=
LOG_LEVEL=info
"@ | Set-Content -Encoding ASCII (Join-Path $packageDir ".env.nodepool.example")

    $helper = Get-DotEnvHelperText
    @"
`$ErrorActionPreference = "Stop"
$helper

`$envFile = Join-Path `$PSScriptRoot ".env.nodepool"
if (!(Test-Path `$envFile)) {
    Copy-Item (Join-Path `$PSScriptRoot ".env.nodepool.example") `$envFile
    Write-Host "Created .env.nodepool from template. Fill required values and re-run."
    exit 1
}

Import-DotEnv -Path `$envFile
Assert-RequiredEnv -Names @(
    "DATABASE_URL",
    "REDIS_URL",
    "JWT_SECRET",
    "WORKER_EXECUTION_SECRET",
    "NODEPOOL_GRPC_ADDR"
)

New-Item -ItemType Directory -Force -Path (Join-Path `$PSScriptRoot "api\torrents") | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path `$PSScriptRoot "bt_torrents") | Out-Null

Write-Host "Starting hivemind-nodepool server"
& (Join-Path `$PSScriptRoot "hivemind-nodepool.exe")
"@ | Set-Content -Encoding ASCII (Join-Path $packageDir "start-nodepool.ps1")

    @"
# HiveMind Nodepool Server

This package is the multi-host coordination server:

- `hivemind-nodepool.exe` role binary (nodepool feature only)
- requires PostgreSQL and Redis
- owns worker registration, scheduling, and BT package seeding

## Setup

1. Provision PostgreSQL and Redis.
2. Copy `.env.nodepool.example` to `.env.nodepool`.
3. Set non-default `JWT_SECRET` and `WORKER_EXECUTION_SECRET`.
4. Set torrent advertise/announce hosts to routable addresses for multi-host workers.
5. Run:

```powershell
.\start-nodepool.ps1
```

Workers and masters connect to this nodepool over gRPC. Do not expose PostgreSQL or Redis publicly.
"@ | Set-Content -Encoding UTF8 (Join-Path $packageDir "README.md")

    Write-ShaAndManifest -PackageDir $packageDir -PackageName "hivemind-nodepool-server" -Extra @{
        role = "nodepool"
        binary = "hivemind-nodepool.exe"
        ui = @()
        serves_ui_on_start = $false
    }
    Write-Host "Nodepool server package written to $packageDir"
}

New-Item -ItemType Directory -Force -Path $outputRoot | Out-Null

switch ($Role) {
    "worker" { Package-WorkerClient }
    "master" { Package-MasterClient }
    "nodepool" { Package-NodepoolServer }
    default {
        Package-NodepoolServer
        Package-MasterClient
        Package-WorkerClient
    }
}

Write-Host "Role client packages ready under $outputRoot"
