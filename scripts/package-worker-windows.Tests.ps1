$ErrorActionPreference = "Stop"

$scriptPath = Join-Path $PSScriptRoot "package-worker-windows.ps1"
$scriptText = Get-Content -LiteralPath $scriptPath -Raw

if ($scriptText -match "(?i)monty") {
    throw "Windows worker packaging must not retain the removed Monty runtime contract."
}

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
    -Needle 'Assert-RequiredEnv -Names @("WORKER_GRPC_ADDR", "WORKER_CONTROL_HTTP_ADDR")' `
    -Message "start-worker launcher must require worker listen/control addresses without requiring a pre-provisioned token."

# Zero-config onboarding: neither the launcher nor the template may require a
# Nodepool endpoint, a static Worker ID, or a reusable nodepool token.
if ($scriptText -match 'Required setting NODEPOOL_GRPC_ENDPOINT') {
    throw "start-worker launcher must not require NODEPOOL_GRPC_ENDPOINT/NODEPOOL_GRPC_ADDR; public onboarding discovers the transport after login."
}

foreach ($forbiddenRequirement in @(
        'Assert-RequiredEnv[^\r\n]*WORKER_ID',
        'Assert-RequiredEnv[^\r\n]*NODEPOOL',
        'Assert-RequiredEnv[^\r\n]*WORKER_ADVERTISE_ADDR'
    )) {
    if ($scriptText -match $forbiddenRequirement) {
        throw "worker launcher must not require a static identity/endpoint setting matched by '$forbiddenRequirement'."
    }
}

Assert-Contains `
    -Haystack $scriptText `
    -Needle 'NODEPOOL_GRPC_ADDR and NODEPOOL_GRPC_ENDPOINT are' `
    -Message "worker package template must mark both Nodepool endpoint settings as optional compatibility settings."

Assert-Contains `
    -Haystack $scriptText `
    -Needle 'Ensure-JwtSecret -Path $envFile' `
    -Message "start-worker launcher must call the JWT secret initializer."

Assert-Contains `
    -Haystack $scriptText `
    -Needle 'TORRENT_TASK_ARTIFACT_BASE_URL=' `
    -Message "worker package template must expose the remote task artifact base URL setting."

Assert-Contains `
    -Haystack $scriptText `
    -Needle 'HIVEMIND_GENERAL_COMPUTE_WINDOWS_BACKENDS=' `
    -Message "worker package template must expose the native Windows HCS registry setting."

# The package README is Markdown, and it must be built from a literal
# here-string: an interpolating one silently eats every backtick as an escape
# character, so the code spans and fenced blocks reach the package intact.
# Extracting it here also lets the assertions below run against the rendered
# text, which is what an operator actually reads, rather than the script source.
$readmeMatch = [regex]::Match($scriptText, "(?s)\`$readme = @'\r?\n(.*?)\r?\n'@")
if (!$readmeMatch.Success) {
    throw "windows worker package README must come from a literal here-string, or PowerShell strips its Markdown backticks."
}
$packagedReadme = $readmeMatch.Groups[1].Value

Assert-Contains `
    -Haystack $packagedReadme `
    -Needle '`.env.worker.example`' `
    -Message "packaged README must keep its Markdown inline code spans intact."

# The README is written with -Encoding ASCII, which would turn anything else
# into a literal '?' in the shipped package.
if ([regex]::IsMatch($packagedReadme, '[^\x00-\x7F]')) {
    throw "packaged README must stay ASCII-only because it is written with -Encoding ASCII."
}

# The generated env template must remain complete and must not contain server secrets.
$envMatch = [regex]::Match($scriptText, '(?s)\$envTemplate = @"\r?\n(.*?)\r?\n"@')
if (!$envMatch.Success) {
    throw "worker package must build .env.worker.example from a here-string."
}
$packagedEnv = $envMatch.Groups[1].Value
if ([regex]::IsMatch($packagedEnv, '(?m)^\s*HEADSCALE_API_KEY\s*=')) {
    throw "worker package must never distribute the server-side HEADSCALE_API_KEY."
}
foreach ($expected in @(
        "NODEPOOL_GRPC_ENDPOINT", "WEBSITE_API_BASE", "HEADSCALE_LOGIN_SERVER",
        "WORKER_VPN_AUTHKEY", "WORKER_VPN_HOSTNAME", "VPN_STARTUP_TIMEOUT_SECS",
        "TORRENT_TASK_ARTIFACT_BASE_URL", "HIVEMIND_GENERAL_COMPUTE_WINDOWS_BACKENDS"
    )) {
    Assert-Contains -Haystack $packagedEnv -Needle $expected `
        -Message "worker package template must expose '$expected'."
}
foreach ($forbiddenRequirement in @(
        'Assert-RequiredEnv[^\r\n]*WORKER_VPN_AUTHKEY',
        'Assert-RequiredEnv[^\r\n]*WORKER_NODEPOOL_TOKEN',
        'Assert-RequiredEnv[^\r\n]*WORKER_ID',
        'Assert-RequiredEnv[^\r\n]*NODEPOOL'
    )) {
    if ($scriptText -match $forbiddenRequirement) {
        throw "worker launcher must not require a static setting matched by '$forbiddenRequirement'."
    }
}

Write-Host "package-worker-windows launcher tests passed"
