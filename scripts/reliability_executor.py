#!/usr/bin/env python3
"""External worker executor used by the reliability harness.

The worker runtime invokes non-Monty executors as:

    <executor> <task_id> <torrent>

This script performs real local work, returns RESULT_TORRENT on success, and
exits non-zero for failure-injected tasks. It is test infrastructure only; it
does not change the production service architecture or proto schema.
"""

from __future__ import annotations

import hashlib
import os
import sys
import tempfile
import time
from pathlib import Path
from urllib.parse import parse_qs, quote, urlparse


def env_int(name: str, default: int) -> int:
    raw = os.environ.get(name, "").strip()
    if not raw:
        return default
    try:
        value = int(raw)
    except ValueError:
        return default
    return value if value >= 0 else default


def task_kind(task_id: str) -> str:
    lowered = task_id.lower()
    if "failure" in lowered or "fail" in lowered:
        return "failure"
    if "long" in lowered:
        return "long"
    if "retry" in lowered:
        return "retry"
    if "io" in lowered:
        return "io"
    if "parallel" in lowered:
        return "parallel"
    return "cpu"


def btih_from_torrent(task_id: str, torrent: str) -> str:
    parsed = urlparse(torrent)
    if parsed.scheme.lower() == "magnet":
        for xt in parse_qs(parsed.query).get("xt", []):
            if xt.lower().startswith("urn:btih:"):
                candidate = xt[len("urn:btih:") :].strip().lower()
                if len(candidate) == 40:
                    return candidate
    return hashlib.sha1(f"{task_id}|{torrent}".encode("utf-8")).hexdigest()


def sleep_with_progress(seconds: int, label: str) -> None:
    deadline = time.monotonic() + seconds
    next_log = time.monotonic()
    while True:
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            return
        if time.monotonic() >= next_log:
            elapsed = max(0, seconds - int(remaining))
            print(f"executor_progress kind={label} elapsed_sec={elapsed} target_sec={seconds}", flush=True)
            next_log = time.monotonic() + 30
        time.sleep(min(1.0, remaining))


def cpu_work(seconds: int) -> str:
    deadline = time.monotonic() + seconds
    digest = b"seed"
    rounds = 0
    while time.monotonic() < deadline:
        digest = hashlib.sha256(digest + str(rounds).encode("ascii")).digest()
        rounds += 1
    return hashlib.sha1(digest).hexdigest()


def io_work(task_id: str, seconds: int) -> str:
    target = Path(tempfile.gettempdir()) / f"hivemind-reliability-{task_id}.bin"
    payload = hashlib.sha256(task_id.encode("utf-8")).digest() * 4096
    digest = hashlib.sha1()
    deadline = time.monotonic() + seconds
    writes = 0
    try:
        with target.open("wb") as fh:
            while time.monotonic() < deadline:
                fh.write(payload)
                writes += 1
        with target.open("rb") as fh:
            for chunk in iter(lambda: fh.read(1024 * 1024), b""):
                digest.update(chunk)
        digest.update(str(writes).encode("ascii"))
        return digest.hexdigest()
    finally:
        if target.exists():
            target.unlink()


def main(argv: list[str]) -> int:
    if len(argv) < 3:
        print("executor_error missing task_id/torrent", flush=True)
        return 64

    task_id = argv[1]
    torrent = argv[2]
    kind = task_kind(task_id)
    start = time.monotonic()

    print(f"executor_start task_id={task_id} kind={kind}", flush=True)

    if kind == "failure":
        sleep_with_progress(env_int("RELIABILITY_FAILURE_SECONDS", 3), kind)
        print("executor_failure_injected kind=failure", flush=True)
        return 42

    if kind == "long":
        sleep_with_progress(env_int("RELIABILITY_LONG_SECONDS", 900), kind)
        work_hash = cpu_work(env_int("RELIABILITY_LONG_CPU_TAIL_SECONDS", 2))
    elif kind == "retry":
        sleep_with_progress(env_int("RELIABILITY_RETRY_SECONDS", 90), kind)
        work_hash = cpu_work(env_int("RELIABILITY_RETRY_CPU_TAIL_SECONDS", 2))
    elif kind == "io":
        work_hash = io_work(task_id, env_int("RELIABILITY_IO_SECONDS", 5))
    elif kind == "parallel":
        sleep_with_progress(env_int("RELIABILITY_PARALLEL_SECONDS", 10), kind)
        work_hash = cpu_work(env_int("RELIABILITY_PARALLEL_CPU_TAIL_SECONDS", 1))
    else:
        work_hash = cpu_work(env_int("RELIABILITY_CPU_SECONDS", 5))

    btih = btih_from_torrent(task_id, torrent)
    result = f"result://{quote(task_id)}?btih={btih}"
    elapsed = time.monotonic() - start
    print(f"executor_complete task_id={task_id} kind={kind} elapsed_sec={elapsed:.3f} work_hash={work_hash}", flush=True)
    print(f"RESULT_TORRENT={result}", flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
