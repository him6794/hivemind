$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$readmePath = Join-Path $repoRoot "README.md"
$gettingStartedPath = Join-Path $repoRoot "docs/GETTING_STARTED.md"
$architecturePath = Join-Path $repoRoot "docs/ARCHITECTURE.md"
$envExamplePath = Join-Path $repoRoot ".env.example"

foreach ($path in @($readmePath, $gettingStartedPath, $architecturePath, $envExamplePath)) {
    if (!(Test-Path -LiteralPath $path)) {
        throw "Missing required release documentation: $path"
    }
}

$readme = Get-Content -LiteralPath $readmePath -Raw -Encoding UTF8
$gettingStarted = Get-Content -LiteralPath $gettingStartedPath -Raw -Encoding UTF8
$architecture = Get-Content -LiteralPath $architecturePath -Raw -Encoding UTF8
$envExample = Get-Content -LiteralPath $envExamplePath -Raw -Encoding UTF8

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
    "帳號中心",
    "Master UI",
    "3000",
    "Worker UI",
    "3001",
    "Master API",
    "8082",
    "Worker control",
    "18080",
    "唯一平台 authority",
    "不得暴露給 Worker、browser 或下載的 package",
    "Browser 不直接連線 Nodepool"
)

Assert-Contains -DocumentName "docs/GETTING_STARTED.md" -DocumentText $gettingStarted -ExpectedValues @(
    "MANAGED_CONSENSUS_ROLLOUT_MODE=enforce",
    "strict-majority quorum certificate",
    "validated, assignment-bound",
    "per-replica usage evidence",
    'task-wide `max_cpt` cap',
    "missing or invalid receipts are not billable"
)

$currentDocuments = @{
    "README.md" = $readme
    "docs/GETTING_STARTED.md" = $gettingStarted
    "docs/ARCHITECTURE.md" = $architecture
}

$forbiddenPatterns = @(
    "managed.?prover",
    "worker sidecar",
    "risc.?zero",
    "zkvm",
    "managed.?proof",
    "MANAGED_PROOF",
    "zk-metering-proof-state\\.md",
    "zk-managed-proof-.*\\.md",
    "managed-prover-host-support-state\\.md"
)

foreach ($document in $currentDocuments.GetEnumerator()) {
    foreach ($pattern in $forbiddenPatterns) {
        if ($document.Value -match ("(?i)" + $pattern)) {
            throw "$($document.Key) retains removed proof workflow text matching '$pattern'."
        }
    }
}

Write-Host "release documentation contract tests passed"
