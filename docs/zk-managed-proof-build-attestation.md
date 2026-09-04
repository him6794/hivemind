# Managed Proof Guest Build Attestation

## Current branch status — blocked (2026-09-04)

This document is a provenance record, not a claim that the current branch has a
reproducible prover. The current branch changes the Worker runtime by adding a
separate GPU-capable implementation and isolates the proof dependency in the
independent frozen crate
`executor-rs/crates/managed-function-runtime-v0`. That frozen crate must
reproduce the existing Nodepool trust pin before a prover can be staged or
released.

The Nodepool trust pin is unchanged:

```text
[466412732, 2327327967, 2963073729, 178423767,
 1914766815, 1823038484, 4206432854, 2659673256]
```

The pin was generated from the build inputs at base commit `01ffb3a` and recorded
at commit `8a2f621`. The runtime source used there has Git blob ID
`c6ac7f42dbc818d702e47b19028bf8631978af69` and SHA-256
`251bccabdd9d173bedf8dcbf2f772e9b5d734e98e982232310347b8bbe15b94c`. That
source was the crate-root
`executor-rs/crates/managed-function-runtime/src/lib.rs` in the then-current
workspace. The current branch keeps that source in the independent frozen crate
at `executor-rs/crates/managed-function-runtime-v0/src/lib.rs`; this copy is a
build input, not a change to the Nodepool pin. The pin is a proof protocol
boundary; changing it requires a new guest attestation, receipt fixture,
verifier rollout, and Nodepool deployment.

The current branch has not yet reproduced that identity after isolating the
GPU runtime. A synchronized Linux build of the current proof workspace ran the
exact one-test selector and failed with a generated ID different from the pin.
The most recent observed failure was:

```text
running 1 test
test tests::generated_guest_id_matches_nodepool_trust_pin ... FAILED

left:  [620973768, 2285667629, 2218031721, 2034546542,
        3727158571, 3602128842, 3095290754, 1686194236]
right: [466412732, 2327327967, 2963073729, 178423767,
        1914766815, 1823038484, 4206432854, 2659673256]
```

A later pinned native WSL attempt using the independent frozen crate did not reach
that selector because the RISC Zero recursion artifact was absent from the
cache and the official download returned HTTP 400 after three attempts. It was
terminated after the build script reported the upstream download failure; no
image ID was produced and no trust pin was changed. This is an infrastructure
block, not proof equality evidence.

Other controlled experiments using different dispatcher, package metadata, and
source-remapping inputs also produced non-matching IDs. These are real failures,
not filtered tests or zero-test successes. Until one exact build environment
passes the equality test, there is no current-branch guest ELF, receipt fixture,
or staged prover that may be described as trusted.

The canonical gate is intentionally exact and must report the selected test as
`ok`:

```text
cargo test --locked -p hivemind-managed-proof-zkvm \
  tests::generated_guest_id_matches_nodepool_trust_pin -- --exact --nocapture
```

`scripts/build-managed-prover.sh` runs this gate before staging a binary and
exits without staging when the environment cannot reproduce the pin. A source
hash, a successful compile, a completed proof, or Worker telemetry is not a
substitute for this equality check.

## Source and runtime boundary

`managed-function-v0` is a frozen proof identity, not a floating alias for the
Worker runtime. The proof host and guest depend on the independent frozen crate
`executor-rs/crates/managed-function-runtime-v0`, whose crate-root source is
kept separate from the default Worker crate. The default Worker build compiles
the separate GPU-capable runtime. GPU operations therefore cannot silently
change the guest semantics, and the proof path cannot silently inherit GPU
code.

The proof build scripts remap
`executor-rs/crates/managed-function-runtime-v0/src/lib.rs` to the canonical
historical path
`/run/desktop/mnt/host/d/hivemind/executor-rs/crates/managed-function-runtime/src/lib.rs`,
while the repository-wide remap covers the remaining source paths.

