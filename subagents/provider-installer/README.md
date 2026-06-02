# Provider Installer Scaffold

This scaffold bootstraps a provider worker with low-friction scripts for Windows and Linux.

## Windows

Run in PowerShell:

```powershell
.\install-worker.ps1 -MasterUrl "http://127.0.0.1:8082" -AuthToken "<token>" -InstallDir "C:\hivemind-worker"
```

Update an existing install:

```powershell
.\update-worker.ps1 -InstallDir "C:\hivemind-worker"
```

## Linux

Run in shell:

```bash
./install-worker.sh --master-url http://127.0.0.1:8082 --auth-token <token> --install-dir /opt/hivemind-worker
```

Update an existing install:

```bash
./update-worker.sh --install-dir /opt/hivemind-worker
```

## Notes

- Installer creates a runtime config at `config/worker.env`.
- Update scripts use `release/version.txt` and `release/worker-executor.*` in the install directory as artifact source placeholders.
- Integrate real artifact download/signature validation in release pipeline.
