$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$readmePath = Join-Path $repoRoot "README.md"
$gettingStartedPath = Join-Path $repoRoot "docs/GETTING_STARTED.md"
$architecturePath = Join-Path $repoRoot "docs/ARCHITECTURE.md"

foreach ($path in @($readmePath, $gettingStartedPath, $architecturePath)) {
    if (!(Test-Path -LiteralPath $path)) {
        throw "Missing required release documentation: $path"
    }
}

$readme = Get-Content -LiteralPath $readmePath -Raw
$gettingStarted = Get-Content -LiteralPath $gettingStartedPath -Raw
$architecture = Get-Content -LiteralPath $architecturePath -Raw

function Assert-Contains {
    param(
        [string]$DocumentName,
        [string]$DocumentText,
        [string[]]$ExpectedValues
    )

    foreach ($expected in $ExpectedValues) {
        if (!$DocumentText.Contains($expected)) {
            throw "$DocumentName must document '$expected'."
        }
    }
}

Assert-Contains -DocumentName "README.md" -DocumentText $readme -ExpectedValues @(
    "Three-surface release",
    "docs/GETTING_STARTED.md",
    "scripts/release-stack-smoke.ps1 -KeepRunning",
    "npm run test:e2e"
)

Assert-Contains -DocumentName "docs/GETTING_STARTED.md" -DocumentText $gettingStarted -ExpectedValues @(
    "Docker",
    "PowerShell",
    "OpenSSL",
    "Node.js 20.9+",
    "POSTGRES_PASSWORD",
    "JWT_SECRET",
    "WORKER_EXECUTION_PRIVATE_KEY_PEM",
    "WORKER_EXECUTION_PUBLIC_KEY_PEM",
    "WORKER_NODEPOOL_TOKEN",
    "scripts/release-stack-smoke.ps1 -KeepRunning",
    "cd frontend",
    "npm ci",
    "npm run test:e2e",
    "HIVEMIND_E2E_EVIDENCE_DIR",
    "docker compose down",
    "collision-free ephemeral",
    "preserves user-supplied",
    "Troubleshooting"
)

Assert-Contains -DocumentName "docs/ARCHITECTURE.md" -DocumentText $architecture -ExpectedValues @(
    "Official Site",
    "8080",
    "account center",
    "Master UI",
    "3000",
    "Worker UI",
    "3001",
    "Master API",
    "8082",
    "worker control",
    "18080",
    "Nodepool is the only trusted authority",
    "must never",
    "connect directly to nodepool"
)

Write-Host "release documentation contract tests passed"
