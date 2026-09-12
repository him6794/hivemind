# Managed prover host support state

## Goal

Codify the legacy managed-proof host contract and the self-contained Windows
Worker package contract. Linux x86_64 remains the currently available sidecar
target; a native Windows package may carry a prover only when a genuine
target-matched PE artifact passes the unchanged equality and attestation gates.
Consensus-enabled managed tasks are separate: their closed DSL executes natively
on Windows and does not require a prover, Docker, WSL, Rust, Cargo, or another
runtime.

## Status

blocked

## Acceptance criteria

- `scripts/build-managed-prover.sh` retains the unchanged Linux/WSL build and
  guest-image equality gate; it does not repin or weaken the Nodepool image ID.
- `scripts/verify-managed-prover-windows.ps1` rejects non-PE, wrong-architecture,
  undeclared-dependency, and un-attested Windows prover artifacts.
- `scripts/package-worker-windows.ps1` packages the validated PE prover beside
  `hivemind-bin.exe`, records it in `SHA256SUMS`, `manifest.json`, and provenance,
  and resolves its path from the package directory at runtime.
- Missing or invalid Windows prover artifacts prevent creation of a
  managed-proof-capable package; runtime failures remain fail-closed.
- No Docker, WSL, Rust, Cargo, RISC Zero toolchain, Linux runtime, or external
  proving service is required on the end-user Windows machine.
- Real Windows PE build, exact guest-ID equality, attestation, clean-machine
  launch, proof verification, and proof-to-settlement evidence are required
  before changing this status to supported.

## Current step

The package and verifier contracts are implemented, but no validated native
Windows RISC Zero prover exists for the locked RISC Zero 3.0.6 stack. The
current branch's guest-image equality and Linux staged-prover attestation are
also blocked. The package script therefore refuses to package without an
operator-supplied target-matched PE artifact and attested SHA-256.

## Completed

- Worker local sidecar protocol remains bounded and fail-closed.
- Windows package artifact manifests can include the prover once the prerequisite
  artifact exists.
- Static verifier, package, release documentation, and HCS contract tests pass.
- The existing Linux verifier remains unchanged and continues to reject anything
  that is not a Linux x86_64 ELF.

## Next checkpoint

Obtain or upstream a supported native Windows RISC Zero prover build for every
advertised Windows architecture. Then run the exact equality test, record the
attestation, validate dependencies on a clean machine, and run real proof and
Nodepool settlement checks. Do not rename the current Linux ELF or update the
Nodepool trust pin.

## Blockers

- RISC Zero 3.0.6's native Windows toolchain/circuit build is currently blocked
  by missing Windows toolchain support and C++/MSVC/linker failures.
- The current guest image does not have a passing equality-gate result, and the
  current staged Linux sidecar has no attested digest.

## Owner

- `/root` coordinates implementation and verification.

## Current recovery evidence — 2026-09-11

The Windows package contract now inventories allowed non-system DLLs beside the
source prover, copies them into the package-local `prover/` directory, and runs
the unchanged PE/architecture/import/dependency/digest verifier against that
final layout. The package contract test passes. This validates packaging
behavior only; it does not create a missing PE artifact or prove native Windows
RISC Zero execution.

The support status remains `blocked` because no genuine target-matched Windows
prover, exact guest-image equality result, attested digest, clean-host launch,
real proof, Nodepool verification, or proof-to-settlement evidence is available.
