[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$WorkerExecutable,
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
    Fail-Prerequisite "packaged Worker executable is missing"
}
$workerItem = Get-Item -LiteralPath $WorkerExecutable -Force
if (!($workerItem -is [IO.FileInfo]) -or
    (($workerItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0)) {
    Fail-Prerequisite "Worker executable must be a regular, non-reparse file"
}
$packageRoot = $workerItem.DirectoryName
$bundleRoot = Join-Path $packageRoot "windows-hcs-runtime"
if (!(Test-Path -LiteralPath $bundleRoot -PathType Container)) {
    Fail-Prerequisite "signed package-relative Windows HCS runtime bundle is missing"
}
$bundleItem = Get-Item -LiteralPath $bundleRoot -Force
if (!($bundleItem -is [IO.DirectoryInfo]) -or
    (($bundleItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0)) {
    Fail-Prerequisite "Windows HCS runtime bundle must be a regular, non-reparse directory"
}
$bundleManifest = Join-Path $bundleRoot "bundle-manifest.json"
if (!(Test-Path -LiteralPath $bundleManifest -PathType Leaf)) {
    Fail-Prerequisite "signed Windows HCS runtime bundle manifest is missing"
}
$bundleManifestItem = Get-Item -LiteralPath $bundleManifest -Force
if (!($bundleManifestItem -is [IO.FileInfo]) -or
    (($bundleManifestItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0)) {
    Fail-Prerequisite "Windows HCS runtime bundle manifest must be a regular, non-reparse file"
}
try {
    $signedManifest = Get-Content -LiteralPath $bundleManifest -Raw | ConvertFrom-Json
} catch {
    Fail-Prerequisite "signed Windows HCS runtime bundle manifest is not valid JSON"
}
if ($null -eq $signedManifest.manifest -or
    [string]::IsNullOrWhiteSpace([string]$signedManifest.signature) -or
    $null -eq $signedManifest.manifest.backends -or
    @($signedManifest.manifest.backends).Count -eq 0) {
    Fail-Prerequisite "signed Windows HCS runtime bundle manifest is incomplete"
}
foreach ($bundleEntry in @(Get-ChildItem -LiteralPath $bundleRoot -Force -Recurse)) {
    if (($bundleEntry.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
        Fail-Prerequisite "Windows HCS runtime bundle contains a reparse point"
    }
    if (!$bundleEntry.PSIsContainer -and !($bundleEntry -is [IO.FileInfo])) {
        Fail-Prerequisite "Windows HCS runtime bundle contains an unsupported filesystem entry"
    }
}

New-Item -ItemType Directory -Force -Path $EvidenceDirectory | Out-Null
$evidence = [ordered]@{
    schema = "hivemind.windows-hcs-e2e.v1"
    platform = "windows"
    provider = "hcs-windows-containers"
    worker_executable = $workerItem.FullName
    hcs_runtime_bundle = $bundleItem.FullName
    bundle_manifest = $bundleManifestItem.FullName
    containers_feature = $containers.State
    vmcompute = $vmcompute.Status.ToString()
    status = "prerequisites_ready"
}
$evidence | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $EvidenceDirectory "prerequisites.json") -Encoding UTF8
Write-Host "Windows HCS E2E prerequisites passed; execute only the native HCS harness next."
