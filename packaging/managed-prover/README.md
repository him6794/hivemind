# Managed-proof prover sidecar staging directory

The Worker generates RISC Zero proofs for `managed-function-v0` tasks by
spawning an isolated prover sidecar. RISC Zero 3.0.6 only supports Linux and
macOS prover hosts, and building it pulls in a dedicated guest toolchain that is
far heavier than the rest of this repository's build. The prover is therefore
built once on a supported host and staged here, rather than compiled inside the
regular `hivemind-rs/Dockerfile` build.

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

Run this on a supported Linux host (or WSL) that has the RISC Zero toolchain:

```bash
bash scripts/build-managed-prover.sh
```

The script writes the binary into this directory and prints its SHA-256. Record
that hash in `docs/zk-managed-proof-build-attestation.md` together with the guest
image ID it embeds, so a released image can be traced back to a reproducible
guest build.

## Verifying a staged binary

The embedded guest must match the Nodepool trust pin in
`hivemind-rs/crates/managed-proof/src/lib.rs` (`RISC0_MANAGED_GUEST_ID`). A
mismatch is not a soft failure: the Nodepool rejects every envelope the prover
produces, and all managed tasks fail. Regenerate the pin and the receipt fixture
whenever the shared guest source changes.

The binary itself is deliberately not tracked in git — it is ~95 MB of build
output. Only this README and `.gitkeep` are.
