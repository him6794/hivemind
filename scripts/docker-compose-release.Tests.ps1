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
    '${POSTGRES_HOST_PORT:-5432}:5432',
    '${NODEPOOL_GRPC_HOST_PORT:-50051}:50051',
    '${TORRENT_TRACKER_HOST_PORT:-6969}:6969',
    '${TORRENT_SEED_HOST_PORT:-6881}:6881',
    '${MASTER_HTTP_HOST_PORT:-8082}:8082',
    '${WORKER_GRPC_HOST_PORT:-50053}:50053',
    '${WORKER_CONTROL_HOST_PORT:-18080}:18080',
    '${MASTER_UI_HOST_PORT:-3000}:80',
    '${WORKER_UI_HOST_PORT:-3001}:80',
    '${SITE_HOST_PORT:-8080}:3000'
)) {
    if (!$composeText.Contains($expectedPortMapping)) {
        throw "docker-compose.yml must allow the release smoke harness to choose a collision-free infrastructure port via '$expectedPortMapping'."
    }
}

foreach ($expectedVolumeName in @(
    '${REDIS_VOLUME_NAME:-hivemind-redis-data}',
    '${POSTGRES_VOLUME_NAME:-hivemind-postgres-data}',
    '${HIVEMIND_DATA_VOLUME_NAME:-hivemind-data}',
    '${NODEPOOL_TORRENTS_VOLUME_NAME:-hivemind-nodepool-torrents}',
    '${NODEPOOL_TASK_PACKAGES_VOLUME_NAME:-hivemind-nodepool-task-packages}',
    '${MASTER_TASK_REFERENCES_VOLUME_NAME:-hivemind-master-task-references}',
    '${MASTER_TORRENTS_VOLUME_NAME:-hivemind-master-torrents}',
    '${WORKER_TASK_DOWNLOADS_VOLUME_NAME:-hivemind-worker-task-downloads}',
    '${WORKER_TORRENTS_VOLUME_NAME:-hivemind-worker-torrents}',
    '${WORKER_GENERAL_COMPUTE_CONFIG_VOLUME_NAME:-hivemind-worker-general-compute-config}',
    '${WORKER_GENERAL_COMPUTE_STATE_VOLUME_NAME:-hivemind-worker-general-compute-state}'
)) {
    if (!$composeText.Contains($expectedVolumeName)) {
        throw "docker-compose.yml must let release smoke isolate persistent state via '$expectedVolumeName'."
    }
}

if (!$composeText.Contains('WORKER_NODEPOOL_TOKEN: ${WORKER_NODEPOOL_TOKEN:-}')) {
    throw "docker-compose.yml must allow an empty WORKER_NODEPOOL_TOKEN so worker registration can wait for UI login."
}
foreach ($expectedWorkerCredential in @(
    'WORKER_NODEPOOL_USERNAME: ${WORKER_NODEPOOL_USERNAME:-}',
    'WORKER_NODEPOOL_PASSWORD: ${WORKER_NODEPOOL_PASSWORD:-}'
)) {
    if (!$composeText.Contains($expectedWorkerCredential)) {
        throw "docker-compose.yml must propagate optional worker registration credential '$expectedWorkerCredential'."
    }
}
if (!$composeText.Contains('HIVEMIND_SEED_DEFAULT_USER: ${HIVEMIND_SEED_DEFAULT_USER:-false}')) {
    throw "docker-compose.yml must keep default-user seeding explicitly opt-in for the reviewed OCI fixture."
}

if (!$composeText.Contains('WORKER_EXECUTION_PUBLIC_KEY_PEM: ${WORKER_EXECUTION_PUBLIC_KEY_PEM:?')) {
    throw "docker-compose.yml must require and propagate WORKER_EXECUTION_PUBLIC_KEY_PEM to the worker."
}