The proof build also requires the pinned RISC Zero scheme and guest toolchain
used by the trusted build. The native Linux/macOS/WSL path is supported; native
Windows RISC Zero proving remains unsupported and must fail closed. The
container builder is not evidence of identity: a previous container build from
the same source produced a different guest because its guest toolchain and
build environment differed.

The current branch's proof lock and frozen-package inputs are currently hashed as
follows (SHA-256, working tree):

```text
17512820a74c61803766ae09e3278c0e90b78b60b72c43ae77c9a7ad991b4e15  zkvm/managed-proof/Cargo.lock
bafe9a1b15c87662ab9040fa8a6e8965cb7477df3696addfc117030a492d8961  zkvm/managed-proof/methods/guest/Cargo.lock
cd8de5cde2ba5638ac72428b5a4b95724ffb3473246697cbdad56842f03fa2c5  executor-rs/crates/managed-function-runtime-v0/Cargo.lock
a647d2f2c0b4e3b99dc72ba99ae2f3ead00a57e864de05a557491bd37085f7ed  executor-rs/crates/managed-function-runtime-v0/Cargo.toml
57a543a55e5d49916c810e6fd9665bdbfcfb4bdb1d4752e3b241561626f5bb96  zkvm/managed-proof/host/Cargo.toml
92e636ce329041594911473a5aa7ab355fa0c10664816626e149b27434dbcbb2  zkvm/managed-proof/methods/guest/Cargo.toml
```

The pinned WSL invocation uses the following D-backed environment paths:

```text
source root:       /run/desktop/mnt/host/d/hivemind
RISC0_HOME:        /run/desktop/mnt/host/d/hivemind/.cache/zkvm-risc0
CARGO_TARGET_DIR:  /run/desktop/mnt/host/d/hivemind/.cache/zkvm-target
TMPDIR:            /run/desktop/mnt/host/d/hivemind/.cache/zkvm-tmp
```

These hashes describe the current build inputs only; they are not an equality
result. The release remains blocked until the exact selector passes with the
pinned image ID.

## Historical attestation — commit `8a2f621` (2026-08-11)

The following evidence belongs to the trusted historical build at `8a2f621`,
not to the current branch. It remains useful only as provenance for the pin and
as a reference for the exact environment that must be reproduced.

That build generated:

```text
[466412732, 2327327967, 2963073729, 178423767,
 1914766815, 1823038484, 4206432854, 2659673256]
```

The exact selector passed after the pin was updated:

```text
running 1 test
test tests::generated_guest_id_matches_nodepool_trust_pin ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 7 filtered out
mode=pin-test elapsed_seconds=114
cleanup_complete rc=0
```

The historical build used the repository's D:-backed WSL caches and
`HIVEMIND_ZKVM_USE_DOCKER=0`. Its recorded artifact digests were:

```text
a6707caf4c6968befa8f24ff9d9416a9bd6d09a858fd75e7b2be40079aeff036  guest ELF (527,936 bytes)
8221629b1ba7f2a22430cb4b18a8f2ecb02b306bedb1069d6290cbab95f890bb  risc0-managed-proof-v1.json (664,258 bytes)
```

The historical receipt was regenerated against that guest and contained the
pinned image ID. It had a 656-byte journal, one Composite segment at index 0
using poseidon2, no assumption receipts, and a 63,914-word seal. Those values
are not evidence for the current branch until the equality gate passes again.

The historical source digest in an earlier attestation draft did not match the
file in any tracked revision that has been located. The exact `8a2f621` commit
has SHA-256
`4e2e209e45082fe5e6f5f5daffc1cd98d0fcd15c3d57e6a0c6f3556a91ebb907` for that
file. The record below uses the verifiable commit digest and explicitly does
not claim that the incomplete historical manifest proves which uncommitted
source variant produced image ID `466...`.

The historical build inputs included these source and manifest records:

