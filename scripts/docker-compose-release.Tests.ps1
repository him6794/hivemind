$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$dockerIgnorePath = Join-Path $repoRoot ".dockerignore"

if (!(Test-Path -LiteralPath $dockerIgnorePath)) {
    throw ".dockerignore must exist at the repository root so the hivemind Docker build context excludes unrelated release artifacts."
}

$dockerIgnore = Get-Content -LiteralPath $dockerIgnorePath -Raw
foreach ($expected in @(
    ".git",
    "frontend",
    "test_logs",
    "subagents"
)) {
    if (!$dockerIgnore.Contains($expected)) {
        throw ".dockerignore must exclude '$expected' from the hivemind Docker build context."
    }
}

foreach ($expected in @(
    "!executor-rs/",
    "!executor-rs/Cargo.toml",
    "!executor-rs/crates/",
    "!executor-rs/crates/managed-function-runtime/",
    "!executor-rs/crates/managed-function-runtime/**"
)) {
    if (!$dockerIgnore.Contains($expected)) {
        throw ".dockerignore must include '$expected' because hivemind-worker-executor has a path dependency inside the executor-rs workspace."
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
    if ($dockerIgnore.Contains($unexpected)) {
        throw ".dockerignore must not include broad executor-rs restore rule '$unexpected' because it pulls unrelated build artifacts into the hivemind image context."
    }
}

foreach ($expected in @(
    "hivemind-rs/target",
    "hivemind-rs/target-alt",
    "hivemind-rs/target-local"
)) {
    if (!$dockerIgnore.Contains($expected)) {
        throw ".dockerignore must exclude '$expected' so local Rust build artifacts do not balloon the hivemind image context."
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
    "cp /app/hivemind-rs/target/release/hivemind-bin",
    "COPY --from=builder /tmp/hivemind-bin /app/hivemind-bin"
)) {
    if (!$dockerfileText.Contains($expected)) {
        throw "hivemind-rs/Dockerfile must preserve the built binary outside the cache mount via '$expected'."
    }
}

$composeOutput = & docker compose config 2>&1
$composeExitCode = $LASTEXITCODE
if ($composeExitCode -ne 0) {
    $joinedOutput = ($composeOutput | Out-String)
    throw "docker compose config must succeed for release packaging.`n${joinedOutput}"
}

Write-Host "docker compose release tests passed"