# Production general-compute configuration is an operator-owned boundary.  It
# must be addressed by a fixed in-container path backed by an explicit
# read-only volume, never by an inferred host path or a caller-provided path.
foreach ($expectedProductionSetting in @(
    'HIVEMIND_GENERAL_COMPUTE_PRODUCTION_BACKENDS: /etc/hivemind/general-compute/backends.json',
    'HIVEMIND_GENERAL_COMPUTE_CAS_ROOT: /var/lib/hivemind/general-compute/cas'
)) {
    if (!$composeText.Contains($expectedProductionSetting)) {
        throw "docker-compose.yml must expose the operator-owned production boundary via '$expectedProductionSetting'."
    }
}
foreach ($unexpectedProductionPath in @(
    'HIVEMIND_GENERAL_COMPUTE_PRODUCTION_BACKENDS: ${',
    'HIVEMIND_GENERAL_COMPUTE_PRODUCTION_BACKENDS: ./',
    'HIVEMIND_GENERAL_COMPUTE_PRODUCTION_BACKENDS: ../'
)) {
    if ($composeText.Contains($unexpectedProductionPath)) {
        throw "docker-compose.yml must not infer a host path for production backend configuration ('$unexpectedProductionPath')."
    }
}
foreach ($expectedProductionVolume in @(
    'source: worker-general-compute-config',
    'target: /etc/hivemind/general-compute',
    'source: worker-general-compute-state',
    'target: /var/lib/hivemind/general-compute',
    'read_only: true'
)) {
    if (!$composeText.Contains($expectedProductionVolume)) {
        throw "docker-compose.yml must declare the operator-owned general-compute volume contract '$expectedProductionVolume'."
    }
}

if ($composeText.Contains("MONTY_EXECUTABLE") -or $composeText.Contains("/app/monty")) {
    throw "docker-compose.yml must not retain the removed Monty executable contract."
}

foreach ($expectedCorsSetting in @(
    'MASTER_CORS_ALLOWED_ORIGINS: ${MASTER_CORS_ALLOWED_ORIGINS:-}',
    'WORKER_CONTROL_CORS_ALLOWED_ORIGINS: ${WORKER_CONTROL_CORS_ALLOWED_ORIGINS:-}'
)) {
    if (!$composeText.Contains($expectedCorsSetting)) {
        throw "docker-compose.yml must propagate dynamic release UI origins via '$expectedCorsSetting'."
    }
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
    "!packaging/",
    "!packaging/**",
    "!proto/",
    "!proto/**",
    "!hivemind-rs/",
    "!hivemind-rs/**",
    "!frontend/",
    "!frontend/**",
    "!executor-rs/",
    "!executor-rs/Cargo.toml",
    "!executor-rs/Cargo.lock",
    "!executor-rs/crates/managed-function-runtime/",
    "!executor-rs/crates/managed-function-runtime/**",
    "!executor-rs/crates/general-compute-runtime/",
    "!executor-rs/crates/general-compute-runtime/**"
)) {
    if ($dockerIgnoreLines -notcontains $expected) {
        throw ".dockerignore must contain the exact release-context include '$expected'."
    }
}

