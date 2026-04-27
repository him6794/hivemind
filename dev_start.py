#!/usr/bin/env python3
"""Hivemind local all-in-one dev launcher.

Usage:
  python dev_start.py

It starts, in order:
  - PostgreSQL Docker container on localhost:55432 (if Docker is available)
  - Redis Docker container on localhost:56379 (if Docker is available)
  - Nodepool Go service
  - Master Go service
  - Worker Go service
  - Master UI
  - Worker UI

Press Ctrl+C to stop services started by this script.
"""

from __future__ import annotations

import os
import shutil
import signal
import socket
import subprocess
import time
from pathlib import Path


ROOT = Path(__file__).resolve().parent
PG_PORT = 55432
REDIS_PORT = 56379
APP_PORTS = [3000, 3001, 8081, 8082, 50051, 50053, 18080]
MONTY_BIN = ROOT / "executor-rs" / ("monty.exe" if os.name == "nt" else "monty")
EXECUTOR_CLI_BIN = ROOT / "executor-rs" / "target" / "debug" / ("executor-cli.exe" if os.name == "nt" else "executor-cli")
EXECUTOR_BIN = MONTY_BIN if MONTY_BIN.exists() else EXECUTOR_CLI_BIN


def is_windows() -> bool:
    return os.name == "nt"


def port_open(host: str, port: int, timeout: float = 0.5) -> bool:
    try:
        with socket.create_connection((host, port), timeout=timeout):
            return True
    except OSError:
        return False


def run_quiet(args: list[str]) -> tuple[int, str]:
    try:
        p = subprocess.run(args, cwd=ROOT, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=20)
        return p.returncode, p.stdout.strip()
    except Exception as exc:  # noqa: BLE001 - launcher should keep going when optional deps fail
        return 1, str(exc)


def kill_port_windows(port: int) -> None:
    """Kill processes listening on a dev port.

    This intentionally only runs on APP_PORTS, not database ports.
    Set HIVEMIND_NO_KILL=1 to disable.
    """
    if not is_windows() or os.getenv("HIVEMIND_NO_KILL") == "1":
        return

    # Prefer PowerShell because netstat/findstr is brittle with localized output,
    # IPv6, and spacing differences. Fall back to netstat if needed.
    code, out = run_quiet([
        "powershell",
        "-NoProfile",
        "-Command",
        f"Get-NetTCPConnection -LocalPort {port} -State Listen -ErrorAction SilentlyContinue | "
        "Select-Object -ExpandProperty OwningProcess -Unique",
    ])
    pids: set[str] = set()
    if code == 0 and out:
        for line in out.splitlines():
            pid = line.strip()
            if pid.isdigit():
                pids.add(pid)
    else:
        code, out = run_quiet(["cmd", "/c", f"netstat -ano | findstr LISTENING | findstr :{port}"])
        if code == 0 and out:
            for line in out.splitlines():
                parts = line.split()
                if len(parts) >= 5 and parts[-1].isdigit():
                    pids.add(parts[-1])

    for pid in sorted(pids):
        if pid and pid != "0":
            print(f"[cleanup] kill old process on port {port}, pid={pid}")
            run_quiet(["taskkill", "/PID", pid, "/F"])

    # Give Windows a moment to release the socket.
    deadline = time.time() + 5
    while time.time() < deadline and port_open("127.0.0.1", port, timeout=0.1):
        time.sleep(0.2)


def wait_port(label: str, port: int, timeout: int = 30) -> bool:
    print(f"[wait] {label} on port {port}")
    deadline = time.time() + timeout
    while time.time() < deadline:
        if port_open("127.0.0.1", port, timeout=0.2):
            print(f"[ok] {label} ready")
            return True
        time.sleep(1)
    print(f"[warn] {label} not ready after {timeout}s")
    return False


def ensure_docker_container(name: str, run_args: list[str], port: int) -> None:
    if port_open("127.0.0.1", port):
        print(f"[ok] port {port} already open, skip {name}")
        return
    if not shutil.which("docker"):
        print(f"[skip] docker not found; please start {name} yourself on port {port}")
        return

    code, _ = run_quiet(["docker", "inspect", name])
    if code == 0:
        print(f"[start] docker start {name}")
        code, out = run_quiet(["docker", "start", name])
        if code != 0:
            print(f"[warn] docker start {name} failed: {out}")
        return

    print(f"[start] docker run {name}")
    code, out = run_quiet(["docker", "run", "-d", "--name", name, *run_args])
    if code != 0:
        print(f"[warn] docker run {name} failed: {out}")


