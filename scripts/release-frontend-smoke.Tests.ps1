$ErrorActionPreference = "Stop"

$scriptPath = Join-Path $PSScriptRoot "release-frontend-smoke.ps1"
if (!(Test-Path -LiteralPath $scriptPath)) {
    throw "release-frontend-smoke.ps1 must exist."
}

$scriptText = Get-Content -LiteralPath $scriptPath -Raw
foreach ($expected in @("official-site", "master-ui", "worker-ui")) {
    if (!$scriptText.Contains($expected)) {
        throw "release frontend smoke harness must include $expected."
    }
}

if (!$scriptText.Contains("build-release-frontends.ps1")) {
    throw "release frontend smoke harness must delegate shared builds to build-release-frontends.ps1."
}

if (!$scriptText.Contains("Test-PortOccupied")) {
    throw "release frontend smoke harness must include Test-PortOccupied port pre-check function."
}

$previousApiBase = $env:VITE_API_BASE
$previousWorkerBase = $env:VITE_WORKER_CONTROL_BASE
$previousWebsiteNodepool = $env:WEBSITE_NODEPOOL_GRPC_ADDR
$previousErrorActionPreference = $ErrorActionPreference
try {
    $ErrorActionPreference = "Continue"
    $env:VITE_API_BASE = "http://127.0.0.1:8082"
    Remove-Item Env:\WEBSITE_NODEPOOL_GRPC_ADDR -ErrorAction SilentlyContinue
    Remove-Item Env:\VITE_WORKER_CONTROL_BASE -ErrorAction SilentlyContinue

    $output = & powershell -NoProfile -ExecutionPolicy Bypass -File $scriptPath -SkipBuild -CheckOnly 2>&1
    $exitCode = $LASTEXITCODE
    if ($exitCode -eq 0) {
        throw "release frontend smoke harness must fail when WEBSITE_NODEPOOL_GRPC_ADDR is missing."
    }

    $joinedOutput = ($output | Out-String)
    if (!$joinedOutput.Contains("WEBSITE_NODEPOOL_GRPC_ADDR")) {
        throw "missing official site env failure must name WEBSITE_NODEPOOL_GRPC_ADDR."
    }
    if (!$joinedOutput.Contains("Set WEBSITE_NODEPOOL_GRPC_ADDR")) {
        throw "missing official site env failure must include an actionable fix."
    }

    $env:WEBSITE_NODEPOOL_GRPC_ADDR = "127.0.0.1:50051"
    $missingWorkerOutput = & powershell -NoProfile -ExecutionPolicy Bypass -File $scriptPath -SkipBuild -CheckOnly 2>&1
    $missingWorkerExitCode = $LASTEXITCODE
    if ($missingWorkerExitCode -eq 0) {
        throw "release frontend smoke harness must fail when VITE_WORKER_CONTROL_BASE is missing."
    }

    $joinedOutput = ($missingWorkerOutput | Out-String)
    if (!$joinedOutput.Contains("VITE_WORKER_CONTROL_BASE")) {
        throw "missing worker control env failure must name VITE_WORKER_CONTROL_BASE."
    }
    if (!$joinedOutput.Contains("Set VITE_WORKER_CONTROL_BASE")) {
        throw "missing worker control env failure must include an actionable fix."
    }

    $env:VITE_WORKER_CONTROL_BASE = "http://127.0.0.1:8083"
    $badTimeoutOutput = & powershell -NoProfile -ExecutionPolicy Bypass -File $scriptPath -SkipBuild -CheckOnly -StartupTimeoutSeconds 0 2>&1
    $badTimeoutExitCode = $LASTEXITCODE
    if ($badTimeoutExitCode -eq 0) {
        throw "release frontend smoke harness must reject StartupTimeoutSeconds values below 1."
    }

    $badTimeoutText = ($badTimeoutOutput | Out-String)
    if (!$badTimeoutText.Contains("StartupTimeout") -and !$badTimeoutText.Contains("ParameterArgumentValidationError")) {
        throw "invalid timeout failure must identify the StartupTimeoutSeconds parameter validation."
    }
}
finally {
    $ErrorActionPreference = $previousErrorActionPreference

    if ($null -eq $previousApiBase) {
        Remove-Item Env:\VITE_API_BASE -ErrorAction SilentlyContinue
    }
    else {
        $env:VITE_API_BASE = $previousApiBase
    }

    if ($null -eq $previousWorkerBase) {
        Remove-Item Env:\VITE_WORKER_CONTROL_BASE -ErrorAction SilentlyContinue
    }
    else {
        $env:VITE_WORKER_CONTROL_BASE = $previousWorkerBase
    }

    if ($null -eq $previousWebsiteNodepool) {
        Remove-Item Env:\WEBSITE_NODEPOOL_GRPC_ADDR -ErrorAction SilentlyContinue
    }
    else {
        $env:WEBSITE_NODEPOOL_GRPC_ADDR = $previousWebsiteNodepool
    }
}

