# Managed-Proof Dependency Audit

Scope: the two Rust workspaces that carry the managed-proof feature.

| Workspace | Lockfile | Role | Trust position |
|---|---|---|---|
| `hivemind-rs` | `hivemind-rs/Cargo.lock` | Nodepool verifier, scheduler, Worker | Nodepool is the trusted authority |
| `zkvm/managed-proof` | `zkvm/managed-proof/Cargo.lock` | Prover sidecar (RISC Zero host) | Runs on the untrusted Worker |

The split matters. The verifier decides whether a managed execution is settled,
so its dependency graph is the one that must stay clean. The prover only
produces a candidate proof; if it is compromised or wrong, the Nodepool rejects
the envelope and the task fails closed.

## `hivemind-rs`: clean

```
cd hivemind-rs && cargo audit
```

0 vulnerabilities. Three warnings are accepted and unrelated to proving:
`derivative` (RUSTSEC-2024-0388, unmaintained), `paste` (RUSTSEC-2024-0436,
unmaintained), and a yanked `spin 0.9.8`. None is a vulnerability and none is
reachable from the verifier's decision path.

The verifier deliberately keeps a narrow feature graph — `risc0-verifier`
enables only the protobuf transport, a no-default-feature RISC Zero verifier,
and `disable-dev-mode`. It does not enable `std`, `prove`, methods, or the
Docker builder, which is why the prover's advisories below do not appear here.

## `zkvm/managed-proof`: two accepted advisories

```
cd zkvm/managed-proof && cargo audit
```

Both findings are in RISC Zero's transitive graph and neither can be fixed by
upgrading. They are recorded in `zkvm/managed-proof/.cargo/audit.toml`.

### RUSTSEC-2023-0071 — `rsa 0.9.10`, Marvin timing side channel

Dependency path:

```
rsa v0.9.10
└── rzup v0.5.2
    ├── risc0-build v3.0.6 → risc0-zkvm v3.0.6 → hivemind-managed-proof-zkvm
    ├── risc0-groth16 v3.0.5 → risc0-zkvm v3.0.6
    └── risc0-zkvm v3.0.6
```

No fixed release exists. `rzup` is the RISC Zero toolchain installer, pulled in
for build-time toolchain resolution. The guest is compiled ahead of the release
build by `scripts/build-managed-prover.sh`, and the shipped sidecar never
invokes `rzup` at runtime — it reads a request on stdin, proves, and exits.

The advisory also requires an attacker to time RSA *private key* operations.
`rzup` performs signature verification, a public-key operation, so even on a
host that does run it the described attack has no private key to recover.

### RUSTSEC-2025-0055 — `tracing-subscriber 0.2.25`, ANSI log poisoning

Dependency path:

```
tracing-subscriber v0.2.25
└── ark-relations v0.5.1
    ├── ark-crypto-primitives v0.5.0 → ark-groth16 v0.5.0 → risc0-groth16 v3.0.5
    ├── ark-groth16 v0.5.0
    └── ark-snark v0.5.1
```

The fix is `>= 0.3.20`, but `ark-relations 0.5.1` requires `^0.2`, so
`cargo update` cannot cross the minor boundary; resolving it needs an upstream
release, not a lockfile change.

Two independent reasons it is not reachable here. The `ark-*` chain enters
through `risc0-groth16`, which implements Groth16 receipt compression: the
prover emits Composite receipts and the Nodepool verifier rejects every receipt
kind except Composite, with exactly one segment. And the advisory concerns
attacker-controlled text reaching a log; the sidecar installs no tracing
subscriber, has stderr redirected to null by its Worker parent, and writes only
the validated response JSON to stdout.

## Re-review triggers

Re-run both audits and revisit this document when any of these change:

- The pinned RISC Zero version moves off 3.0.6.
- The verifier starts accepting Succinct or Groth16 receipts.
- The prover gains logging, or the Worker stops discarding its stderr.
- The sidecar gains any runtime toolchain resolution.
- A fixed `rsa` release appears, or `ark-relations` moves to
  `tracing-subscriber 0.3`.
