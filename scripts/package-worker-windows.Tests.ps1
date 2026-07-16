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

Assert-Contains `
    -Haystack $scriptText `
    -Needle 'function Assert-NativeCommandSuccess' `
    -Message "standalone worker packager must define a native-command failure guard."

Assert-Contains `
    -Haystack $scriptText `
    -Needle 'Assert-NativeCommandSuccess -Command "cargo build worker binary"' `
    -Message "standalone worker packager must stop after a failed Cargo build."

Assert-Contains `
    -Haystack $scriptText `
    -Needle 'WORKER_ID=' `
    -Message "standalone worker package must require deployment-time worker identity."

Assert-NotContains `
    -Haystack $scriptText `
    -Needle 'WORKER_ID=$env:COMPUTERNAME' `
    -Message "standalone worker package must not bake the build host identity."

$composePath = Join-Path $PSScriptRoot "..\docker-compose.yml"
$composeText = Get-Content -LiteralPath $composePath -Raw
$workerBlockMatch = [regex]::Match($composeText, '(?ms)^  worker:\r?\n.*?(?=^  [a-z-]+:|\z)')
if (!$workerBlockMatch.Success) {
    throw "docker-compose.yml must contain a worker service block."
}
$workerBlock = $workerBlockMatch.Value
$nodepoolBlockMatch = [regex]::Match($composeText, '(?ms)^  nodepool:\r?\n.*?(?=^  [a-z-]+:|\z)')
if (!$nodepoolBlockMatch.Success) {
    throw "docker-compose.yml must contain a nodepool service block."
}
$nodepoolBlock = $nodepoolBlockMatch.Value
$allInOneBlockMatch = [regex]::Match($composeText, '(?ms)^  hivemind:\r?\n.*?(?=^  [a-z-]+:|\z)')
if (!$allInOneBlockMatch.Success) {
    throw "docker-compose.yml must contain the all-in-one service block."
}
$allInOneBlock = $allInOneBlockMatch.Value

Assert-Contains `
    -Haystack $workerBlock `
    -Needle 'WORKER_EXECUTION_SECRET: ${WORKER_EXECUTION_SECRET:?WORKER_EXECUTION_SECRET must be set to a non-default value}' `
    -Message "canonical Compose worker must require the worker-execution secret."

Assert-NotContains `
    -Haystack $workerBlock `
    -Needle 'JWT_SECRET:' `
    -Message "canonical Compose worker must not receive the control-plane JWT_SECRET."

Assert-Contains `
    -Haystack $nodepoolBlock `
    -Needle 'WORKER_EXECUTION_SECRET: ${WORKER_EXECUTION_SECRET:?WORKER_EXECUTION_SECRET must be set to a non-default value}' `
    -Message "canonical Compose nodepool must require the worker-execution secret."

Assert-Contains `
    -Haystack $allInOneBlock `
    -Needle 'WORKER_EXECUTION_SECRET: ${WORKER_EXECUTION_SECRET:?WORKER_EXECUTION_SECRET must be set to a non-default value}' `
    -Message "canonical Compose all-in-one service must require the worker-execution secret."

Assert-Contains `
    -Haystack $workerBlock `
    -Needle '- "${WORKER_GRPC_BIND_HOST:-127.0.0.1}:50053:50053"' `
    -Message "canonical Compose worker gRPC host exposure must default to loopback."

Assert-Contains `
    -Haystack $workerBlock `
    -Needle 'WORKER_ADVERTISE_ADDR: ${WORKER_ADVERTISE_ADDR:-worker:50053}' `
    -Message "canonical Compose worker must preserve an explicit multi-host advertise endpoint override."

Assert-NotContains `
    -Haystack $workerBlock `
    -Needle '- "50053:50053"' `
    -Message "canonical Compose worker must not publish gRPC on every host interface by default."

Assert-Contains `
    -Haystack $allInOneBlock `
    -Needle '- "${WORKER_GRPC_BIND_HOST:-127.0.0.1}:50053:50053"' `
    -Message "canonical Compose all-in-one worker gRPC host exposure must default to loopback."

Assert-Contains `
    -Haystack $allInOneBlock `
    -Needle 'WORKER_ADVERTISE_ADDR: ${WORKER_ADVERTISE_ADDR:-hivemind:50053}' `
    -Message "canonical Compose all-in-one worker must preserve an explicit advertise endpoint override."

Assert-NotContains `
    -Haystack $allInOneBlock `
    -Needle '- "50053:50053"' `
    -Message "canonical Compose all-in-one worker must not publish gRPC on every host interface by default."

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
    -Needle 'New-ItemProperty -LiteralPath $key -Name "WindowAlpha" -Value 255 -PropertyType DWord -Force' `
    -Message "start-worker launcher must explicitly persist fully opaque cmd.exe console alpha."

Assert-Contains `
    -Haystack $scriptText `
    -Needle "WindowTransparency" `
    -Message "start-worker launcher must remove persisted Console WindowTransparency values."

