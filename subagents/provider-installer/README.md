# Provider Installer Scaffold

This scaffold bootstraps a provider worker with low-friction shell install/update scripts.

## Install / Update

```bash
./install-worker.sh --master-url http://127.0.0.1:8082 --auth-token <token> --install-dir /opt/hivemind-worker
./update-worker.sh --install-dir /opt/hivemind-worker
```

## Notes

- Installer creates a runtime config at `config/worker.env`.
- Install/update scripts use `release/version.txt` and `release/worker-executor.*` in the install directory as artifact source placeholders.
- Worker artifacts must be listed in `release/SHA256SUMS`, and that manifest must have a detached OpenSSL signature at `release/SHA256SUMS.sig`.
- Install/update scripts verify `SHA256SUMS.sig` before trusting any checksum entry, then verify the source and copied artifact hash.
- Provide the trusted release public key with `HIVEMIND_RELEASE_PUBLIC_KEY=/path/to/release-public-key.pem`, or place `release-public-key.pem` next to the installer script or in the install root. Keys inside `release/` are intentionally ignored because that directory is the untrusted artifact source.

Example release signing flow:

```bash
sha256sum worker-executor > SHA256SUMS
openssl dgst -sha256 -sign release-private-key.pem -out SHA256SUMS.sig SHA256SUMS
```
