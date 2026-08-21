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

Assert-Contains `
    -Haystack $scriptText `
    -Needle 'NODEPOOL_GRPC_ENDPOINT or NODEPOOL_GRPC_ADDR' `
    -Message "start-worker launcher must accept either the new or compatibility Nodepool endpoint setting."

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
# character, so the code spans and fenced blocks reach the provider stripped.
# Extracting it here also lets the assertions below run against the rendered
# text, which is what a provider actually reads, rather than the script source.
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

# A provider who unpacks this on Windows must not discover the missing prover by
# watching every managed task fail. Say it in the package README instead.
$noteStart = $packagedReadme.IndexOf("## Managed proving")
if ($noteStart -lt 0) {
    throw "windows worker package README must carry a managed-proving section."
}
$note = $packagedReadme.Substring($noteStart)

# Match on prose, not on where the template happens to wrap: re-flowing a
# paragraph must not turn a documented guarantee into a red test.
$flowedNote = [regex]::Replace($note, '\s+', ' ')
$flowedReadme = [regex]::Replace($packagedReadme, '\s+', ' ')

Assert-Contains `
    -Haystack $flowedNote `
    -Needle "ordinary worker workloads" `
    -Message "windows worker package README must state that Windows workers run ordinary worker workloads."

Assert-Contains `
    -Haystack $flowedNote `
    -Needle "does not include a RISC Zero prover sidecar" `
    -Message "windows worker package README must state that no RISC Zero prover sidecar ships with it."

Assert-Contains `
    -Haystack $flowedNote `
    -Needle "Linux, macOS, and WSL" `
    -Message "windows worker package README must name the supported RISC Zero proving hosts."

Assert-Contains `
    -Haystack $flowedNote `
    -Needle "fails closed" `
    -Message "windows worker package README must state that managed proving fails closed here."

Assert-Contains `
    -Haystack $flowedNote `
    -Needle "supported Linux-based runtime" `
    -Message "windows worker package README must point managed proving at a supported Linux-based runtime."

Assert-Contains `
    -Haystack $flowedNote `
    -Needle "worker image or runtime that contains the Linux prover sidecar" `
    -Message "windows worker package README must say managed tasks need a runtime carrying the Linux prover sidecar."

Assert-Contains `
    -Haystack $flowedNote `
    -Needle "wsl bash scripts/build-managed-prover.sh" `
    -Message "windows worker package README must show how to build the sidecar from a Windows checkout via WSL."

foreach ($expected in @("RECURSION_SRC_PATH", "recursion_zkr.zip", "SHA-256")) {
    Assert-Contains `
        -Haystack $flowedNote `
        -Needle $expected `
        -Message "windows worker package README must document the offline recursion artifact escape hatch via '$expected'."
}

Assert-Contains `
    -Haystack $flowedNote `
    -Needle "official upstream offline escape hatch" `
    -Message "windows worker package README must use the canonical official upstream offline escape hatch wording."

# This package ships no prover sidecar, so the template must not hand the worker
# a prover path: managed proving has to fail closed on native Windows.
$envMatch = [regex]::Match($scriptText, '(?s)\$envTemplate = @"\r?\n(.*?)\r?\n"@')
if (!$envMatch.Success) {
    throw "windows worker package must build .env.worker.example from a here-string."
}
$packagedEnv = $envMatch.Groups[1].Value

if ([regex]::IsMatch($packagedEnv, '(?m)^\s*HEADSCALE_API_KEY\s*=')) {
    throw "worker package must never distribute the server-side HEADSCALE_API_KEY."
}

foreach ($expected in @(
        "NODEPOOL_GRPC_ENDPOINT",
        "WEBSITE_API_BASE",
        "HEADSCALE_LOGIN_SERVER",
        "WORKER_VPN_AUTHKEY",
        "WORKER_VPN_HOSTNAME",
        "VPN_STARTUP_TIMEOUT_SECS"
    )) {
    Assert-Contains `
        -Haystack $packagedEnv `
        -Needle $expected `
        -Message "worker package template must expose optional Headscale startup setting '$expected'."
}

if ($scriptText -match 'Assert-RequiredEnv[^\r\n]*WORKER_VPN_AUTHKEY') {
    throw "worker launcher must not require WORKER_VPN_AUTHKEY; UI-login/direct-endpoint mode remains supported."
}

if ($scriptText -match 'Assert-RequiredEnv[^\r\n]*WORKER_NODEPOOL_TOKEN') {
    throw "worker launcher must not require WORKER_NODEPOOL_TOKEN; UI registration remains supported."
}

Assert-Contains `
    -Haystack $scriptText `
    -Needle "NODEPOOL_GRPC_ENDPOINT or NODEPOOL_GRPC_ADDR" `
    -Message "worker launcher must accept either the new or compatibility Nodepool endpoint setting."

Assert-Contains `
    -Haystack $scriptText `
    -Needle '$workerExitCode' `
    -Message "worker launcher must propagate the native worker exit code."

Assert-Contains `
    -Haystack $scriptText `
    -Needle 'name = "libtailscale.dll"' `
    -Message "MSVC package manifest must include the shipped libtailscale DLL hash."

Assert-Contains `
    -Haystack $scriptText `
    -Needle 'name = "vcruntime140.dll"' `
    -Message "MSVC package must ship the matching Visual C++ runtime dependency."

Assert-Contains `
    -Haystack $scriptText `
    -Needle '$packageArtifacts | ForEach-Object' `
    -Message "package SHA256SUMS must cover every shipped artifact, including native DLLs."

if ($scriptText -match 'WORKER_ID=\$env:COMPUTERNAME') {
    throw "worker package must not bake the packaging host COMPUTERNAME into the target worker identity."
}

Assert-Contains `
    -Haystack $scriptText `
    -Needle 'WORKER_ID=' `
    -Message "worker package template must leave WORKER_ID runtime-selected on the target host."

Assert-Contains `
    -Haystack $flowedReadme `
    -Needle "WEBSITE_API_BASE" `
    -Message "packaged README must document the Website API base used for authenticated VPN enrollment."

Assert-Contains `
    -Haystack $flowedReadme `
    -Needle "/api/vpn/config" `
    -Message "packaged README must identify the protected Rust Website API VPN-config contract."

Assert-Contains `
    -Haystack $flowedReadme `
    -Needle "one-time Headscale key" `
    -Message "packaged README must explain that interactive enrollment consumes a one-time Headscale key locally."

Assert-Contains `
    -Haystack $flowedReadme `
    -Needle "persisted VPN state" `
    -Message "packaged README must document restart state rehydration."

Assert-Contains `
    -Haystack $flowedReadme `
    -Needle "HEADSCALE_API_KEY" `
    -Message "packaged README must state that the platform Headscale API key is not shipped."

Assert-Contains `
    -Haystack $flowedReadme `
    -Needle "Orange Pi" `
    -Message "packaged README must keep Master and Worker off the Orange Pi platform host."

if ([regex]::IsMatch($packagedEnv, '(?m)^\s*MANAGED_PROVER_EXECUTABLE\s*=\s*\S')) {
    throw "windows worker template must not point MANAGED_PROVER_EXECUTABLE at a native Windows path; managed proving must fail closed."
}

Assert-Contains `
    -Haystack $packagedEnv `
    -Needle "MANAGED_PROVER_EXECUTABLE" `
    -Message "windows worker template must explain why no managed prover is configured on native Windows."

Write-Host "package-worker-windows launcher tests passed"
