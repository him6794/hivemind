# Managed-proof prover sidecar staging directory

The Worker generates RISC Zero proofs for `managed-function-v0` tasks by
spawning an isolated prover sidecar. Building it pulls in a dedicated guest
toolchain that is far heavier than the rest of this repository's build, so the
prover is built once on a supported host and staged here, rather than compiled
inside the regular `hivemind-rs/Dockerfile` build.

## Supported proving hosts

RISC Zero 3.0.6 proving hosts are Linux, macOS, and WSL. Native Windows proving
is unsupported — RISC Zero ships no Windows prover, and Hivemind does not
emulate one. `scripts/build-managed-prover.sh` refuses to run under a native
Windows shell (MINGW/MSYS/Cygwin) up front, rather than failing deep inside a
RISC Zero build script. WSL reports `Linux`, so it takes the supported path.

A native Windows Worker still runs ordinary worker workloads, but it ships no
prover sidecar, so managed tasks fail closed there. Managed tasks must run on a
worker image or runtime that contains the Linux prover sidecar staged in this
directory.

## What goes here

A single Linux `x86_64` executable named:

```
hivemind-managed-proof-prover
```

`hivemind-rs/Dockerfile` copies the whole directory to `/app/prover/` in the
runtime image. When the binary is absent the image still builds; the Worker then
fails every managed task closed, because `MANAGED_PROVER_EXECUTABLE` cannot be
spawned. That is the intended safe behaviour, not a silent downgrade — an
unproven managed execution is never settled.

## Producing the binary

Run this on a Linux, macOS, or WSL host that has the RISC Zero toolchain:

```bash
bash scripts/build-managed-prover.sh
```

From a Windows checkout, run the same build through WSL:

```powershell
wsl bash scripts/build-managed-prover.sh
```

The script writes the binary into this directory and prints its SHA-256. Record
that hash in `docs/zk-managed-proof-build-attestation.md` together with the guest
image ID it embeds, so a released image can be traced back to a reproducible
guest build.

## Building without access to the RISC Zero artifact bucket

The recursion circuit build downloads `recursion_zkr.zip` from
`risc0-artifacts.s3.us-west-2.amazonaws.com`. Where network policy blocks that
bucket, use `RECURSION_SRC_PATH` — the official upstream offline escape hatch —
rather than patching anything in the RISC Zero registry sources:

```bash
RECURSION_SRC_PATH=/path/to/recursion_zkr.zip bash scripts/build-managed-prover.sh
```

`scripts/build-managed-prover.sh` verifies that artifact's SHA-256 against
`744b999f0a35b3c86753311c7efb2a0054be21727095cf105af6ee7d3f4d8849` before
handing it to Cargo, and aborts on a mismatch. This reuses a checked artifact —
it does not skip the check. With `RECURSION_SRC_PATH` unset the script reuses a
`recursion_zkr.zip` already present in the Cargo target tree under the same
digest check, and otherwise leaves RISC Zero to its normal network download.

## Verifying a staged binary

The embedded guest must match the Nodepool trust pin in
`hivemind-rs/crates/managed-proof/src/lib.rs` (`RISC0_MANAGED_GUEST_ID`). A
mismatch is not a soft failure: the Nodepool rejects every envelope the prover
produces, and all managed tasks fail. Regenerate the pin and the receipt fixture
whenever the shared guest source changes.

The binary itself is deliberately not tracked in git — it is ~95 MB of build
output. Only this README and `.gitkeep` are.
