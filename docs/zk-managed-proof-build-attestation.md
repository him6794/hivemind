# Managed Proof Guest Build Attestation

Date: 2026-08-09  
Base commit: `7d93e9bc738d6cde68f37d897e0eb26f9c67420d`

## Result

The RISC Zero methods build generated this guest image ID from the current working source:

```text
[3606400121, 4250889949, 2277454476, 3430793801,
 2111044864, 2713379816, 851522248, 2751351423]
```

The generated ID equals both `hivemind_managed_proof_methods::HIVEMIND_MANAGED_PROOF_GUEST_ID` and the Nodepool trust pin `hivemind_managed_proof::RISC0_MANAGED_GUEST_ID`. The tracked regression test is `tests::generated_guest_id_matches_nodepool_trust_pin` in `zkvm/managed-proof/host/src/lib.rs`.

Verification result:

```text
running 1 test
test tests::generated_guest_id_matches_nodepool_trust_pin ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 4 filtered out
mode=pin-test elapsed_seconds=39
cleanup_complete rc=0
```

The canonical test command is:

```text
cargo test --locked -p hivemind-managed-proof-zkvm \
  tests::generated_guest_id_matches_nodepool_trust_pin -- --exact
```

The local run used the repository's D:-backed WSL caches with `HIVEMIND_ZKVM_USE_DOCKER=0`. After the command, neither `/run/desktop/mnt/host/d` nor `/root/.cargo` remained mounted.

## Artifact digests

```text
f921fcf53c2ca0f6a00d322d0c6e6441af3d683ff638edacbda35531522bf429  guest ELF
5eaff68d05f00235908298df01ccb5d1dd1c03a0677f8911fbf06f5839179a29  risc0-managed-proof-v1.json
```

The generated ELF path for this run was:

```text
D:\hivemind\.cache\zkvm-target\riscv-guest\hivemind-managed-proof-methods\hivemind-managed-proof-guest\riscv32im-risc0-zkvm-elf\release\hivemind-managed-proof-guest.bin
```

## Build-input digests

All values are SHA-256:

```text
32b480079501507e1667897a5be7cfcc53e27937934fdc70126410da0c81c69e  hivemind-rs/crates/managed-proof/Cargo.toml
fa2048629f91c68abb5458967a57813923ce4ad884017b7507e941aa5c36ddce  hivemind-rs/crates/managed-proof/src/lib.rs
7994b6576771a5cece6fd01cdf7a799fd55b0996b9ce43d1e912700e79d54e88  executor-rs/crates/managed-function-runtime/Cargo.toml
3199de8cba84e621b54bc10000a5edba44d391475056fe039a6c4774ea6c405a  executor-rs/crates/managed-function-runtime/src/lib.rs
46354017fb305c9854564153b353a3c98ed3741d6b73ebcc8bfa613d67a5667e  zkvm/managed-proof/Cargo.lock
5c04ef03b91d5c1d3de2cf95b34fd8fe22214a41c44a1a573894ea23e6d2acf8  zkvm/managed-proof/methods/Cargo.toml
5dafb569aafd6a523fed714722ff5df2bf8dbd551e9b06d76f6f8b7d5666ba2b  zkvm/managed-proof/methods/build.rs
9d646fcfd9c9e14204276534b8c4c3b4e13ef0258d17b6bd782d285544b841e5  zkvm/managed-proof/methods/guest/Cargo.toml
1a76923d7a7254e113798ddb2e0e99494db1693b0febb68d2c83c453888f291d  zkvm/managed-proof/methods/guest/src/main.rs
```

This attestation records the local build evidence; the tracked pin-equality test remains the executable guard for future source or dependency changes.
