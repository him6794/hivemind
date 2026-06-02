param(
    [string]$InstallDir = "C:\hivemind-worker"
)

$ErrorActionPreference = "Stop"
$binDir = Join-Path $InstallDir "bin"
$releaseDir = Join-Path $InstallDir "release"
$exeSource = Join-Path $releaseDir "worker-executor.exe"
$exeTarget = Join-Path $binDir "worker-executor.exe"
$versionFile = Join-Path $releaseDir "version.txt"

if (-not (Test-Path $exeSource)) {
    throw "Missing release artifact: $exeSource"
}

New-Item -ItemType Directory -Force -Path $binDir | Out-Null
Copy-Item -Force $exeSource $exeTarget

if (Test-Path $versionFile) {
    $version = (Get-Content $versionFile -Raw).Trim()
    Write-Host "Updated worker to version $version"
} else {
    Write-Host "Updated worker binary (version metadata missing)"
}
