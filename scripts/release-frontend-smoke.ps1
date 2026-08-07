param(
    [switch]$SkipBuild,
    [switch]$CheckOnly,
    [ValidateRange(1, 600)]
    [int]$StartupTimeoutSeconds = 30
)

$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$surfaces = @(
    @{
        Name = "official-site"
        Path = "frontend"
        Port = 4173
        RequiredEnv = @("WEBSITE_NODEPOOL_GRPC_ADDR")
        Artifact = ".next/standalone/server.js"
        Framework = "next"
        ExpectedContent = "Hivemind"
    },
    @{
        Name = "master-ui"
        Path = "frontend/master-ui"
        Port = 4174
        RequiredEnv = @("VITE_API_BASE")
        Artifact = "dist/index.html"
        Framework = "vite"
        ExpectedContent = '<div id="root">'
    },
    @{
        Name = "worker-ui"
        Path = "frontend/worker-ui"
        Port = 4175
        RequiredEnv = @("VITE_API_BASE", "VITE_WORKER_CONTROL_BASE")
        Artifact = "dist/index.html"
        Framework = "vite"
        ExpectedContent = '<div id="root">'
    }
)

function Get-RequiredEnvValue {
    param([string]$Name)

    $value = [Environment]::GetEnvironmentVariable($Name, "Process")
    if ([string]::IsNullOrWhiteSpace($value)) {
        throw "Missing required release env $Name. Set $Name before running the frontend release smoke harness."
    }
    return $value
}

function Invoke-CheckedCommand {
    param(
        [string]$Command,
        [string[]]$Arguments,
        [string]$WorkingDirectory
    )

    Write-Host "RUN $Command $($Arguments -join ' ') [$WorkingDirectory]"
    $previousLocation = Get-Location
    try {
        Set-Location -LiteralPath $WorkingDirectory
        & $Command @Arguments
        if ($LASTEXITCODE -ne 0) {
            throw "$Command exited with code $LASTEXITCODE in $WorkingDirectory."
        }
    }
    finally {
        Set-Location $previousLocation
    }
}

function Wait-ForHttpOk {
    param(
        [string]$Uri,
        [int]$TimeoutSeconds,
        [string]$ExpectedContent
    )

    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    $lastError = $null
    while ((Get-Date) -lt $deadline) {
        try {
            $response = Invoke-WebRequest -Uri $Uri -UseBasicParsing -TimeoutSec 5
            if ($response.StatusCode -ge 200 -and $response.StatusCode -lt 400 -and $response.Content.Contains($ExpectedContent)) {
                return
            }
            $lastError = "Unexpected response from $Uri status=$($response.StatusCode)"
        }
        catch {
            $lastError = $_.Exception.Message
        }
        Start-Sleep -Milliseconds 500
    }

    throw "Timed out waiting for $Uri. Last error: $lastError"
}

function Test-PortOccupied {
    param([int]$Port)

    try {
        $listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, $Port)
        $listener.Start()
        $listener.Stop()
        return $false
    }
    catch {
        return $true
    }
}

function Stop-PreviewProcessTree {
    param([Diagnostics.Process]$Process)

    if ($null -eq $Process -or $Process.HasExited) {
        return
    }

    if ($env:OS -eq "Windows_NT") {
        & taskkill.exe /PID ([string]$Process.Id) /T /F 2>$null | Out-Null
    }
    else {
        Stop-Process -Id $Process.Id -Force -ErrorAction SilentlyContinue
    }
}

$processes = @()
try {
    $buildScriptPath = Join-Path $PSScriptRoot "build-release-frontends.ps1"
    if (!(Test-Path -LiteralPath $buildScriptPath)) {
        throw "Missing shared build helper $buildScriptPath."
    }

    if (!$SkipBuild) {
        Invoke-CheckedCommand -Command "powershell" -Arguments @("-NoProfile", "-ExecutionPolicy", "Bypass", "-File", $buildScriptPath) -WorkingDirectory $repoRoot
    }

    foreach ($surface in $surfaces) {
        foreach ($envName in $surface.RequiredEnv) {
            [void](Get-RequiredEnvValue -Name $envName)
        }

        $surfacePath = Join-Path $repoRoot $surface.Path
        if (!(Test-Path -LiteralPath (Join-Path $surfacePath "package.json"))) {
            throw "Missing package.json for $($surface.Name) at $surfacePath."
        }

        $artifactPath = Join-Path $surfacePath $surface.Artifact
        if (!(Test-Path -LiteralPath $artifactPath)) {
            throw "Missing build artifact for $($surface.Name): $artifactPath. Run the harness without -SkipBuild or run npm run build first."
        }

        Write-Host "CHECK $($surface.Name) artifact $artifactPath"
    }

    if ($CheckOnly) {
        Write-Host "release frontend smoke check-only passed"
        exit 0
    }

    foreach ($surface in $surfaces) {
        $port = $surface.Port
        if (Test-PortOccupied -Port $port) {
            throw "Port $port is already occupied ($($surface.Name)). Stop the process listening on port $port before running the frontend release smoke harness."
        }
        Write-Host "PORT $port free for $($surface.Name)"
    }

    foreach ($surface in $surfaces) {
        $surfacePath = Join-Path $repoRoot $surface.Path
        $port = [string]$surface.Port
        $url = "http://127.0.0.1:$port/"
        Write-Host "START $($surface.Name) preview $url"
        if ($surface.Framework -eq "next") {
            $process = Start-Process `
                -FilePath "npm.cmd" `
                -ArgumentList @("exec", "--", "next", "start", "--hostname", "127.0.0.1", "--port", $port) `
                -WorkingDirectory $surfacePath `
                -PassThru `
                -WindowStyle Hidden
        }
        else {
            $process = Start-Process `
                -FilePath "npm.cmd" `
                -ArgumentList @("exec", "--", "vite", "preview", "--host", "127.0.0.1", "--port", $port, "--strictPort") `
                -WorkingDirectory $surfacePath `
                -PassThru `
                -WindowStyle Hidden
        }
        $processes += $process
        Wait-ForHttpOk -Uri $url -TimeoutSeconds $StartupTimeoutSeconds -ExpectedContent $surface.ExpectedContent
        Write-Host "PASS $($surface.Name) preview $url"
    }

    Write-Host "release frontend smoke passed for official-site, master-ui, worker-ui"
}
finally {
    foreach ($process in $processes) {
        if ($null -ne $process -and !$process.HasExited) {
            Stop-PreviewProcessTree -Process $process
            Write-Host "CLEANUP stopped preview process $($process.Id)"
        }
    }
}
