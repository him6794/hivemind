param(
    [Parameter(Mandatory = $true)][string]$Version,
    [string]$OutputDir = "dist\release",
    [string]$MontyExecutable = ""
)

$ErrorActionPreference = "Stop"
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$rustRoot = Join-Path $repoRoot "hivemind-rs"
$output = Join-Path $repoRoot $OutputDir

if ($Version -notmatch '^[0-9]+\.[0-9]+\.[0-9]+([-.][0-9A-Za-z.-]+)?$') {
    throw "Version must use semantic-version form, for example 1.0.0 or 1.0.0-rc.1."
}
if (Test-Path $output) {
    throw "Output directory already exists: $output. Choose another OutputDir to avoid overwriting artifacts."
}
if ([string]::IsNullOrWhiteSpace($MontyExecutable)) {
    throw "MontyExecutable is required for a publishable worker release."
}
if (!(Test-Path -LiteralPath $MontyExecutable -PathType Leaf)) {
    throw "Monty executable was not found: $MontyExecutable"
}

$roles = @(
    @{ Name = "hivemind-master"; Features = "master" },
    @{ Name = "hivemind-nodepool"; Features = "nodepool" },
    @{ Name = "hivemind-worker"; Features = "worker" }
)

New-Item -ItemType Directory -Force -Path $output | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $output "bin") | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $output "ui\master") | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $output "ui\worker") | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $output "ui\root") | Out-Null

Push-Location $rustRoot
try {
    foreach ($role in $roles) {
        cargo build --release --no-default-features --features $role.Features --bin $role.Name
        Copy-Item -LiteralPath (Join-Path $rustRoot "target\release\$($role.Name).exe") -Destination (Join-Path $output "bin")
    }
} finally { Pop-Location }

foreach ($ui in @(
    @{ Name = "root"; Path = "frontend" },
    @{ Name = "master"; Path = "frontend\master-ui" },
    @{ Name = "worker"; Path = "frontend\worker-ui" }
)) {
    Push-Location (Join-Path $repoRoot $ui.Path)
    try {
        npm ci --ignore-scripts
        npm run build
    } finally { Pop-Location }
    Copy-Item -Recurse -Path (Join-Path $repoRoot "$($ui.Path)\dist\*") -Destination (Join-Path $output "ui\$($ui.Name)")
}

Copy-Item -LiteralPath $MontyExecutable -Destination (Join-Path $output "bin")

$manifest = [ordered]@{
    version = $Version
    generated_at_utc = (Get-Date).ToUniversalTime().ToString("o")
    binaries = $roles.Name
    ui = @("root", "master", "worker")
    worker_runtime = (Split-Path -Leaf $MontyExecutable)
    protocol_source = "proto/hivemind.proto"
}
$manifest | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $output "release-manifest.json") -Encoding UTF8

$hashes = Get-ChildItem -Recurse -File $output | Where-Object { $_.Name -ne "SHA256SUMS" } | ForEach-Object {
    $hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $_.FullName).Hash.ToLowerInvariant()
    "{0}  {1}" -f $hash, ($_.FullName.Substring($output.Length + 1) -replace '\\', '/')
}
$hashes | Sort-Object | Set-Content -LiteralPath (Join-Path $output "SHA256SUMS") -Encoding ASCII
Write-Host "Release artifacts written to $output"
