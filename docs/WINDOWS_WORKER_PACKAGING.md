# Windows Worker Packaging

This is the release path for a Windows-first provider worker package.

## Build

From the repository root:

```powershell
.\scripts\package-worker-windows.ps1 `
  -NodepoolGrpcAddr "nodepool.example.com:50051" `
  -WorkerGrpcAddr "0.0.0.0:50053" `
  -WorkerControlHttpAddr "127.0.0.1:18080"
```

The script builds `hivemind-bin.exe` and writes a package to `dist\windows-worker`.

## Package Contents

- `hivemind-bin.exe`: Rust service binary.
- `.env.worker.example`: provider-facing worker configuration template.
- `start-worker.ps1`: loads `.env.worker` and starts `hivemind-bin.exe worker`.
- `README.md`: provider setup instructions.

## Provider Setup

1. Copy `.env.worker.example` to `.env.worker`.
2. Set `NODEPOOL_GRPC_ADDR` to the nodepool gRPC address.
3. Set `WORKER_ADVERTISE_ADDR` when the worker is reachable through a public IP, LAN IP, VPN IP, or Tailscale address that differs from `WORKER_GRPC_ADDR`.
4. Place `monty.exe` next to `hivemind-bin.exe`, or set `MONTY_EXECUTABLE` to an absolute path.
5. Run:

```powershell
.\start-worker.ps1
```

## Release Gate

Before publishing the package:

```powershell
cd hivemind-rs
cargo test --workspace
cd ..
npm --prefix frontend\worker-ui run build
.\scripts\package-worker-windows.ps1
```

The package is acceptable for MVP release when the worker appears in `/api/workers`, provider settings can be edited from Worker UI, and a submitted task can be assigned to the worker over gRPC.