if (($dockerIgnoreLines -join "`n") -match "(?i)monty|managed-function-transpiler") {
    throw ".dockerignore must not restore files belonging to the removed Monty workspace."
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

# A production_sandboxed_oci registration is only deployable when the Worker
# image carries an operator-pinned OCI runner. The launcher never falls back to
# direct process execution, so an image without runc cannot execute production
# tasks and must be rejected by the release packaging contract.
if ($dockerfileText -notmatch '(?m)apt-get install -y --no-install-recommends[^\r\n]*\brunc\b[^\r\n]*\buidmap\b') {
    throw "hivemind-rs/Dockerfile runtime stage must install runc and uidmap for rootless production OCI execution."
}
if (!$dockerfileText.Contains("RUN mkdir -p /app/api/torrents /app/bt_torrents /app/sandbox /app/general-compute")) {
    throw "hivemind-rs/Dockerfile must create the operator-owned general-compute state root before dropping privileges."
}
if (!$dockerfileText.Contains("hivemind:100000:65536")) {
    throw "hivemind-rs/Dockerfile must provision an explicit subordinate UID/GID range for the non-root OCI runner."
}

if ($dockerfileText.Contains("COPY --from=builder /app/hivemind-rs/target/release/hivemind-bin")) {
    throw "hivemind-rs/Dockerfile must not copy the release binary directly from the cache-mounted target directory."
}

if (!$dockerfileText.Contains("managed-function-runtime")) {
    throw "hivemind-rs/Dockerfile must stage the managed-function-runtime dependency explicitly."
}

if (!$dockerfileText.Contains("COPY executor-rs/crates/general-compute-runtime ./executor-rs/crates/general-compute-runtime")) {
    throw "hivemind-rs/Dockerfile must stage the general-compute-runtime dependency explicitly for the Worker release build."
}

if (!$dockerfileText.Contains("COPY executor-rs/Cargo.toml executor-rs/Cargo.lock ./executor-rs/")) {
    throw "hivemind-rs/Dockerfile must stage the managed runtime workspace manifest explicitly."
}

if ($dockerfileText -match "(?i)monty|managed-function-transpiler") {
    throw "hivemind-rs/Dockerfile must not build or package the removed Monty workspace."
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

# Managed-function settlement depends on a proof the worker can actually
# produce. RISC Zero has no supported Windows prover host, so the sidecar is
# built separately and staged; if the image stops carrying it, every managed
# task fails closed and no test below this line would notice.
$proverStagingDir = Join-Path $repoRoot "packaging/managed-prover"
if (!(Test-Path -LiteralPath $proverStagingDir)) {
    throw "packaging/managed-prover must exist so the worker image can stage the managed-proof prover sidecar."
}

if (!$dockerfileText.Contains("COPY packaging/managed-prover/ /app/prover/")) {
    throw "hivemind-rs/Dockerfile must stage the managed-proof prover sidecar into /app/prover/."
}

$proverBuildScript = Join-Path $repoRoot "scripts/build-managed-prover.sh"
if (!(Test-Path -LiteralPath $proverBuildScript)) {
    throw "scripts/build-managed-prover.sh must exist so the staged prover sidecar is reproducible on a supported host."
}

foreach ($expectedManagedProofSetting in @(
    'MANAGED_PROOF_ROLLOUT_MODE: ${MANAGED_PROOF_ROLLOUT_MODE:-enforce}',
    'MANAGED_PROVER_EXECUTABLE: ${MANAGED_PROVER_EXECUTABLE:-/app/prover/hivemind-managed-proof-prover}',
    'MANAGED_PROVER_TIMEOUT_SECS: ${MANAGED_PROVER_TIMEOUT_SECS:-900}'
)) {
    if (!$composeText.Contains($expectedManagedProofSetting)) {
        throw "docker-compose.yml must configure managed-proof settlement via '$expectedManagedProofSetting'."
    }
}

$managedProofEnvLines = @(Get-Content -LiteralPath (Join-Path $repoRoot ".env.example"))
if ($managedProofEnvLines -notcontains "MANAGED_PROOF_ROLLOUT_MODE=enforce") {
    throw ".env.example must document the fail-closed default 'MANAGED_PROOF_ROLLOUT_MODE=enforce'."
}

$envExamplePath = Join-Path $repoRoot ".env.example"
if (!(Test-Path -LiteralPath $envExamplePath)) {
    throw "Missing environment template at $envExamplePath."
}

$envExampleLines = @(Get-Content -LiteralPath $envExamplePath)
foreach ($expectedVariable in @(
    "WORKER_EXECUTION_PUBLIC_KEY_PEM",
    "WORKER_NODEPOOL_TOKEN",
    "WORKER_NODEPOOL_USERNAME",
    "WORKER_NODEPOOL_PASSWORD"
)) {
    if ($envExampleLines -notcontains "${expectedVariable}=") {
        throw ".env.example must include an explicit blank ${expectedVariable} entry."
    }
}
foreach ($expectedSetting in @(
    "POSTGRES_HOST_PORT=5432",
    "REDIS_HOST_PORT=6379",
    "NODEPOOL_GRPC_HOST_PORT=50051",
    "TORRENT_TRACKER_HOST_PORT=6969",
    "TORRENT_SEED_HOST_PORT=6881",
    "MASTER_HTTP_HOST_PORT=8082",
    "WORKER_GRPC_HOST_PORT=50053",
    "WORKER_CONTROL_HOST_PORT=18080",
    "MASTER_UI_HOST_PORT=3000",
    "WORKER_UI_HOST_PORT=3001",
    "SITE_HOST_PORT=8080",
    "WORKER_GENERAL_COMPUTE_CONFIG_VOLUME_NAME=hivemind-worker-general-compute-config",
    "WORKER_GENERAL_COMPUTE_STATE_VOLUME_NAME=hivemind-worker-general-compute-state",
    "HIVEMIND_SEED_DEFAULT_USER=false"
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
    "WORKER_NODEPOOL_TOKEN",
    # Cleared so the assertions below read the compose defaults rather than
    # whatever the invoking shell happens to export.
    "HIVEMIND_ADMIN_USERS",
    "MANAGED_PROOF_ROLLOUT_MODE",
    "MANAGED_PROVER_EXECUTABLE",
    "MANAGED_PROVER_TIMEOUT_SECS"
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

    # The dispatcher that applies the rollout policy runs in nodepool, so the
    # setting must land there. Placing it only on the worker silently leaves
    # nodepool on the default, which makes an operator's observe-mode migration
    # a no-op.
    if ($resolvedCompose.services.nodepool.environment.MANAGED_PROOF_ROLLOUT_MODE -ne "enforce") {
        throw "Resolved nodepool configuration must default MANAGED_PROOF_ROLLOUT_MODE to the fail-closed 'enforce'."
    }
    if ($null -ne $resolvedCompose.services.worker.environment.MANAGED_PROOF_ROLLOUT_MODE) {
        throw "MANAGED_PROOF_ROLLOUT_MODE must not be set on the worker: the worker never reads it, so it would imply a policy the worker cannot apply."
    }
    if ($resolvedCompose.services.worker.environment.MANAGED_PROVER_EXECUTABLE -ne "/app/prover/hivemind-managed-proof-prover") {
        throw "Resolved worker configuration must point MANAGED_PROVER_EXECUTABLE at the prover staged into the image."
    }
    if ($resolvedCompose.services.worker.environment.MANAGED_PROVER_TIMEOUT_SECS -ne "900") {
        throw "Resolved worker configuration must allow a full managed proof to complete via MANAGED_PROVER_TIMEOUT_SECS."
    }
    if ($resolvedCompose.services.worker.environment.HIVEMIND_GENERAL_COMPUTE_PRODUCTION_BACKENDS -ne "/etc/hivemind/general-compute/backends.json") {
        throw "Resolved worker configuration must use the fixed in-container production backend registry path."
    }
    if ($resolvedCompose.services.worker.environment.HIVEMIND_GENERAL_COMPUTE_CAS_ROOT -ne "/var/lib/hivemind/general-compute/cas") {
        throw "Resolved worker configuration must keep general-compute CAS state under its dedicated mutable volume."
    }

    $generalComputeConfigMounts = @($resolvedCompose.services.worker.volumes | Where-Object {
        $_.target -eq "/etc/hivemind/general-compute"
    })
    if ($generalComputeConfigMounts.Count -ne 1) {
        throw "Worker must have exactly one operator-owned general-compute config mount."
    }
    $generalComputeConfigMount = $generalComputeConfigMounts[0]
    if ($generalComputeConfigMount.type -ne "volume" -or
        $generalComputeConfigMount.source -ne "worker-general-compute-config" -or
        $generalComputeConfigMount.read_only -ne $true) {
        throw "Production backend config and pinned runner must come from the read-only named volume."
    }
    $generalComputeStateMounts = @($resolvedCompose.services.worker.volumes | Where-Object {
        $_.target -eq "/var/lib/hivemind/general-compute"
    })
    if ($generalComputeStateMounts.Count -ne 1) {
        throw "Worker must have exactly one dedicated general-compute state mount."
    }
    $generalComputeStateMount = $generalComputeStateMounts[0]
    if ($generalComputeStateMount.type -ne "volume" -or
        $generalComputeStateMount.source -ne "worker-general-compute-state" -or
        $generalComputeStateMount.read_only -eq $true) {
        throw "General-compute task bundles and CAS journal must use the dedicated mutable named volume."
    }

    # Nodepool authorizes every admin RPC, so an unpropagated HIVEMIND_ADMIN_USERS
    # makes the whole documented /api/admin/* surface unreachable under Compose —
    # including the managed-proof metrics operators are told to watch during an
    # observe-mode migration.
    if ($null -eq $resolvedCompose.services.nodepool.environment.HIVEMIND_ADMIN_USERS) {
        throw "docker-compose.yml must propagate HIVEMIND_ADMIN_USERS to nodepool, which is where admin authorization happens."
    }
    if ($resolvedCompose.services.nodepool.environment.HIVEMIND_ADMIN_USERS -ne "") {
        throw "Resolved nodepool configuration must default HIVEMIND_ADMIN_USERS to empty so no account is an admin unless one is named."
    }
}
finally {
    foreach ($name in $composeEnvironmentNames) {
        [Environment]::SetEnvironmentVariable($name, $originalComposeEnvironment[$name], "Process")
    }
    Remove-Item -LiteralPath $emptyEnvFile -Force -ErrorAction SilentlyContinue
}

Write-Host "docker compose release tests passed"
