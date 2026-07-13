# Hivemind Crates

## OVERVIEW
Each crate owns one backend boundary; preserve narrow dependencies and feature isolation.

## WHERE TO LOOK
- `common`, `models`, `config`, `proto` — shared foundations
- `auth`, `database` — identity and persistence
- `master-api`, `node-manager`, `task-scheduler` — control plane
- `worker-executor` — untrusted task execution
- `torrent-service`, `vpn-service` — distribution/network integrations

## ANTI-PATTERNS
- Do not create cyclic cross-crate dependencies.
- Do not bypass authorization in a lower-level helper.
- Do not treat generated protobuf output as hand-edited source.