Assert-Contains `
    -Haystack $scriptText `
    -Needle "function Reset-CurrentConsoleOpacity" `
    -Message "start-worker launcher must reset the already-created console window, not only persisted registry values."

Assert-Contains `
    -Haystack $scriptText `
    -Needle "GetConsoleWindow" `
    -Message "start-worker launcher must locate the current console window handle."

Assert-Contains `
    -Haystack $scriptText `
    -Needle "SetLayeredWindowAttributes(`$consoleWindow, 0, 255, 0x2)" `
    -Message "start-worker launcher must force the current console window alpha to fully opaque."

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
    -Needle "function Reset-WindowsTerminalCmdOpacity" `
    -Message "start-worker launcher must reset Windows Terminal cmd.exe profile opacity, not only legacy conhost settings."

Assert-Contains `
    -Haystack $scriptText `
    -Needle "Microsoft.WindowsTerminal_8wekyb3d8bbwe" `
    -Message "start-worker launcher must inspect packaged Windows Terminal settings."

Assert-Contains `
    -Haystack $scriptText `
    -Needle "useAcrylic" `
    -Message "start-worker launcher must disable Windows Terminal acrylic transparency."

Assert-Contains `
    -Haystack $scriptText `
    -Needle "useAcrylicInTabRow" `
    -Message "start-worker launcher must disable Windows Terminal tab row acrylic transparency."

Assert-Contains `
    -Haystack $scriptText `
    -Needle "opacity" `
    -Message "start-worker launcher must force Windows Terminal profile opacity to 100."

$importCall = $scriptText.IndexOf("Import-DotEnv -Path `$envFile")
$resetCall = if ($importCall -ge 0) { $scriptText.LastIndexOf("Reset-WindowsTerminalCmdOpacity", $importCall) } else { -1 }
if ($resetCall -lt 0 -or $importCall -lt 0) {
    throw "start-worker launcher must reset console opacity before .env import or validation can abort startup."
}

Assert-NotContains `
    -Haystack $scriptText `
    -Needle 'function Ensure-JwtSecret' `
    -Message "start-worker launcher must not auto-generate a JWT secret that cannot match nodepool."

Assert-NotContains `
    -Haystack $scriptText `
    -Needle 'function New-RandomJwtSecret' `
    -Message "start-worker launcher must not contain a local JWT secret generator."

Assert-Contains `
    -Haystack $scriptText `
    -Needle 'Assert-RequiredEnv -Names @("NODEPOOL_GRPC_ADDR", "WORKER_GRPC_ADDR", "WORKER_CONTROL_HTTP_ADDR", "WORKER_ADVERTISE_ADDR", "WORKER_ID", "WORKER_EXECUTION_SECRET")' `
    -Message "start-worker launcher must require explicit nodepool, advertise endpoint, control API, and worker-execution settings."

Assert-Contains `
    -Haystack $scriptText `
    -Needle 'Set WORKER_NODEPOOL_TOKEN, or set both WORKER_NODEPOOL_USERNAME and WORKER_NODEPOOL_PASSWORD.' `
    -Message "start-worker launcher must allow token or username/password nodepool authentication."

Assert-Contains `
    -Haystack $scriptText `
    -Needle 'WORKER_CONTROL_HTTP_ADDR=$WorkerControlHttpAddr' `
    -Message "worker package template must default the control HTTP address so the Worker UI starts with the worker."

Assert-NotContains `
    -Haystack $scriptText `
    -Needle 'Ensure-JwtSecret -Path $envFile' `
    -Message "start-worker launcher must not initialize JWT_SECRET locally."

Assert-Contains `
    -Haystack $scriptText `
    -Needle 'WORKER_EXECUTION_SECRET=' `
    -Message "worker package template must provide an explicit worker-execution secret setting."

Assert-NotContains `
    -Haystack $scriptText `
    -Needle 'JWT_SECRET=' `
    -Message "worker package must not request the control-plane JWT_SECRET."

Assert-Contains `
    -Haystack $scriptText `
    -Needle 'same non-default worker-execution secret used by the nodepool' `
    -Message "worker package README must document the nodepool/worker trust secret."

Assert-Contains `
    -Haystack $scriptText `
    -Needle 'TORRENT_TASK_ARTIFACT_BASE_URL=' `
    -Message "worker package template must expose the remote task artifact base URL setting."

Write-Host "package-worker-windows launcher tests passed"
