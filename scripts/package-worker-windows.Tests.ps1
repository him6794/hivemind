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

Assert-Contains `
    -Haystack $scriptText `
    -Needle 'TORRENT_TASK_ARTIFACT_BASE_URL=' `
    -Message "worker package template must expose the remote task artifact base URL setting."

Write-Host "package-worker-windows launcher tests passed"
