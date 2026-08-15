[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$WorkerExecutable,
    [Parameter(Mandatory = $true)][string]$BackendRegistry,
    [Parameter(Mandatory = $true)][string]$EvidenceDirectory
)

$ErrorActionPreference = "Stop"

function Fail-Prerequisite {
    param([Parameter(Mandatory = $true)][string]$Message)

    Write-Error "Windows HCS E2E blocked: $Message" -ErrorAction Continue
    exit 2
}

if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) {
    Fail-Prerequisite "the harness must run on a native Windows host"
}

$containers = Get-WindowsOptionalFeature -Online -FeatureName Containers -ErrorAction SilentlyContinue
if ($null -eq $containers -or $containers.State -ne "Enabled") {
    $state = if ($null -eq $containers) { "unavailable" } else { $containers.State }
    Fail-Prerequisite "Windows Containers optional feature is not enabled (state: $state)"
}

$vmcompute = Get-Service -Name vmcompute -ErrorAction SilentlyContinue
if ($null -eq $vmcompute -or $vmcompute.Status -ne "Running") {
    $state = if ($null -eq $vmcompute) { "missing" } else { $vmcompute.Status }
    Fail-Prerequisite "vmcompute is not running (state: $state)"
}

if (!(Test-Path -LiteralPath $WorkerExecutable -PathType Leaf)) {
    Fail-Prerequisite "operator-provided Worker executable is missing"
}
if (!(Test-Path -LiteralPath $BackendRegistry -PathType Leaf)) {
    Fail-Prerequisite "operator-provided Windows backend registry is missing"
}

try {
    $registry = Get-Content -LiteralPath $BackendRegistry -Raw | ConvertFrom-Json
} catch {
    Fail-Prerequisite "operator-provided Windows backend registry is not valid JSON"
}
if ($null -eq $registry -or @($registry).Count -eq 0) {
    Fail-Prerequisite "operator-provided Windows backend registry is empty"
}

New-Item -ItemType Directory -Force -Path $EvidenceDirectory | Out-Null
$evidence = [ordered]@{
    schema = "hivemind.windows-hcs-e2e.v1"
    platform = "windows"
    provider = "hcs-windows-containers"
    worker_executable = (Resolve-Path -LiteralPath $WorkerExecutable).Path
    backend_registry = (Resolve-Path -LiteralPath $BackendRegistry).Path
    containers_feature = $containers.State
    vmcompute = $vmcompute.Status.ToString()
    status = "prerequisites_ready"
}
$evidence | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $EvidenceDirectory "prerequisites.json") -Encoding UTF8
Write-Host "Windows HCS E2E prerequisites passed; execute only the native HCS harness next."
