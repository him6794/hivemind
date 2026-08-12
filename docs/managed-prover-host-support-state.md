# Managed prover host support state

## Goal

Codify the RISC Zero managed-proof prover host contract: Linux, macOS, and WSL
are supported; native Windows proving is rejected early; and the official
`RECURSION_SRC_PATH` offline artifact escape hatch is available and documented.

## Status

complete

## Acceptance criteria

- `scripts/build-managed-prover.sh` accepts Linux and macOS, treats Linux under
  WSL as supported, and rejects native Windows shells with an actionable error.
- The build script resolves and SHA-256 verifies `recursion_zkr.zip` through
  `RECURSION_SRC_PATH` or the local Cargo target tree, without patching RISC
  Zero sources.
- Contract tests fail before the implementation and pass afterward.
- README, environment, staging, and native-Windows package documentation state
  the same support matrix and fail-closed behavior.
- Focused release contract tests and `git diff --check` pass; no native Windows
  proving build is attempted.

## Current step

The build-script host guard and recursion artifact resolver are implemented and
their contract test is green. Documentation and package contract coverage are
implemented, the offline escape-hatch wording is consistent, and the focused
release gate is green.

## Completed

- Existing `scripts/build-managed-prover.Tests.ps1` is intentionally RED: the
  production script lacks the required `MINGW`, `WSL`, digest, and resolver
  contract markers.
- Existing release candidate commits and Docker E2E validation are preserved.
- Focused checks passed: build-script contract, shell syntax, release docs,
  Windows package, release-stack smoke contract, and `git diff --check`.

## Next checkpoint

No further implementation is required for this host-support contract. The
focused local commit is created and remains local; do not push.

## Blockers

- Do not run full RISC Zero proving on native Windows; upstream C++/MSVC limits
  make that host unsupported. Full proving belongs on Linux, macOS, or WSL.

## Owner

- `/root` coordinates implementation, verification, and local commit.
- `/root` completed final review and verification.
