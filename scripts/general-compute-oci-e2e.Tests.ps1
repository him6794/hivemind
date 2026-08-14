$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$harnessPath = Join-Path $PSScriptRoot "general-compute-oci-e2e.ps1"

if (!(Test-Path -LiteralPath $harnessPath -PathType Leaf)) {
    throw "Missing operator OCI E2E harness at $harnessPath."
}

$harnessText = Get-Content -LiteralPath $harnessPath -Raw

foreach ($expected in @(
    "param(",
    "[switch]`$CheckOnly",
    "[switch]`$Run",
    "HIVEMIND_ENABLE_REAL_OCI_E2E",
    "HIVEMIND_GENERAL_COMPUTE_PRODUCTION_BACKENDS",
    "docker compose",
    "--project-name",
    "--project-directory",
    "finally",
    "RunnerNotPinned",
    "rootless",
    "seccomp",
    "SCMP_ACT_ERRNO"
)) {
    if (!$harnessText.Contains($expected)) {
        throw "OCI E2E harness must contain the fail-closed/operator-isolation contract '$expected'."
    }
}

if ($harnessText -notmatch '(?i)operator.*registry|registry.*operator') {
    throw "OCI E2E harness must describe the registry as operator-owned."
}
if ($harnessText -notmatch '(?i)rootfs') {
    throw "OCI E2E harness must validate an operator-provisioned rootfs."
}
if ($harnessText -notmatch '(?i)sha256') {
    throw "OCI E2E harness must verify pinned SHA-256 material."
}
if ($harnessText -notmatch '(?i)cleanup|down') {
    throw "OCI E2E harness must clean up its isolated Compose project."
}
if ($harnessText -match '(?i)fallback.*direct|direct.*fallback') {
    throw "OCI E2E harness must not offer a direct-process fallback."
}
if ($harnessText -match '(?i)MONTY_EXECUTABLE|/app/monty') {
    throw "OCI E2E harness must not reintroduce the removed Monty contract."
}

Write-Host "general-compute OCI E2E harness contract passed"