```text
32b480079501507e1667897a5be7cfcc53e27937934fdc70126410da0c81c69e  hivemind-rs/crates/managed-proof/Cargo.toml
4e2e209e45082fe5e6f5f5daffc1cd98d0fcd15c3d57e6a0c6f3556a91ebb907  hivemind-rs/crates/managed-proof/src/lib.rs (exact 8a2f621 commit)
7994b6576771a5cece6fd01cdf7a799fd55b0996b9ce43d1e912700e79d54e88  executor-rs/crates/managed-function-runtime/Cargo.toml
251bccabdd9d173bedf8dcbf2f772e9b5d734e98e982232310347b8bbe15b94c  executor-rs/crates/managed-function-runtime/src/lib.rs
b256dee6c9c6487da6eebac6b8cbf68dc2f98454e21c78ff5fa90edca8d9897d  zkvm/managed-proof/Cargo.lock
5c04ef03b91d5c1d3de2cf95b34fd8fe22214a41c44a1a573894ea23e6d2acf8  zkvm/managed-proof/methods/Cargo.toml
5dafb569aafd6a523fed714722ff5df2bf8dbd551e9b06d76f6f8b7d5666ba2b  zkvm/managed-proof/methods/build.rs
9d646fcfdc9e14204276534b8c4c3b4e13ef0258d17b6bd782d285544b841e5  zkvm/managed-proof/methods/guest/Cargo.toml
760ee5ed3e860e41e010b3b59b1085e0b3e8898b4d2728d3b46045e0b73fdd0c  zkvm/managed-proof/methods/guest/src/main.rs
bafe9a1b15c87662ab9040fa8a6e8965cb7477df3696addfc117030a492d8961  zkvm/managed-proof/methods/guest/Cargo.lock
```

The historical guest lock resolved `managed-function-runtime` as version
`0.0.7`. The host used Rust 1.90.0, while the RISC Zero guest compiler was the
pinned 1.97.0 toolchain. The source was built from the canonical WSL path
`/run/desktop/mnt/host/d/hivemind`, with the pinned RISC Zero 3.0.6 builder and
its verified recursion artifact. Those environment inputs are part of the
provenance; a different host compiler, guest toolchain, path, or lockfile is not
an equivalent attestation.

## Historical staged prover and environment sensitivity

The release worker image may only contain a staged prover whose embedded guest
ID has passed the equality gate. An earlier container-built sidecar had digest
`c0dc79ea479b64af2d9f62f20315b4442d5776390d6091cf8a403ae22e67b983` and
reported:

```text
[851157164, 2331111488, 898154945, 2202623007,
 559143449, 4095204016, 1237502462, 1480841899]
```

Nodepool correctly rejected those envelopes because the ID differed from the
pin. The task ended failed without billing or settlement. This demonstrates
that building from the same source is not sufficient; toolchain, compiler
inputs, source paths, profiles, and other environment details are part of the
proof identity.

The release guard in `scripts/build-managed-prover.sh` therefore runs the pin
test before building. `scripts/verify-staged-prover.sh` additionally checks the
staged file's architecture and attested digest. Neither guard permits an
unverified Worker claim to become settlement authority.

## Historical release-image smoke result — 2026-08-11

A separately recorded historical Compose run used a WSL-built sidecar with
digest `e0fdbd71d410961d0726aa680bfbfeeefe3d7c9278de3cac6d1ee6a8178c508e`.
The Worker runtime used Debian trixie because the supported prover host binary
required GLIBC 2.39. That run reported a verified managed-proof settlement.
It is retained as historical evidence only; it does not validate the current
branch's GPU/runtime isolation or its current proof build.

## Release rule

Do not update the Nodepool pin to make a failing build pass. Do not stage a
prover, regenerate a receipt fixture, or claim managed settlement for the
current branch until the exact generated image ID equals the pinned value in a
supported operator-controlled build environment. If that environment is not
available, the release status is **blocked**, not skipped and not green.
