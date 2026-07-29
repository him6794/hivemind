$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$dockerIgnorePath = Join-Path $repoRoot ".dockerignore"
$composePath = Join-Path $repoRoot "docker-compose.yml"

if (!(Test-Path -LiteralPath $composePath)) {
    throw "Missing release compose file at $composePath."
}

$composeText = Get-Content -LiteralPath $composePath -Raw
foreach ($expectedPortMapping in @(
    '${REDIS_HOST_PORT:-6379}:6379',
    '${POSTGRES_HOST_PORT:-5432}:5432'
)) {
    if (!$composeText.Contains($expectedPortMapping)) {
        throw "docker-compose.yml must allow the release smoke harness to choose a collision-free infrastructure port via '$expectedPortMapping'."
    }
}

if (!$composeText.Contains('WORKER_NODEPOOL_TOKEN: ${WORKER_NODEPOOL_TOKEN:-}')) {
    throw "docker-compose.yml must allow an empty WORKER_NODEPOOL_TOKEN so worker registration can wait for UI login."
}

if (!$composeText.Contains('WORKER_EXECUTION_PUBLIC_KEY_PEM: ${WORKER_EXECUTION_PUBLIC_KEY_PEM:?')) {
    throw "docker-compose.yml must require and propagate WORKER_EXECUTION_PUBLIC_KEY_PEM to the worker."
}

if (!(Test-Path -LiteralPath $dockerIgnorePath)) {
    throw ".dockerignore must exist at the repository root so the hivemind Docker build context excludes unrelated release artifacts."
}

$dockerIgnoreLines = @(Get-Content -LiteralPath $dockerIgnorePath |
    ForEach-Object { $_.Trim() } |
    Where-Object { $_ -and !$_.StartsWith("#") })
foreach ($expected in @(
    "*",
    ".git",
    "test_logs",
    "subagents",
    "hivemind-rs/target",
    "hivemind-rs/target/**",
    "hivemind-rs/target-alt",
    "hivemind-rs/target-alt/**",
    "hivemind-rs/target-local",
    "hivemind-rs/target-local/**",
    "frontend/master-ui/node_modules",
    "frontend/worker-ui/node_modules",
    "frontend/node_modules",
    "frontend/.next",
    "executor-rs/*",
    "executor-rs/crates/*"
)) {
    if ($dockerIgnoreLines -notcontains $expected) {
        throw ".dockerignore must contain the exact release-context exclusion '$expected'."
    }
}

foreach ($expected in @(
    "!proto/",
    "!proto/**",
    "!hivemind-rs/",
    "!hivemind-rs/**",
    "!frontend/",
    "!frontend/**",
    "!executor-rs/",
    "!executor-rs/README.md",
    "!executor-rs/Cargo.toml",
    "!executor-rs/Cargo.lock",
    "!executor-rs/crates/managed-function-runtime/",
    "!executor-rs/crates/managed-function-runtime/**",
    "!executor-rs/crates/managed-function-transpiler/",
    "!executor-rs/crates/managed-function-transpiler/**",
    "!executor-rs/crates/monty/",
    "!executor-rs/crates/monty/**",
    "!executor-rs/crates/monty-cli/",
    "!executor-rs/crates/monty-cli/**",
    "!executor-rs/crates/monty-js/",
    "!executor-rs/crates/monty-js/**",
    "!executor-rs/crates/monty-type-checking/",
    "!executor-rs/crates/monty-type-checking/**",
    "!executor-rs/crates/monty-typeshed/",
    "!executor-rs/crates/monty-typeshed/**"
)) {
    if ($dockerIgnoreLines -notcontains $expected) {
        throw ".dockerignore must contain the exact release-context include '$expected'."
    }
}

foreach ($unexpected in @(
    "!hivemind-rs/target/",
    "!hivemind-rs/target/**",
    "!hivemind-rs/target-alt/",
    "!hivemind-rs/target-alt/**",
    "!hivemind-rs/target-local/",
    "!hivemind-rs/target-local/**",
    "!executor-rs/**",
    "!executor-rs/target/",
    "!executor-rs/target/**"
)) {
    if ($dockerIgnoreLines -contains $unexpected) {
        throw ".dockerignore must not include broad executor-rs restore rule '$unexpected' because it pulls unrelated build artifacts into the hivemind image context."
    }
}

