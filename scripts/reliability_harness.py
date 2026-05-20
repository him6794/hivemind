#!/usr/bin/env python3
"""Run real reliability validation against the local Hivemind services.

This harness intentionally starts real Docker dependencies, real Go service
processes, real worker executors, TCP latency proxies, and performs HTTP API
submission/verification. It should be used for evidence collection; a passing
calibration run is not the full DoD unless the run parameters satisfy the DoD.
"""

from __future__ import annotations

import argparse
import contextlib
import hashlib
import http.client
import json
import os
import random
import shutil
import signal
import socket
import subprocess
import sys
import threading
import time
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Any
from urllib.parse import urlencode


ROOT = Path(__file__).resolve().parents[1]
LOG_ROOT = ROOT / "test_logs" / "reliability"


def now_iso() -> str:
    return datetime.now(timezone.utc).isoformat()


def append_log(path: Path, message: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("a", encoding="utf-8") as fh:
        fh.write(f"{now_iso()} {message}\n")


def run_cmd(args: list[str], cwd: Path = ROOT, timeout: int = 120, env: dict[str, str] | None = None) -> subprocess.CompletedProcess[str]:
    return subprocess.run(args, cwd=str(cwd), env=env, text=True, capture_output=True, timeout=timeout, check=False)


def wait_port(host: str, port: int, timeout: float) -> None:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        with contextlib.closing(socket.socket(socket.AF_INET, socket.SOCK_STREAM)) as sock:
            sock.settimeout(0.5)
            try:
                sock.connect((host, port))
                return
            except OSError:
                time.sleep(0.25)
    raise RuntimeError(f"port did not open: {host}:{port}")


def http_json(method: str, host: str, port: int, path: str, body: Any | None = None, token: str = "", timeout: int = 30) -> Any:
    data = None
    headers = {"Content-Type": "application/json"}
    if token:
        headers["Authorization"] = f"Bearer {token}"
    if body is not None:
        data = json.dumps(body, separators=(",", ":")).encode("utf-8")
    conn = http.client.HTTPConnection(host, port, timeout=timeout)
    try:
        conn.request(method, path, body=data, headers=headers)
        resp = conn.getresponse()
        raw = resp.read()
    finally:
        conn.close()
    text = raw.decode("utf-8", errors="replace")
    try:
        parsed = json.loads(text) if text else {}
    except json.JSONDecodeError:
        parsed = {"raw": text}
    if resp.status >= 400:
        raise RuntimeError(f"{method} {path} returned HTTP {resp.status}: {parsed}")
    return parsed


class DelayProxy:
    def __init__(self, name: str, listen_port: int, target_port: int, latency_ms: int, jitter_ms: int, log_path: Path, host: str = "127.0.0.1") -> None:
        self.name = name
        self.listen_port = listen_port
        self.target_port = target_port
        self.latency_ms = latency_ms
        self.jitter_ms = jitter_ms
        self.log_path = log_path
        self.host = host
        self._stop = threading.Event()
        self._sock: socket.socket | None = None
        self._thread: threading.Thread | None = None
        self.connections = 0
        self.bytes_forwarded = 0

    def start(self) -> None:
        self.log_path.parent.mkdir(parents=True, exist_ok=True)
        self._sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self._sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self._sock.bind((self.host, self.listen_port))
        self._sock.listen()
        self._sock.settimeout(0.5)
        self._thread = threading.Thread(target=self._accept_loop, name=f"proxy-{self.name}", daemon=True)
        self._thread.start()
        self._log(f"proxy_start listen={self.listen_port} target={self.target_port} latency_ms={self.latency_ms} jitter_ms={self.jitter_ms}")

    def stop(self) -> None:
        self._stop.set()
        if self._sock is not None:
            try:
                self._sock.close()
            except OSError as exc:
                self._log(f"socket_close_error err={exc}")
        if self._thread is not None:
            self._thread.join(timeout=2)
        self._log(f"proxy_stop connections={self.connections} bytes_forwarded={self.bytes_forwarded}")

    def _log(self, message: str) -> None:
        with self.log_path.open("a", encoding="utf-8") as fh:
            fh.write(f"{now_iso()} {self.name} {message}\n")

    def _accept_loop(self) -> None:
        assert self._sock is not None
        while not self._stop.is_set():
            try:
                client, addr = self._sock.accept()
            except OSError:
                if not self._stop.is_set():
                    self._log("accept_loop_error")
                break
            self.connections += 1
            self._log(f"accept remote={addr[0]}:{addr[1]}")
            threading.Thread(target=self._handle, args=(client,), daemon=True).start()

    def _handle(self, client: socket.socket) -> None:
        try:
            upstream = socket.create_connection((self.host, self.target_port), timeout=5)
        except OSError as exc:
            self._log(f"connect_failed target={self.target_port} err={exc}")
            client.close()
            return
        upstream.settimeout(None)
        client.settimeout(None)
        threading.Thread(target=self._pipe, args=(client, upstream, "c2s"), daemon=True).start()
        threading.Thread(target=self._pipe, args=(upstream, client, "s2c"), daemon=True).start()

    def _pipe(self, src: socket.socket, dst: socket.socket, direction: str) -> None:
        with contextlib.ExitStack() as stack:
            stack.callback(lambda: src.close())
            stack.callback(lambda: dst.close())
            while not self._stop.is_set():
                try:
                    data = src.recv(65536)
                except OSError as exc:
                    self._log(f"recv_error direction={direction} err={exc}")
                    return
                if not data:
                    return
                delay = self.latency_ms + (random.randint(0, self.jitter_ms) if self.jitter_ms > 0 else 0)
                if delay > 0:
                    time.sleep(delay / 1000.0)
                try:
                    dst.sendall(data)
                except OSError as exc:
                    self._log(f"send_error direction={direction} err={exc}")
                    return
                self.bytes_forwarded += len(data)
                if random.random() < 0.01:
                    self._log(f"forward direction={direction} bytes={len(data)} delay_ms={delay}")


@dataclass
class ManagedProcess:
    name: str
    process: subprocess.Popen[str]
    stdout: Path
    stderr: Path
    env: dict[str, str] = field(default_factory=dict)

    @property
    def pid(self) -> int:
        return self.process.pid

    def is_alive(self) -> bool:
        return self.process.poll() is None

    def stop(self) -> None:
        if self.process.poll() is not None:
            return
        if os.name == "nt":
            self.process.terminate()
        else:
            self.process.send_signal(signal.SIGTERM)
        try:
            self.process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self.process.kill()

    def kill(self) -> None:
        if self.process.poll() is None:
            self.process.kill()


class ReliabilityHarness:
    def __init__(self, args: argparse.Namespace) -> None:
        self.args = args
        self.run_root = LOG_ROOT / datetime.now().strftime("%Y%m%d-%H%M%S")
        self.bin_dir = ROOT / "test_logs" / "bin"
        self.processes: dict[str, ManagedProcess] = {}
        self.proxies: list[DelayProxy] = []
        self.rng = random.Random(args.seed)
        self.summary: dict[str, Any] = {
            "started_at": now_iso(),
            "seed": args.seed,
            "parameters": vars(args),
            "runs": [],
            "regressions": [],
            "passed": False,
            "dod_satisfied": False,
        }

    def run(self) -> int:
        self.run_root.mkdir(parents=True, exist_ok=True)
        try:
            self.build_binaries()
            for index in range(1, self.args.runs + 1):
                run_result = self.run_once(index)
                self.summary["runs"].append(run_result)
                self.write_reports()
                if not run_result["passed"]:
                    self.summary["regressions"].append({"run": index, "failures": run_result["failures"]})
                    if self.args.stop_on_failure:
                        break
            self.summary["passed"] = len(self.summary["runs"]) == self.args.runs and all(r["passed"] for r in self.summary["runs"])
            self.summary["dod_satisfied"] = self.summary["passed"] and self.args.runs >= 10 and self.args.failure_simulations >= 3 and self.args.long_seconds >= 900
            self.summary["finished_at"] = now_iso()
            self.write_reports()
            return 0 if self.summary["passed"] else 1
        finally:
            self.cleanup()

    def build_binaries(self) -> None:
        self.bin_dir.mkdir(parents=True, exist_ok=True)
        targets = [
            ("nodepool", ROOT / "services" / "nodepool", ["go", "build", "-o", str(self.bin_dir / exe_name("nodepool")), ".\\cmd\\server" if os.name == "nt" else "./cmd/server"]),
            ("master", ROOT / "services" / "master", ["go", "build", "-o", str(self.bin_dir / exe_name("master")), ".\\cmd\\server" if os.name == "nt" else "./cmd/server"]),
            ("worker", ROOT / "services" / "worker", ["go", "build", "-o", str(self.bin_dir / exe_name("worker")), ".\\cmd\\server" if os.name == "nt" else "./cmd/server"]),
        ]
        for name, cwd, cmd in targets:
            cp = run_cmd(cmd, cwd=cwd, timeout=180)
            (self.run_root / f"build-{name}.out.log").write_text(cp.stdout, encoding="utf-8")
            (self.run_root / f"build-{name}.err.log").write_text(cp.stderr, encoding="utf-8")
            if cp.returncode != 0:
                raise RuntimeError(f"build failed for {name}: {cp.stderr}")

    def run_once(self, index: int) -> dict[str, Any]:
        run_id = f"rel-{datetime.now().strftime('%Y%m%d%H%M%S')}-r{index:02d}"
        run_dir = self.run_root / run_id
        run_dir.mkdir(parents=True, exist_ok=True)
        result: dict[str, Any] = {
            "run": index,
            "run_id": run_id,
            "started_at": now_iso(),
            "tasks": {},
            "failures": [],
            "events": [],
            "passed": False,
        }
        self.cleanup_runtime()
        try:
            self.start_dependencies(run_dir)
            self.start_services(run_dir)
            token = self.login()
            tasks = self.submit_workload(run_id, token, run_dir)
            result["tasks"] = {task["task_id"]: task for task in tasks}
            duplicate_ok = self.submit_duplicate(tasks[0]["task_id"], token, run_dir)
            if duplicate_ok:
                result["failures"].append("duplicate submission was accepted")
            self.run_failure_simulations(token, run_dir, result)
            observed = self.wait_for_tasks(tasks, token, run_dir)
            result["observed_tasks"] = observed
            result["workers_final"] = self.get_workers(token)
            self.evaluate_run(tasks, observed, result)
            result["passed"] = not result["failures"]
            result["finished_at"] = now_iso()
            return result
        except Exception as exc:
            result["failures"].append(f"harness exception: {exc}")
            result["finished_at"] = now_iso()
            return result
        finally:
            (run_dir / "run-result.json").write_text(json.dumps(result, indent=2, sort_keys=True), encoding="utf-8")
            self.cleanup_runtime()

    def start_dependencies(self, run_dir: Path) -> None:
        for name in ("hivemind-rel-pg", "hivemind-rel-redis"):
            run_cmd(["docker", "rm", "-f", name], timeout=60)
        pg = run_cmd([
            "docker", "run", "-d", "--name", "hivemind-rel-pg",
            "-e", "POSTGRES_USER=hivemind", "-e", "POSTGRES_PASSWORD=hivemind", "-e", "POSTGRES_DB=hivemind",
            "-p", "25432:5432", "postgres:16-alpine",
        ], timeout=120)
        redis = run_cmd(["docker", "run", "-d", "--name", "hivemind-rel-redis", "-p", "26379:6379", "redis:7-alpine"], timeout=120)
        (run_dir / "docker-start.log").write_text(pg.stdout + pg.stderr + redis.stdout + redis.stderr, encoding="utf-8")
        if pg.returncode != 0 or redis.returncode != 0:
            raise RuntimeError("docker dependency start failed")
        deadline = time.monotonic() + 60
        while time.monotonic() < deadline:
            ready = run_cmd(["docker", "exec", "hivemind-rel-pg", "pg_isready", "-U", "hivemind", "-d", "hivemind"], timeout=5)
            query = run_cmd(["docker", "exec", "-e", "PGPASSWORD=hivemind", "hivemind-rel-pg", "psql", "-U", "hivemind", "-d", "hivemind", "-c", "SELECT 1"], timeout=5)
            if ready.returncode == 0 and query.returncode == 0:
                break
            time.sleep(1)
        else:
            raise RuntimeError("postgres did not become ready")
        ping = run_cmd(["docker", "exec", "hivemind-rel-redis", "redis-cli", "ping"], timeout=5)
        if "PONG" not in ping.stdout:
            raise RuntimeError("redis did not become ready")

    def start_proxy(self, name: str, listen_port: int, target_port: int, run_dir: Path) -> None:
        proxy = DelayProxy(name, listen_port, target_port, self.args.latency_ms, self.args.jitter_ms, run_dir / "latency-proxy.log")
        proxy.start()
        self.proxies.append(proxy)

    def start_services(self, run_dir: Path) -> None:
        nodepool_env = {
            "NODEPOOL_POSTGRES_DSN": "postgres://hivemind:hivemind@127.0.0.1:25432/hivemind?sslmode=disable",
            "NODEPOOL_REDIS_ADDR": "127.0.0.1:26379",
            "NODEPOOL_ENABLE_HTTP_AUTH": "1",
            "NODEPOOL_HTTP_ADDR": ":18081",
            "NODEPOOL_ADDR": ":50051",
            "NODEPOOL_LOG_FILE": str(run_dir / "nodepool-events.log"),
            "NODEPOOL_TASK_TIMEOUT_SEC": str(self.args.task_timeout_seconds),
            "NODEPOOL_TASK_MAX_REDISPATCH": "3",
        }
        self.start_process("nodepool", self.bin_dir / exe_name("nodepool"), ROOT / "services" / "nodepool" / "cmd" / "server", run_dir, nodepool_env)
        wait_port("127.0.0.1", 50051, 30)
        wait_port("127.0.0.1", 18081, 30)
        self.start_proxy("nodepool-grpc", 55051, 50051, run_dir)
        try:
            http_json("POST", "127.0.0.1", 18081, "/api/register", {"username": "worker3", "password": "worker123"})
        except Exception as exc:
            append_log(run_dir / "harness-warnings.log", f"register worker3 user warning: {exc}")

        master_env = {
            "MASTER_HTTP_ADDR": ":18082",
            "NODEPOOL_GRPC_ADDR": "127.0.0.1:55051",
            "NODEPOOL_HTTP_BASE": "http://127.0.0.1:18081",
            "BT_PUBLIC_BASE_URL": "http://127.0.0.1:18082",
        }
        self.start_process("master", self.bin_dir / exe_name("master"), ROOT / "services" / "master" / "cmd" / "server", run_dir, master_env)
        wait_port("127.0.0.1", 18082, 30)

        executor = f"{sys.executable} {ROOT / 'scripts' / 'reliability_executor.py'}"
        common = {
            "NODEPOOL_ADDR": self.args.worker_nodepool_addr,
            "WORKER_PASSWORD": "worker123",
            "WORKER_AUTO_REGISTER": "1",
            "WORKER_REGISTER_TIMEOUT_SEC": "15",
            "WORKER_REGISTER_RETRY_INTERVAL_SEC": "2",
            "WORKER_EXECUTOR_CMD": executor,
            "WORKER_EXECUTOR_TIMEOUT_SEC": str(self.args.long_seconds + 600),
            "WORKER_USAGE_REPORT_INTERVAL_SEC": "2",
            "RELIABILITY_CPU_SECONDS": str(self.args.cpu_seconds),
            "RELIABILITY_IO_SECONDS": str(self.args.io_seconds),
            "RELIABILITY_FAILURE_SECONDS": str(self.args.failure_seconds),
            "RELIABILITY_RETRY_SECONDS": str(self.args.retry_seconds),
            "RELIABILITY_LONG_SECONDS": str(self.args.long_seconds),
            "RELIABILITY_PARALLEL_SECONDS": str(self.args.parallel_seconds),
        }
        workers = [
            ("worker1", 51053, 50053, 18080),
            ("worker2", 51054, 50054, 18083),
            ("worker3", 51055, 50055, 18084),
        ]
        for username, grpc_port, public_port, control_port in workers:
            env = dict(common)
            env.update({
                "WORKER_USERNAME": username,
                "WORKER_ADDR": f":{grpc_port}",
                "WORKER_PUBLIC_ADDR": f"127.0.0.1:{public_port}",
                "WORKER_CONTROL_ADDR": f":{control_port}",
            })
            self.start_process(username, self.bin_dir / exe_name("worker"), ROOT / "services" / "worker" / "cmd" / "server", run_dir, env)
            wait_port("127.0.0.1", grpc_port, 30)
            wait_port("127.0.0.1", control_port, 30)
            self.start_proxy(f"{username}-grpc", public_port, grpc_port, run_dir)
        time.sleep(3)

    def start_process(self, name: str, exe: Path, cwd: Path, run_dir: Path, extra_env: dict[str, str]) -> None:
        env = os.environ.copy()
        env.update(extra_env)
        stdout = run_dir / f"{name}.out.log"
        stderr = run_dir / f"{name}.err.log"
        out_fh = stdout.open("w", encoding="utf-8")
        err_fh = stderr.open("w", encoding="utf-8")
        creationflags = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0
        proc = subprocess.Popen([str(exe)], cwd=str(cwd), env=env, stdout=out_fh, stderr=err_fh, text=True, creationflags=creationflags)
        self.processes[name] = ManagedProcess(name, proc, stdout, stderr, extra_env)

    def login(self) -> str:
        resp = http_json("POST", "127.0.0.1", 18082, "/api/login", {"username": "worker1", "password": "worker123"})
        if not resp.get("success") or not resp.get("token"):
            raise RuntimeError(f"login failed: {resp}")
        return str(resp["token"])

    def submit_workload(self, run_id: str, token: str, run_dir: Path) -> list[dict[str, Any]]:
        specs = [
            ("cpu", 1),
            ("io", 1),
            ("failure", 1),
            ("retry", 1),
            ("long", 1),
        ]
        specs.extend((f"parallel-{i}", 1) for i in range(self.args.parallel_tasks))
        tasks = []
        for kind, host_count in specs:
            task_id = f"{run_id}-{kind}"
            btih = hashlib.sha1(f"{task_id}|{kind}".encode("utf-8")).hexdigest()
            body = {
                "task_id": task_id,
                "torrent": f"magnet:?xt=urn:btih:{btih}&dn={kind}",
                "memory_gb": 1,
                "gpu_memory_gb": 0,
                "host_count": host_count,
                "max_cpt": 0,
            }
            resp = http_json("POST", "127.0.0.1", 18082, "/api/upload-task", body, token=token)
            task = {"kind": kind, "task_id": task_id, "btih": btih, "upload": resp, "expected_status": "FAILED" if kind == "failure" else "COMPLETED"}
            tasks.append(task)
        (run_dir / "submitted-tasks.json").write_text(json.dumps(tasks, indent=2), encoding="utf-8")
        return tasks

    def submit_duplicate(self, task_id: str, token: str, run_dir: Path) -> bool:
        btih = hashlib.sha1(f"{task_id}|duplicate".encode("utf-8")).hexdigest()
        resp = http_json("POST", "127.0.0.1", 18082, "/api/upload-task", {
            "task_id": task_id,
            "torrent": f"magnet:?xt=urn:btih:{btih}&dn=duplicate",
            "memory_gb": 1,
            "gpu_memory_gb": 0,
            "host_count": 1,
            "max_cpt": 0,
        }, token=token)
        (run_dir / "duplicate-response.json").write_text(json.dumps(resp, indent=2), encoding="utf-8")
        return bool(resp.get("success"))

    def run_failure_simulations(self, token: str, run_dir: Path, result: dict[str, Any]) -> None:
        for index in range(self.args.failure_simulations):
            delay = self.rng.uniform(self.args.kill_min_seconds, self.args.kill_max_seconds)
            time.sleep(delay)
            workers = [name for name in ("worker1", "worker2", "worker3") if name in self.processes and self.processes[name].is_alive()]
            if not workers:
                result["failures"].append("no live worker available to kill")
                return
            victim = self.rng.choice(workers)
            pid = self.processes[victim].pid
            self.processes[victim].kill()
            result["events"].append({"event": "worker_killed", "worker": victim, "pid": pid, "at": now_iso(), "delay_sec": delay})
            reconnect_delay = self.rng.uniform(self.args.reconnect_min_seconds, self.args.reconnect_max_seconds)
            time.sleep(reconnect_delay)
            self.restart_worker(victim, run_dir)
            result["events"].append({"event": "worker_restarted", "worker": victim, "pid": self.processes[victim].pid, "at": now_iso(), "delay_sec": reconnect_delay})
            time.sleep(2)
            (run_dir / f"workers-after-failure-{index + 1}.json").write_text(json.dumps(self.get_workers(token), indent=2), encoding="utf-8")

    def restart_worker(self, name: str, run_dir: Path) -> None:
        old = self.processes.pop(name, None)
        if old is not None:
            old.stop()
        worker_index = int(name[-1])
        grpc_port = 51052 + worker_index
        public_port = 50052 + worker_index
        control_port = {1: 18080, 2: 18083, 3: 18084}[worker_index]
        executor = f"{sys.executable} {ROOT / 'scripts' / 'reliability_executor.py'}"
        env = {
            "NODEPOOL_ADDR": self.args.worker_nodepool_addr,
            "WORKER_PASSWORD": "worker123",
            "WORKER_AUTO_REGISTER": "1",
            "WORKER_REGISTER_TIMEOUT_SEC": "15",
            "WORKER_REGISTER_RETRY_INTERVAL_SEC": "2",
            "WORKER_EXECUTOR_CMD": executor,
            "WORKER_EXECUTOR_TIMEOUT_SEC": str(self.args.long_seconds + 600),
            "WORKER_USAGE_REPORT_INTERVAL_SEC": "2",
            "WORKER_USERNAME": name,
            "WORKER_ADDR": f":{grpc_port}",
            "WORKER_PUBLIC_ADDR": f"127.0.0.1:{public_port}",
            "WORKER_CONTROL_ADDR": f":{control_port}",
            "RELIABILITY_CPU_SECONDS": str(self.args.cpu_seconds),
            "RELIABILITY_IO_SECONDS": str(self.args.io_seconds),
            "RELIABILITY_FAILURE_SECONDS": str(self.args.failure_seconds),
            "RELIABILITY_RETRY_SECONDS": str(self.args.retry_seconds),
            "RELIABILITY_LONG_SECONDS": str(self.args.long_seconds),
            "RELIABILITY_PARALLEL_SECONDS": str(self.args.parallel_seconds),
        }
        self.start_process(name, self.bin_dir / exe_name("worker"), ROOT / "services" / "worker" / "cmd" / "server", run_dir, env)
        wait_port("127.0.0.1", grpc_port, 30)

    def wait_for_tasks(self, tasks: list[dict[str, Any]], token: str, run_dir: Path) -> dict[str, Any]:
        deadline = time.monotonic() + self.args.task_wait_timeout_seconds
        task_ids = {t["task_id"] for t in tasks}
        latest: dict[str, Any] = {}
        while time.monotonic() < deadline:
            resp = http_json("GET", "127.0.0.1", 18082, "/api/tasks?limit=200&sort_by=updated_at&order=desc", token=token)
            latest = {item["TaskID"]: item for item in resp.get("tasks", []) if item.get("TaskID") in task_ids}
            (run_dir / "tasks-latest.json").write_text(json.dumps(resp, indent=2), encoding="utf-8")
            terminal = 0
            for task in tasks:
                item = latest.get(task["task_id"])
                if item and item.get("Status") in ("COMPLETED", "FAILED", "STOPPED"):
                    terminal += 1
            if terminal == len(tasks):
                break
            time.sleep(5)
        for task in tasks:
            task_id = task["task_id"]
            try:
                log = http_json("GET", "127.0.0.1", 18082, f"/api/task/{task_id}/log", token=token)
                (run_dir / f"{task_id}-log.json").write_text(json.dumps(log, indent=2), encoding="utf-8")
            except Exception as exc:
                append_log(run_dir / "artifact-fetch-errors.log", f"{task_id} log fetch failed: {exc}")
            try:
                res = http_json("GET", "127.0.0.1", 18082, f"/api/task/{task_id}/result", token=token)
                (run_dir / f"{task_id}-result.json").write_text(json.dumps(res, indent=2), encoding="utf-8")
            except Exception as exc:
                append_log(run_dir / "artifact-fetch-errors.log", f"{task_id} result fetch failed: {exc}")
        return latest

    def get_workers(self, token: str) -> Any:
        return http_json("GET", "127.0.0.1", 18082, "/api/workers?include_offline=true", token=token)

    def evaluate_run(self, tasks: list[dict[str, Any]], observed: dict[str, Any], result: dict[str, Any]) -> None:
        for task in tasks:
            item = observed.get(task["task_id"])
            if not item:
                result["failures"].append(f"lost task: {task['task_id']}")
                continue
            status = item.get("Status")
            if status != task["expected_status"]:
                result["failures"].append(f"task {task['task_id']} expected {task['expected_status']} got {status}: {item.get('StatusMessage')}")
            if task["expected_status"] == "COMPLETED" and item.get("ResultTorrent", "").find(task["btih"]) < 0:
                result["failures"].append(f"task {task['task_id']} result btih mismatch")
        non_terminal = [item["TaskID"] for item in observed.values() if item.get("Status") in ("PENDING", "DISPATCHED", "RUNNING")]
        if non_terminal:
            result["failures"].append(f"stuck non-terminal tasks: {non_terminal}")
        workers = result.get("workers_final", {}).get("workers", [])
        ids = [w.get("id") for w in workers]
        if sorted(ids) != ["worker1", "worker2", "worker3"]:
            result["failures"].append(f"worker leak or missing worker: {ids}")
        inactive = [w for w in workers if w.get("status") != "ACTIVE"]
        if inactive:
            result["failures"].append(f"zombie/offline workers after reconnect: {inactive}")

    def cleanup_runtime(self) -> None:
        for proc in list(self.processes.values()):
            proc.stop()
        self.processes.clear()
        for proxy in self.proxies:
            proxy.stop()
        self.proxies.clear()
        run_cmd(["docker", "rm", "-f", "hivemind-rel-pg", "hivemind-rel-redis"], timeout=60)

    def cleanup(self) -> None:
        self.cleanup_runtime()
        self.summary["artifacts"] = str(self.run_root)
        (self.run_root / "summary.json").write_text(json.dumps(self.summary, indent=2, sort_keys=True), encoding="utf-8")
        self.write_reports()

    def write_reports(self) -> None:
        report = render_reliability_report(self.summary)
        matrix = render_failure_matrix(self.summary)
        flaky = render_flaky_behavior(self.summary)
        (ROOT / "reliability_report.md").write_text(report, encoding="utf-8")
        (ROOT / "failure_matrix.md").write_text(matrix, encoding="utf-8")
        (ROOT / "flaky_behavior.md").write_text(flaky, encoding="utf-8")


def exe_name(base: str) -> str:
    return f"{base}.exe" if os.name == "nt" else base


def render_reliability_report(summary: dict[str, Any]) -> str:
    params = summary.get("parameters", {})
    runs = summary.get("runs", [])
    lines = [
        "# Reliability Report",
        "",
        f"- Generated: {now_iso()}",
        f"- Artifacts: `{summary.get('artifacts', '')}`",
        f"- Runs requested: {params.get('runs')}",
        f"- Runs completed: {len(runs)}",
        f"- Long-running seconds configured: {params.get('long_seconds')}",
        f"- Network latency/jitter: {params.get('latency_ms')}ms + 0..{params.get('jitter_ms')}ms",
        f"- Overall pass: {summary.get('passed', False)}",
        f"- DoD satisfied: {summary.get('dod_satisfied', False)}",
        "",
        "## Run Results",
    ]
    if not runs:
        lines.append("- No reliability runs have completed yet.")
    for run in runs:
        lines.append(f"- Run {run.get('run')} `{run.get('run_id')}`: passed={run.get('passed')} failures={len(run.get('failures', []))}")
    lines.extend(["", "## DoD Gate Status"])
    dod_items = [
        ("10 consecutive successful full pipeline runs", bool(summary.get("passed")) and params.get("runs", 0) >= 10),
        ("3 consecutive node failure simulations", bool(summary.get("passed")) and params.get("failure_simulations", 0) >= 3),
        ("true network delay simulation", params.get("latency_ms", 0) > 0 or params.get("jitter_ms", 0) > 0),
        ("long-running workload >= 15 min", params.get("long_seconds", 0) >= 900),
        ("failure-injected workload", any("failure" in str(run.get("observed_tasks", {})).lower() for run in runs)),
        ("no task duplication", bool(summary.get("passed"))),
        ("no leaked workers", bool(summary.get("passed"))),
        ("no stuck task ownership", bool(summary.get("passed"))),
        ("no zombie reconnect state", bool(summary.get("passed"))),
    ]
    for item, ok in dod_items:
        lines.append(f"- [{'x' if ok else ' '}] {item}")
    return "\n".join(lines) + "\n"


def render_failure_matrix(summary: dict[str, Any]) -> str:
    lines = ["# Failure Matrix", "", "| Run | Scenario | Result | Evidence |", "| --- | --- | --- | --- |"]
    for run in summary.get("runs", []):
        reached_submission = bool(run.get("tasks"))
        observed = run.get("observed_tasks", {})
        events = run.get("events", [])
        duplicate_failed = any("duplicate" in f.lower() for f in run.get("failures", []))
        failure_task_seen = any("failure" in task_id.lower() for task_id in observed)
        long_task_seen = any("long" in task_id.lower() for task_id in observed)
        parallel_seen = any("parallel" in task_id.lower() for task_id in observed)
        scenarios = [
            ("network latency/jitter", "configured" if summary.get("parameters", {}).get("latency_ms", 0) > 0 else "not configured"),
            ("worker kill/reconnect", "events=" + str(len(events)) if events else "not reached"),
            ("duplicate submission", "rejected" if reached_submission and not duplicate_failed else ("failed" if duplicate_failed else "not reached")),
            ("failure-injected task", "checked" if failure_task_seen else "not reached"),
            ("long-running task", "checked" if long_task_seen else "not reached"),
            ("parallel workload", "checked" if parallel_seen else "not reached"),
        ]
        for scenario, status in scenarios:
            evidence = f"`test_logs/reliability/.../{run.get('run_id')}`"
            lines.append(f"| {run.get('run')} | {scenario} | {status} | {evidence} |")
    if len(lines) == 3:
        lines.append("| n/a | n/a | no runs yet | n/a |")
    return "\n".join(lines) + "\n"


def render_flaky_behavior(summary: dict[str, Any]) -> str:
    lines = ["# Flaky Behavior", ""]
    regressions = summary.get("regressions", [])
    if not regressions:
        lines.append("- No regressions recorded yet." if summary.get("runs") else "- No reliability run evidence recorded yet.")
    for regression in regressions:
        lines.append(f"- Run {regression.get('run')}: {regression.get('failures')}")
    return "\n".join(lines) + "\n"


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--runs", type=int, default=10)
    parser.add_argument("--failure-simulations", type=int, default=3)
    parser.add_argument("--long-seconds", type=int, default=900)
    parser.add_argument("--retry-seconds", type=int, default=90)
    parser.add_argument("--cpu-seconds", type=int, default=5)
    parser.add_argument("--io-seconds", type=int, default=5)
    parser.add_argument("--failure-seconds", type=int, default=3)
    parser.add_argument("--parallel-seconds", type=int, default=10)
    parser.add_argument("--parallel-tasks", type=int, default=6)
    parser.add_argument("--latency-ms", type=int, default=100)
    parser.add_argument("--jitter-ms", type=int, default=250)
    parser.add_argument("--worker-nodepool-addr", default="127.0.0.1:55051")
    parser.add_argument("--task-timeout-seconds", type=int, default=20)
    parser.add_argument("--task-wait-timeout-seconds", type=int, default=1500)
    parser.add_argument("--kill-min-seconds", type=int, default=5)
    parser.add_argument("--kill-max-seconds", type=int, default=30)
    parser.add_argument("--reconnect-min-seconds", type=int, default=5)
    parser.add_argument("--reconnect-max-seconds", type=int, default=30)
    parser.add_argument("--seed", type=int, default=20260520)
    parser.add_argument("--stop-on-failure", action="store_true")
    parser.add_argument("--calibration", action="store_true", help="Run a short non-DoD calibration pass.")
    args = parser.parse_args(argv)
    if args.calibration:
        args.runs = 1
        args.failure_simulations = 1
        args.long_seconds = min(args.long_seconds, 30)
        args.retry_seconds = min(args.retry_seconds, 35)
        args.task_timeout_seconds = min(args.task_timeout_seconds, 10)
        args.task_wait_timeout_seconds = min(args.task_wait_timeout_seconds, 240)
        args.parallel_tasks = min(args.parallel_tasks, 3)
    return args


def main(argv: list[str]) -> int:
    if shutil.which("docker") is None:
        print("docker is required", file=sys.stderr)
        return 2
    args = parse_args(argv)
    harness = ReliabilityHarness(args)
    return harness.run()


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
