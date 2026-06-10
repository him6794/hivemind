#!/usr/bin/env python3
"""Build Hivemind locally on Windows or Linux without Docker.

By default this builds the Rust workspace in release mode and stores the
artifacts under the repository's local target directory on the D drive.
Use --frontend or --all to also build the React frontends.
"""

from __future__ import annotations

import argparse
import os
import subprocess
from pathlib import Path


def run(command: list[str], cwd: Path, env: dict[str, str]) -> None:
    print(f"$ {' '.join(command)}")
    subprocess.run(command, cwd=cwd, env=env, check=True)


def build_backend(repo_root: Path, env: dict[str, str], release: bool) -> None:
    cargo_dir = repo_root / "hivemind-rs"
    cargo = ["cargo", "build", "--workspace"]
    if release:
        cargo.append("--release")
    run(cargo, cargo_dir, env)


def build_frontend(repo_root: Path, env: dict[str, str]) -> None:
    for ui_dir in [repo_root / "frontend" / "master-ui", repo_root / "frontend" / "worker-ui"]:
        run(["npm", "run", "build"], ui_dir, env)


def main() -> int:
    parser = argparse.ArgumentParser(description="Build Hivemind locally without Docker.")
    parser.add_argument(
        "--target-dir",
        default=None,
        help="Cargo target directory. Defaults to <repo>/hivemind-rs/target-local.",
    )
    parser.add_argument(
        "--debug",
        action="store_true",
        help="Build Rust crates in debug mode instead of release.",
    )
    parser.add_argument(
        "--frontend",
        action="store_true",
        help="Also build the React frontends after the Rust backend.",
    )
    parser.add_argument(
        "--all",
        action="store_true",
        help="Build Rust backend and both frontends.",
    )
    args = parser.parse_args()

    repo_root = Path(__file__).resolve().parents[1]
    cargo_target_dir = Path(args.target_dir) if args.target_dir else repo_root / "hivemind-rs" / "target-local"

    env = os.environ.copy()
    env["CARGO_TARGET_DIR"] = str(cargo_target_dir)
    if args.debug:
        env["CARGO_PROFILE_DEV_DEBUG"] = "0"

    release = not args.debug
    build_backend(repo_root, env, release=release)
    if args.frontend or args.all:
        build_frontend(repo_root, env)

    print(f"Build output saved to {cargo_target_dir}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
