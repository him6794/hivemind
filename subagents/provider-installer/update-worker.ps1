param(
    [string]$InstallDir = "C:\hivemind-worker"
)

$ErrorActionPreference = "Stop"

function Resolve-ReleasePublicKey {
    param([Parameter(Mandatory = $true)][string]$ReleaseDir)

    $envKey = [Environment]::GetEnvironmentVariable("HIVEMIND_RELEASE_PUBLIC_KEY", "Process")
    if (-not [string]::IsNullOrWhiteSpace($envKey)) {
        if (Test-Path -LiteralPath $envKey) {
            return $envKey
        }
        throw "HIVEMIND_RELEASE_PUBLIC_KEY points to a missing file: $envKey"
    }

    $scriptKey = Join-Path $PSScriptRoot "release-public-key.pem"
    if (Test-Path -LiteralPath $scriptKey) {
        return $scriptKey
    }

    $installKey = Join-Path (Split-Path -Parent $ReleaseDir) "release-public-key.pem"
    if (Test-Path -LiteralPath $installKey) {
        return $installKey
    }

    throw "Missing trusted release public key. Set HIVEMIND_RELEASE_PUBLIC_KEY or place release-public-key.pem next to this script or install root. Do not trust keys from release/."
}

function Assert-SignedSha256Sums {
    param([Parameter(Mandatory = $true)][string]$ReleaseDir)

    $sums = Join-Path $ReleaseDir "SHA256SUMS"
    $signature = Join-Path $ReleaseDir "SHA256SUMS.sig"
    if (-not (Test-Path -LiteralPath $sums)) {
        throw "Missing signed checksum manifest: $sums"
    }
    if (-not (Test-Path -LiteralPath $signature)) {
        throw "Missing checksum manifest signature: $signature"
    }

    $publicKey = Resolve-ReleasePublicKey -ReleaseDir $ReleaseDir
    $openssl = (Get-Command openssl -ErrorAction SilentlyContinue).Source
    if ([string]::IsNullOrWhiteSpace($openssl)) {
        throw "OpenSSL is required to verify release signatures."
    }

    & $openssl dgst -sha256 -verify $publicKey -signature $signature $sums | Out-Null
    if ($LASTEXITCODE -ne 0) {
        throw "Release signature verification failed for $sums"
    }
}

function Get-ExpectedSha256 {
    param(
        [Parameter(Mandatory = $true)][string]$ArtifactPath,
        [Parameter(Mandatory = $true)][string]$ReleaseDir
    )

    $sums = Join-Path $ReleaseDir "SHA256SUMS"
    $name = [IO.Path]::GetFileName($ArtifactPath)
    foreach ($line in Get-Content -LiteralPath $sums) {
        if ($line -match '^([A-Fa-f0-9]{64})\s+\*?(.+)$') {
            $hash = $matches[1].ToLowerInvariant()
            $entry = [IO.Path]::GetFileName($matches[2].Trim())
            if ($entry -eq $name) {
                return $hash
            }
        }
    }

    throw "Signed SHA256SUMS does not contain an entry for $name"
}

function Copy-VerifiedArtifact {
    param(
        [Parameter(Mandatory = $true)][string]$Source,
        [Parameter(Mandatory = $true)][string]$Target,
        [Parameter(Mandatory = $true)][string]$ReleaseDir
    )

    if (-not (Test-Path -LiteralPath $Source)) {
        throw "Missing release artifact: $Source"
    }
    Assert-SignedSha256Sums -ReleaseDir $ReleaseDir
    $expected = Get-ExpectedSha256 -ArtifactPath $Source -ReleaseDir $ReleaseDir
    $actual = (Get-FileHash -LiteralPath $Source -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actual -ne $expected) {
        throw "Checksum mismatch for $Source. Expected $expected but found $actual."
    }
    Copy-Item -Force -LiteralPath $Source -Destination $Target
    $copied = (Get-FileHash -LiteralPath $Target -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($copied -ne $expected) {
        throw "Copied artifact checksum mismatch for $Target."
    }
}

$binDir = Join-Path $InstallDir "bin"
$releaseDir = Join-Path $InstallDir "release"
$exeSource = Join-Path $releaseDir "worker-executor.exe"
$exeTarget = Join-Path $binDir "worker-executor.exe"
$versionFile = Join-Path $releaseDir "version.txt"

New-Item -ItemType Directory -Force -Path $binDir | Out-Null
Copy-VerifiedArtifact -Source $exeSource -Target $exeTarget -ReleaseDir $releaseDir

if (Test-Path $versionFile) {
    $version = (Get-Content $versionFile -Raw).Trim()
    Write-Host "Updated worker to version $version"
} else {
    Write-Host "Updated worker binary (version metadata missing)"
}
