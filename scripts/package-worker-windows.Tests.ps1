$ErrorActionPreference = "Stop"

$scriptPath = Join-Path $PSScriptRoot "package-worker-windows.ps1"
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

Assert-Contains `
    -Haystack $scriptText `
    -Needle "function Reset-CmdConsoleOpacity" `
    -Message "start-worker launcher must reset cmd.exe console opacity before starting services."

Assert-Contains `
    -Haystack $scriptText `
    -Needle "WindowAlpha" `
    -Message "start-worker launcher must remove persisted Console WindowAlpha values."

Assert-Contains `
    -Haystack $scriptText `
    -Needle "WindowTransparency" `
    -Message "start-worker launcher must remove persisted Console WindowTransparency values."

Assert-Contains `
    -Haystack $scriptText `
    -Needle 'Get-ChildItem -LiteralPath $consoleRoot -Recurse' `
    -Message "start-worker launcher must inspect all Console subkeys, including path-encoded cmd.exe keys."

Assert-Contains `
    -Haystack $scriptText `
    -Needle 'Reset-CmdConsoleOpacity' `
    -Message "start-worker launcher must call the opacity reset function."

Assert-Contains `
    -Haystack $scriptText `
    -Needle 'function Ensure-JwtSecret' `
    -Message "start-worker launcher must auto-generate a JWT secret when it is blank."

Assert-Contains `
    -Haystack $scriptText `
    -Needle 'Assert-RequiredEnv -Names @("NODEPOOL_GRPC_ADDR", "WORKER_GRPC_ADDR", "WORKER_CONTROL_HTTP_ADDR", "WORKER_NODEPOOL_TOKEN")' `
    -Message "start-worker launcher must require the worker nodepool token but not require WORKER_ADVERTISE_ADDR or JWT_SECRET."

Assert-Contains `
    -Haystack $scriptText `
    -Needle 'Ensure-JwtSecret -Path $envFile' `
    -Message "start-worker launcher must call the JWT secret initializer."

Write-Host "package-worker-windows launcher tests passed"