$dockerfilePath = Join-Path $repoRoot "hivemind-rs/Dockerfile"
if (!(Test-Path -LiteralPath $dockerfilePath)) {
    throw "Missing Dockerfile at $dockerfilePath."
}

$dockerfileText = Get-Content -LiteralPath $dockerfilePath -Raw
if ($dockerfileText.Contains("COPY executor-rs ./executor-rs")) {
    throw "hivemind-rs/Dockerfile must not copy the entire executor-rs workspace into the builder image."
}

if ($dockerfileText.Contains("COPY --from=builder /app/hivemind-rs/target/release/hivemind-bin")) {
    throw "hivemind-rs/Dockerfile must not copy the release binary directly from the cache-mounted target directory."
}

if (!$dockerfileText.Contains("managed-function-runtime")) {
    throw "hivemind-rs/Dockerfile must stage the managed-function-runtime dependency explicitly."
}

if (!$dockerfileText.Contains("COPY executor-rs/README.md ./executor-rs/README.md")) {
    throw "hivemind-rs/Dockerfile must stage executor-rs/README.md because the Monty crate embeds it at compile time."
}

if (!$dockerfileText.Contains("18080")) {
    throw "hivemind-rs/Dockerfile must expose worker control HTTP port 18080 for release packaging parity."
}

foreach ($expected in @(
    "--mount=type=cache,target=/usr/local/cargo/registry",
    "--mount=type=cache,target=/usr/local/cargo/git",
    "--mount=type=cache,target=/app/hivemind-rs/target"
)) {
    if (!$dockerfileText.Contains($expected)) {
        throw "hivemind-rs/Dockerfile must use BuildKit cache mount '$expected' to avoid full recompilation on repeat release builds."
    }
}

foreach ($expected in @(
    'cp /app/hivemind-rs/target/release/${HIVEMIND_BIN}',
    'COPY --from=builder /tmp/hivemind-bin /app/hivemind-bin'
)) {
    if (!$dockerfileText.Contains($expected)) {
        throw "hivemind-rs/Dockerfile must preserve the built binary outside the cache mount via '$expected'."
    }
}

$envExamplePath = Join-Path $repoRoot ".env.example"
if (!(Test-Path -LiteralPath $envExamplePath)) {
    throw "Missing environment template at $envExamplePath."
}

$envExampleLines = @(Get-Content -LiteralPath $envExamplePath)
foreach ($expectedVariable in @(
    "WORKER_EXECUTION_PUBLIC_KEY_PEM",
    "WORKER_NODEPOOL_TOKEN"
)) {
    if ($envExampleLines -notcontains "${expectedVariable}=") {
        throw ".env.example must include an explicit blank ${expectedVariable} entry."
    }
}
foreach ($expectedSetting in @(
    "POSTGRES_HOST_PORT=5432",
    "REDIS_HOST_PORT=6379"
)) {
    if ($envExampleLines -notcontains $expectedSetting) {
        throw ".env.example must document the configurable infrastructure mapping '${expectedSetting}'."
    }
}

$composeEnvironmentNames = @(
    "POSTGRES_PASSWORD",
    "JWT_SECRET",
    "WORKER_EXECUTION_PRIVATE_KEY_PEM",
    "WORKER_EXECUTION_PUBLIC_KEY_PEM",
    "WORKER_NODEPOOL_TOKEN"
)
$originalComposeEnvironment = @{}
foreach ($name in $composeEnvironmentNames) {
    $originalComposeEnvironment[$name] = [Environment]::GetEnvironmentVariable($name, "Process")
}

$emptyEnvFile = Join-Path ([IO.Path]::GetTempPath()) ("hivemind-compose-test-" + [guid]::NewGuid().ToString("N") + ".env")
[IO.File]::WriteAllText($emptyEnvFile, "# Intentionally empty; production requirements come from docker-compose.yml.`r`n")