# Occupied-port regression test
Write-Host "--- Occupied-port regression test ---"
$buildScriptPath = Join-Path $PSScriptRoot "build-release-frontends.ps1"

# Ensure dist artifacts exist for the smoke harness artifact checks
Write-Host "Building frontend artifacts for smoke harness prerequisite..."
& powershell -NoProfile -ExecutionPolicy Bypass -File $buildScriptPath
if ($LASTEXITCODE -ne 0) {
    throw "Prerequisite build failed before occupied-port test."
}

# Determine an available test port for the controlled blocker.
$testPort = 4180
$freeCheck = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, $testPort)
try {
    $freeCheck.Start()
    $freeCheck.Stop()
    Write-Host "Test port $testPort is free."
}
catch {
    $freeCheck2 = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 4181)
    try {
        $freeCheck2.Start()
        $freeCheck2.Stop()
        $testPort = 4181
        Write-Host "Test port $testPort (fallback) is free."
    }
    catch {
        throw "Cannot find a free test port for the occupied-port regression test."
    }
}

# Phase 1: Controlled port-blocker test.
$blocker = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, $testPort)
$blocker.Start()
Write-Host "Placed temporary listener on port $testPort"

$phase1Passed = $false
try {
    $second = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, $testPort)
    try {
        $second.Start()
        throw "TEST INFRA FAILURE: port $testPort was not actually occupied by the blocker"
    }
    catch [System.Net.Sockets.SocketException] {
        Write-Host "Confirmed port $testPort is occupied (second listener rejected)."
        $phase1Passed = $true
    }
    finally {
        $second.Stop()
    }
}
finally {
    $blocker.Stop()
    Write-Host "Stopped temporary listener on port $testPort"
}

if (!$phase1Passed) {
    throw "Phase 1 occupied-port detection mechanism validation failed."
}

Write-Host "Phase 1 PASS: controlled port blocker correctly detected."

# Phase 2: Run the smoke harness with one actual target port occupied.
$previousApiBase2 = $env:VITE_API_BASE
$previousWorkerBase2 = $env:VITE_WORKER_CONTROL_BASE
$previousWebsiteNodepool2 = $env:WEBSITE_NODEPOOL_GRPC_ADDR
$previousErrorActionPreference2 = $ErrorActionPreference
$targetBlocker = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 4173)
$targetBlockerStarted = $false
try {
    $ErrorActionPreference = "Continue"
    $env:VITE_API_BASE = "http://127.0.0.1:8082"
    $env:VITE_WORKER_CONTROL_BASE = "http://127.0.0.1:8083"
    $env:WEBSITE_NODEPOOL_GRPC_ADDR = "127.0.0.1:50051"
    try {
        $targetBlocker.Start()
        $targetBlockerStarted = $true
        Write-Host "Placed temporary listener on frontend smoke target port 4173"
    }
    catch [System.Net.Sockets.SocketException] {
        Write-Host "Frontend smoke target port 4173 is already occupied; using existing listener for regression test."
    }

    $output = & powershell -NoProfile -ExecutionPolicy Bypass -File $scriptPath -SkipBuild -StartupTimeoutSeconds 5 2>&1
    $exitCode = $LASTEXITCODE

    if ($exitCode -eq 0) {
        throw "Smoke harness must fail when ports are occupied before preview launch."
    }

    $joinedOutput = ($output | Out-String)
    if (!$joinedOutput.Contains("4173")) {
        throw "Occupied-port failure must identify the occupied official-site port. Got: $joinedOutput"
    }
    if (!$joinedOutput.Contains("occupied")) {
        throw "Occupied-port failure must use the word 'occupied'. Got: $joinedOutput"
    }

    Write-Host "Occupied-port regression: PASS (harness correctly rejected stale occupied ports)"
}
finally {
    if ($targetBlockerStarted) {
        $targetBlocker.Stop()
        Write-Host "Stopped temporary listener on frontend smoke target port 4173"
    }
    $ErrorActionPreference = $previousErrorActionPreference2
    if ($null -eq $previousApiBase2) { Remove-Item Env:\VITE_API_BASE -ErrorAction SilentlyContinue } else { $env:VITE_API_BASE = $previousApiBase2 }
    if ($null -eq $previousWorkerBase2) { Remove-Item Env:\VITE_WORKER_CONTROL_BASE -ErrorAction SilentlyContinue } else { $env:VITE_WORKER_CONTROL_BASE = $previousWorkerBase2 }
    if ($null -eq $previousWebsiteNodepool2) { Remove-Item Env:\WEBSITE_NODEPOOL_GRPC_ADDR -ErrorAction SilentlyContinue } else { $env:WEBSITE_NODEPOOL_GRPC_ADDR = $previousWebsiteNodepool2 }
}

Write-Host "release frontend smoke tests passed"