def npm_install_if_needed(cwd: Path) -> None:
    if (cwd / "node_modules").exists():
        return
    print(f"[setup] npm install in {cwd.relative_to(ROOT)}")
    cmd = "npm install --silent" if is_windows() else "npm install --silent"
    subprocess.run(cmd, cwd=cwd, shell=True, check=False)


def build_executor_if_needed() -> bool:
    if MONTY_BIN.exists():
        print(f"[ok] executor found: {MONTY_BIN}")
        return True
    if EXECUTOR_BIN.exists():
        print(f"[ok] executor found: {EXECUTOR_BIN}")
        return True
    executor_project = ROOT / "executor-rs" / "Cargo.toml"
    if not executor_project.exists():
        print(f"[warn] executor not found: expected {MONTY_BIN}")
        return False
    if not shutil.which("cargo"):
        print("[warn] cargo not found; worker tasks will fail because executor-cli is missing")
        return False
    print("[setup] building executor-cli")
    code = subprocess.run("cargo build --bin executor-cli", cwd=ROOT / "executor-rs", shell=True).returncode
    if code == 0 and EXECUTOR_BIN.exists():
        print(f"[ok] executor built: {EXECUTOR_BIN}")
        return True
    print("[warn] failed to build executor-cli; worker tasks may fail")
    return False


def start_process(name: str, cwd: Path, command: str, env: dict[str, str] | None = None) -> subprocess.Popen:
    merged_env = os.environ.copy()
    if env:
        merged_env.update(env)
    print(f"[start] {name}: {command}")
    return subprocess.Popen(
        command,
        cwd=cwd,
        env=merged_env,
        shell=True,
        creationflags=subprocess.CREATE_NEW_PROCESS_GROUP if is_windows() else 0,
    )


def terminate_process(proc: subprocess.Popen, name: str) -> None:
    if proc.poll() is not None:
        return
    print(f"[stop] {name}")
    try:
        if is_windows():
            proc.send_signal(signal.CTRL_BREAK_EVENT)
            time.sleep(1)
            if proc.poll() is None:
                proc.terminate()
        else:
            proc.terminate()
    except Exception:
        proc.kill()


def wait_and_report() -> None:
    checks = [
        ("PostgreSQL", "127.0.0.1", PG_PORT),
        ("Redis", "127.0.0.1", REDIS_PORT),
        ("Nodepool gRPC", "127.0.0.1", 50051),
        ("Nodepool HTTP", "127.0.0.1", 8081),
        ("Master HTTP", "127.0.0.1", 8082),
        ("Worker gRPC", "127.0.0.1", 50053),
        ("Worker Control HTTP", "127.0.0.1", 18080),
        ("Master UI", "127.0.0.1", 3000),
        ("Worker UI", "127.0.0.1", 3001),
    ]
    print("\n[wait] waiting services...")
    deadline = time.time() + 30
    while time.time() < deadline:
        if all(port_open(host, port, timeout=0.2) for _, host, port in checks):
            break
        time.sleep(1)

    print("\n=== Hivemind local test URLs ===")
    for label, host, port in checks:
        status = "OK" if port_open(host, port, timeout=0.2) else "NOT READY"
        print(f"{label:20s} {host}:{port:<5d} {status}")
    print("\nOpen:")
    print("  Master UI: http://localhost:3000")
    print("  Worker UI: http://localhost:3001")
    print("\nDefault test accounts in docs:")
    print("  testuser / testpass123")
    print("  worker1  / worker123")
    print("\nPress Ctrl+C here to stop Go/npm services started by this script.")


