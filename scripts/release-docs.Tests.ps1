$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$readmePath = Join-Path $repoRoot "README.md"
$gettingStartedPath = Join-Path $repoRoot "docs/GETTING_STARTED.md"
$architecturePath = Join-Path $repoRoot "docs/ARCHITECTURE.md"
$envExamplePath = Join-Path $repoRoot ".env.example"
$proverStagingPath = Join-Path $repoRoot "packaging/managed-prover/README.md"

foreach ($path in @($readmePath, $gettingStartedPath, $architecturePath, $envExamplePath, $proverStagingPath)) {
    if (!(Test-Path -LiteralPath $path)) {
        throw "Missing required release documentation: $path"
    }
}

$readme = Get-Content -LiteralPath $readmePath -Raw -Encoding UTF8
$gettingStarted = Get-Content -LiteralPath $gettingStartedPath -Raw -Encoding UTF8
$architecture = Get-Content -LiteralPath $architecturePath -Raw -Encoding UTF8
$envExample = Get-Content -LiteralPath $envExamplePath -Raw -Encoding UTF8
$proverStaging = Get-Content -LiteralPath $proverStagingPath -Raw -Encoding UTF8

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

# The managed-proof prover host contract. An operator who builds the sidecar on
# the wrong host, or who expects a native Windows worker to prove, only finds
# out when every managed task fails closed, so the README must state it.
Assert-Contains -DocumentName "README.md" -DocumentText $readme -ExpectedValues @(
    "Supported proving hosts",
    "RISC Zero 3.0.6 ships no Windows prover",
    "fails closed",
    "worker image or runtime that contains the",
    "wsl bash scripts/build-managed-prover.sh",
    "RECURSION_SRC_PATH",
    "recursion_zkr.zip",
    "744b999f0a35b3c86753311c7efb2a0054be21727095cf105af6ee7d3f4d8849",
    "official upstream offline escape hatch"
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
    "不得暴露給 Worker、Provider、browser 或下載的 package",
    "Browser 不直接連線 Nodepool"
)

# The managed-prover host contract has to read the same way everywhere an
# operator might look it up. A doc that omits WSL or the fail-closed rule sends
# someone to build a prover on a host RISC Zero cannot support.
$recursionArtifactSha256 = "744b999f0a35b3c86753311c7efb2a0054be21727095cf105af6ee7d3f4d8849"

Assert-Contains -DocumentName "README.md" -DocumentText $readme -ExpectedValues @(
    "Supported proving hosts",
    "| macOS | Yes |",
    "| WSL | Yes |",
    "Native Windows",
    "fails closed",
    "RECURSION_SRC_PATH",
    "recursion_zkr.zip",
    $recursionArtifactSha256,
    "scripts/package-worker-windows.ps1"
)

Assert-Contains -DocumentName ".env.example" -DocumentText $envExample -ExpectedValues @(
    "Linux, macOS, and WSL",
    "native Windows proving is unsupported",
    "fail closed",
    "RECURSION_SRC_PATH",
    "recursion_zkr.zip",
    "official upstream offline escape hatch"
)

Assert-Contains -DocumentName "packaging/managed-prover/README.md" -DocumentText $proverStaging -ExpectedValues @(
    "Linux, macOS, and WSL",
    "Native Windows",
    "fail closed",
    "RECURSION_SRC_PATH",
    "recursion_zkr.zip",
    "SHA-256",
    $recursionArtifactSha256,
    "official upstream offline escape hatch"
)

Write-Host "release documentation contract tests passed"
