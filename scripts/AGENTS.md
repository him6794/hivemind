# OPERATIONS SCRIPTS

## OVERVIEW
Operational helpers for local builds and packaging-related automation live here.
Windows PowerShell packaging scripts have been removed; prefer the Rust workspace
feature-gated builds and `docker-compose.yml`.

## RULES
Fail fast, report exact artifacts, emit machine-readable results, and make platform
behavior explicit. Never delete user data or hide failed subprocesses.