def main() -> int:
    print(f"[root] {ROOT}")

    for port in APP_PORTS:
        kill_port_windows(port)
    still_used = [port for port in APP_PORTS if port_open("127.0.0.1", port, timeout=0.1)]
    if still_used and os.getenv("HIVEMIND_NO_KILL") != "1":
        print(f"[error] these app ports are still occupied after cleanup: {still_used}")
        print("        Close old terminals/processes, or run PowerShell as Administrator and retry.")
        return 1

    ensure_docker_container(
        "pg-hivemind-dev",
        [
            "-p", f"{PG_PORT}:5432",
            "-e", "POSTGRES_USER=hivemind",
            "-e", "POSTGRES_PASSWORD=hivemind",
            "-e", "POSTGRES_DB=hivemind",
            "postgres:16-alpine",
        ],
        PG_PORT,
    )
    ensure_docker_container("redis-hivemind-dev", ["-p", f"{REDIS_PORT}:6379", "redis:7-alpine"], REDIS_PORT)

    wait_port("PostgreSQL", PG_PORT, timeout=30)
    wait_port("Redis", REDIS_PORT, timeout=15)

    npm_install_if_needed(ROOT / "frontend" / "master-ui")
    npm_install_if_needed(ROOT / "frontend" / "worker-ui")
    build_executor_if_needed()

    common_nodepool = {
        "NODEPOOL_POSTGRES_DSN": f"postgres://hivemind:hivemind@127.0.0.1:{PG_PORT}/hivemind?sslmode=disable",
        "NODEPOOL_REDIS_ADDR": f"127.0.0.1:{REDIS_PORT}",
        "NODEPOOL_JWT_SECRET": "dev-secret-change-me",
        "NODEPOOL_ENABLE_HTTP_AUTH": "1",
        "NODEPOOL_HTTP_ADDR": ":8081",
        "NODEPOOL_ADDR": ":50051",
        "NODEPOOL_TASK_TIMEOUT_SEC": "30",
        "NODEPOOL_MAX_REDISPATCH": "2",
        "NODEPOOL_SETTLEMENT_INTERVAL_SEC": "60",
    }

    procs: list[tuple[str, subprocess.Popen]] = []
    try:
        procs.append(("nodepool", start_process("nodepool", ROOT / "services" / "nodepool" / "cmd" / "server", "go run .", common_nodepool)))
        if not wait_port("Nodepool gRPC", 50051, timeout=30):
            print("[error] Nodepool did not start. Check DB/Docker logs above.")
            return 1
        procs.append(("master", start_process("master", ROOT / "services" / "master" / "cmd" / "server", "go run .", {
            "MASTER_HTTP_ADDR": ":8082",
            "NODEPOOL_GRPC_ADDR": "localhost:50051",
            "NODEPOOL_HTTP_BASE": "http://localhost:8081",
            "BT_PUBLIC_BASE_URL": "http://localhost:8082",
        })))
        procs.append(("worker", start_process("worker", ROOT / "services" / "worker" / "cmd" / "server", "go run .", {
            "WORKER_ADDR": ":50053",
            "WORKER_PUBLIC_ADDR": "localhost:50053",
            "WORKER_CONTROL_ADDR": ":18080",
            "NODEPOOL_ADDR": "localhost:50051",
            "WORKER_USERNAME": "worker1",
            "WORKER_PASSWORD": "worker123",
            "WORKER_AUTO_REGISTER": "1",
            "WORKER_EXECUTOR_CMD": str(EXECUTOR_BIN),
            "WORKER_EXECUTOR_RS_BIN": str(EXECUTOR_BIN),
        })))
        procs.append(("master-ui", start_process("master-ui", ROOT / "frontend" / "master-ui", "npm run dev -- --host 127.0.0.1 --strictPort", {
            "VITE_API_BASE": "http://localhost:8082",
        })))
        procs.append(("worker-ui", start_process("worker-ui", ROOT / "frontend" / "worker-ui", "npm run dev -- --host 127.0.0.1 --strictPort", {
            "VITE_API_BASE": "http://localhost:8082",
            "VITE_WORKER_CONTROL_BASE": "http://localhost:18080",
        })))

        wait_and_report()
        reported_exits: set[str] = set()
        while True:
            for name, proc in procs:
                if proc.poll() is not None and name not in reported_exits:
                    print(f"[exit] {name} exited with code {proc.returncode}")
                    reported_exits.add(name)
            time.sleep(3)
    except KeyboardInterrupt:
        print("\n[ctrl-c] stopping...")
    finally:
        for name, proc in reversed(procs):
            terminate_process(proc, name)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())