function Invoke-ComposeConfig {
    param([hashtable]$EnvironmentValues)

    foreach ($name in $composeEnvironmentNames) {
        [Environment]::SetEnvironmentVariable($name, $null, "Process")
    }
    foreach ($name in $EnvironmentValues.Keys) {
        [Environment]::SetEnvironmentVariable($name, $EnvironmentValues[$name], "Process")
    }

    $previousErrorActionPreference = $ErrorActionPreference
    try {
        # Native stderr from an expected compose validation failure becomes an
        # ErrorRecord in Windows PowerShell. Capture it so callers can assert
        # the required-variable diagnostic and exit code.
        $ErrorActionPreference = "Continue"
        $output = @(& docker compose --env-file $emptyEnvFile config --format json 2>&1)
        $exitCode = $LASTEXITCODE
    }
    finally {
        $ErrorActionPreference = $previousErrorActionPreference
    }

    return @{
        ExitCode = $exitCode
        Output = $output
    }
}

$requiredComposeEnvironment = @{
    POSTGRES_PASSWORD = "compose-test-postgres-password"
    JWT_SECRET = "compose-test-jwt-secret-with-at-least-32-bytes"
    WORKER_EXECUTION_PRIVATE_KEY_PEM = "compose-test-private-key"
    WORKER_EXECUTION_PUBLIC_KEY_PEM = "compose-test-public-key"
}

try {
    foreach ($requiredName in @(
        "POSTGRES_PASSWORD",
        "JWT_SECRET",
        "WORKER_EXECUTION_PRIVATE_KEY_PEM",
        "WORKER_EXECUTION_PUBLIC_KEY_PEM"
    )) {
        $scenarioEnvironment = @{}
        foreach ($name in $requiredComposeEnvironment.Keys) {
            $scenarioEnvironment[$name] = $requiredComposeEnvironment[$name]
        }
        [void]$scenarioEnvironment.Remove($requiredName)

        $missingResult = Invoke-ComposeConfig -EnvironmentValues $scenarioEnvironment
        if ($missingResult.ExitCode -eq 0) {
            throw "Raw production compose must reject a missing ${requiredName}."
        }

        $missingOutput = $missingResult.Output -join "`n"
        if (!$missingOutput.Contains($requiredName)) {
            throw "Missing ${requiredName} must produce an actionable compose error.`n${missingOutput}"
        }
    }

    $composeResult = Invoke-ComposeConfig -EnvironmentValues $requiredComposeEnvironment
    if ($composeResult.ExitCode -ne 0) {
        $joinedOutput = $composeResult.Output -join "`n"
        throw "docker compose config must succeed without a preprovisioned WORKER_NODEPOOL_TOKEN.`n${joinedOutput}"
    }

    $resolvedCompose = (($composeResult.Output -join "`n") | ConvertFrom-Json)
    if ($resolvedCompose.services.worker.environment.WORKER_NODEPOOL_TOKEN -ne "") {
        throw "Resolved worker configuration must carry an empty WORKER_NODEPOOL_TOKEN when none is preprovisioned."
    }
    if ($resolvedCompose.services.worker.environment.WORKER_EXECUTION_PUBLIC_KEY_PEM -ne $requiredComposeEnvironment.WORKER_EXECUTION_PUBLIC_KEY_PEM) {
        throw "Resolved worker configuration must receive WORKER_EXECUTION_PUBLIC_KEY_PEM unchanged."
    }
    if ($resolvedCompose.services.worker.environment.EXECUTOR_NETWORK_EGRESS_TARGETS -ne "172.28.0.0/24") {
        throw "Resolved worker egress targets must use the IP/CIDR syntax accepted by SandboxEgressPolicy."
    }
}
finally {
    foreach ($name in $composeEnvironmentNames) {
        [Environment]::SetEnvironmentVariable($name, $originalComposeEnvironment[$name], "Process")
    }
    Remove-Item -LiteralPath $emptyEnvFile -Force -ErrorAction SilentlyContinue
}

Write-Host "docker compose release tests passed"
