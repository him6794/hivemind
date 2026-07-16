$ErrorActionPreference = "Stop"

$scriptPath = Join-Path $PSScriptRoot "package-role-clients.ps1"
$scriptText = Get-Content -LiteralPath $scriptPath -Raw

function Assert-Contains {
    param(
        [Parameter(Mandatory = $true)][string]$Haystack,
        [Parameter(Mandatory = $true)][string]$Needle,
        [Parameter(Mandatory = $true)][string]$Message
    )
    if (!$Haystack.Contains($Needle)) {
        throw $Message
    }
}

function Assert-NotContains {
    param(
        [Parameter(Mandatory = $true)][string]$Haystack,
        [Parameter(Mandatory = $true)][string]$Needle,
        [Parameter(Mandatory = $true)][string]$Message
    )
    if ($Haystack.Contains($Needle)) {
        throw $Message
    }
}

Assert-Contains -Haystack $scriptText -Needle 'cargo build --release --no-default-features --features $Features --bin $Bin' -Message 'role packaging must build feature-gated role binaries'
Assert-Contains -Haystack $scriptText -Needle 'Build-RoleBinary -Bin "hivemind-worker" -Features "worker"' -Message 'worker package must build hivemind-worker with worker feature'
Assert-Contains -Haystack $scriptText -Needle 'Build-RoleBinary -Bin "hivemind-master" -Features "master"' -Message 'master package must build hivemind-master with master feature'
Assert-Contains -Haystack $scriptText -Needle 'Build-RoleBinary -Bin "hivemind-nodepool" -Features "nodepool"' -Message 'nodepool package must build hivemind-nodepool with nodepool feature'
Assert-Contains -Haystack $scriptText -Needle 'frontend\worker-ui' -Message 'worker package must include worker UI assets'
Assert-Contains -Haystack $scriptText -Needle 'frontend\master-ui' -Message 'master package must include master UI assets'
Assert-Contains -Haystack $scriptText -Needle 'WORKER_UI_DIR' -Message 'worker launcher must set WORKER_UI_DIR so UI starts with the worker process'
Assert-Contains -Haystack $scriptText -Needle 'MASTER_UI_DIR' -Message 'master launcher must set MASTER_UI_DIR so UI starts with the master process'
Assert-Contains -Haystack $scriptText -Needle 'WORKER_EXECUTION_SECRET' -Message 'worker package must require worker-execution secret'
Assert-Contains -Haystack $scriptText -Needle 'NODEPOOL_GRPC_ENDPOINT' -Message 'master/worker packages must use NODEPOOL_GRPC_ENDPOINT for multi-host nodepool reachability'
Assert-Contains -Haystack $scriptText -Needle 'WORKER_ADVERTISE_ADDR' -Message 'worker package must require advertise address for multi-host deployments'
Assert-Contains -Haystack $scriptText -Needle 'WORKER_NODEPOOL_USERNAME' -Message 'worker package must support nodepool username/password login'
Assert-Contains -Haystack $scriptText -Needle 'Set WORKER_NODEPOOL_TOKEN, or set both WORKER_NODEPOOL_USERNAME and WORKER_NODEPOOL_PASSWORD.' -Message 'worker launcher must allow token or username/password authentication'
Assert-NotContains -Haystack $scriptText -Needle 'cargo build --release --bin hivemind-bin' -Message 'role packaging must not fall back to the all-features hivemind-bin binary'

$nativeGuardCount = ([regex]::Matches($scriptText, 'Assert-NativeCommandSuccess -Command')).Count
if ($nativeGuardCount -lt 5) {
    throw 'cargo and every npm build/install command must check the native exit code before packaging continues'
}

$workerEnvStart = $scriptText.IndexOf('# HiveMind Windows worker client')
$workerEnvEnd = $scriptText.IndexOf('Set-Content -Encoding ASCII (Join-Path $packageDir ".env.worker.example")')
if ($workerEnvStart -lt 0 -or $workerEnvEnd -lt 0 -or $workerEnvEnd -le $workerEnvStart) {
    throw 'could not isolate worker env template section'
}
$workerEnv = $scriptText.Substring($workerEnvStart, $workerEnvEnd - $workerEnvStart)
Assert-NotContains -Haystack $workerEnv -Needle 'JWT_SECRET=' -Message 'worker env template must not request JWT_SECRET='
Assert-Contains -Haystack $workerEnv -Needle 'WORKER_UI_DIR=.\ui\worker' -Message 'worker env template must default UI dir to packaged assets'
Assert-Contains -Haystack $workerEnv -Needle 'WORKER_ID=' -Message 'worker env template must require deployment-time worker identity'
Assert-NotContains -Haystack $workerEnv -Needle 'WORKER_ID=$env:COMPUTERNAME' -Message 'worker env template must not bake the packager computer name into the package'
Assert-Contains -Haystack $scriptText -Needle '"WORKER_ID",' -Message 'worker launcher must require an explicit worker identity'
Assert-Contains -Haystack $scriptText -Needle 'serves_ui_on_start = $true' -Message 'master/worker manifests must declare UI startup'
Assert-Contains -Haystack $scriptText -Needle 'serves_ui_on_start = $false' -Message 'nodepool manifest must declare no UI surface'
Assert-Contains -Haystack $scriptText -Needle 'monty.exe' -Message 'worker package must ship monty runtime'

$failureTestRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("hivemind-package-role-clients-failure-" + [guid]::NewGuid().ToString("N"))
$fakeBin = Join-Path $failureTestRoot "bin"
$outputRelative = "tmp\package-role-clients-failure-" + [guid]::NewGuid().ToString("N")
$outputRoot = Join-Path (Split-Path -Parent $PSScriptRoot) $outputRelative
New-Item -ItemType Directory -Force -Path $fakeBin | Out-Null
$oldPath = $env:PATH
try {
    @'
@echo off
exit /b 17
'@ | Set-Content -Encoding ASCII (Join-Path $fakeBin "cargo.cmd")

    $env:PATH = "$fakeBin;$oldPath"
    $powershell = (Get-Command powershell.exe -ErrorAction Stop).Source
    $oldErrorActionPreference = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    try {
        $failureOutput = (& $powershell -NoProfile -ExecutionPolicy Bypass -File $scriptPath -Role nodepool -SkipFrontendBuild -OutputDir $outputRelative 2>&1 | Out-String)
    }
    finally {
        $ErrorActionPreference = $oldErrorActionPreference
    }
    $failureExitCode = $LASTEXITCODE

    if ($failureExitCode -eq 0) {
        throw 'packaging must return a nonzero exit code when cargo fails'
    }
    if ($failureOutput -notmatch 'cargo build role binary') {
        throw "packaging did not report the cargo failure before checking for a stale artifact: $failureOutput"
    }
    if (Test-Path -LiteralPath (Join-Path $outputRoot 'nodepool-server')) {
        throw 'packaging must not create a successful role package after cargo fails'
    }
}
finally {
    $env:PATH = $oldPath
    if (Test-Path -LiteralPath $failureTestRoot) {
        Remove-Item -Recurse -Force -LiteralPath $failureTestRoot
    }
    if (Test-Path -LiteralPath $outputRoot) {
        Remove-Item -Recurse -Force -LiteralPath $outputRoot
    }
}

Write-Host "package-role-clients tests passed"
