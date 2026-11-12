"""Run phase-3 pipelines on one local Nodepool, Master, and three native Workers.

Requires a default-feature hivemind-bin build, Docker, OpenSSL, and Python 3.10+.
Only newly created, loopback-published PostgreSQL/Redis containers are modified.
All five application processes run on this host, using three distinct test owners.
"""

import argparse
import datetime
import hashlib
import ipaddress
import json
import math
import os
from pathlib import Path
import secrets
import shutil
import socket
import struct
import subprocess
import tempfile
import time
import urllib.error
import urllib.request
import uuid


REPO = Path(__file__).resolve().parent.parent
SERVICE_PREFIXES = (
    "HIVEMIND_", "WORKER_", "MASTER_", "NODEPOOL_", "DATABASE_", "REDIS_",
    "JWT_", "TORRENT_", "EXECUTOR_", "VPN_", "HEADSCALE_", "TAILSCALE_",
    "MANAGED_", "UPDATE_", "WEBSITE_", "ACCOUNT_", "SMTP_", "AUTH_",
)


def child_environment(source=None):
    """Preserve OS/tool paths, but never inherit an account or service target."""
    source = os.environ if source is None else source
    clean = {key: value for key, value in source.items()
             if not key.upper().startswith(SERVICE_PREFIXES)
             and key.upper() not in {"HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "NO_PROXY"}}
    clean["NO_PROXY"] = "*"
    return clean


def write_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, allow_nan=False) + "\n", encoding="utf-8")


def write_fixtures(directory):
    ordinary = directory / "ordinary"
    ordinary.mkdir(parents=True, exist_ok=True)
    (ordinary / "add.hcode").write_text(
        'return get(input, "previous")[0] + get(get(input, "params"), "add");\n', encoding="utf-8")
    (ordinary / "sum.hcode").write_text(
        'return get(input, "previous")[0] + get(input, "previous")[1];\n', encoding="utf-8")
    write_json(ordinary / "pipeline.json", {
        "input": 5,
        "stages": [{"items": [{"source": "add.hcode", "params": {"add": 1}},
                               {"source": "add.hcode", "params": {"add": 2}}]},
                   {"items": [{"source": "sum.hcode"}]}],
    })
    model = directory / "model"
    model.mkdir(parents=True, exist_ok=True)
    first = [struct.unpack("<f", struct.pack("<f", (n - 63.0) / 80.0))[0] for n in range(128)]
    first_bias = [(n - 8.0) / 20.0 for n in range(16)]
    second = [(n - 23.0) / 40.0 for n in range(48)]
    second_bias = [0.1, -0.2, 0.3]
    tensors = [
        ("0.weight", "F32", [16, 8], first), ("0.bias", "F64", [16], first_bias),
        ("2.weight", "F64", [3, 16], second), ("2.bias", "F64", [3], second_bias),
    ]
    header, payload = {}, bytearray()
    for name, dtype, shape, values in tensors:
        start = len(payload)
        for value in values:
            payload.extend(struct.pack("<f" if dtype == "F32" else "<d", value))
        header[name] = {"dtype": dtype, "shape": shape, "data_offsets": [start, len(payload)]}
    encoded = json.dumps(header, separators=(",", ":")).encode("utf-8")
    encoded += b" " * ((-len(encoded)) % 8)
    weights = model / "weights.safetensors"
    weights.write_bytes(struct.pack("<Q", len(encoded)) + encoded + payload)
    samples = [[0.2, -0.1, 0.3, 0.7, 0.0, 0.4, -0.6, 0.9],
               [0.0, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7]]
    write_json(model / "input.json", samples)
    reference = []
    for sample in samples:
        hidden = [max(0.0, sum(first[row * 8 + col] * sample[col] for col in range(8)) + first_bias[row])
                  for row in range(16)]
        logits = [sum(second[row * 16 + col] * hidden[col] for col in range(16)) + second_bias[row]
                  for row in range(3)]
        largest = max(logits)
        exps = [math.exp(value - largest) for value in logits]
        denominator = sum(exps)
        reference.append([value / denominator for value in exps])
    write_json(model / "reference.json", reference)
    return {"ordinary": ordinary / "pipeline.json", "weights": weights,
            "input": model / "input.json", "reference": reference}


def summarize_run(pipeline, tasks, prefix, timings, elapsed):
    lookup = {task["task_id"]: task for task in tasks if task["task_id"].startswith(prefix + "-")}
    rows, stages = [], []
    final_output = []
    for stage_index, stage in enumerate(pipeline["stages"], 1):
        stage_rows = []
        final_output = []
        for item_index, _ in enumerate(stage["items"], 1):
            task_id = f"{prefix}-{stage_index}-{item_index}"
            if task_id not in lookup:
                raise ValueError(f"missing task {task_id}")
            task = lookup[task_id]
            if task.get("status") != "COMPLETED" or task.get("billing_settled") is not True:
                raise ValueError(f"task {task_id} is not completed and settled")
            for key in ("usage_units", "billed_amount"):
                if not isinstance(task.get(key), int) or task[key] <= 0:
                    raise ValueError(f"task {task_id} has no positive measured {key}")
            consensus = task.get("managed_consensus") or {}
            if (consensus.get("replica_count") != 3 or consensus.get("required_quorum") != 2
                    or consensus.get("matching_votes", 0) < 2 or not consensus.get("settlement_authorized")):
                raise ValueError(f"task {task_id} lacks settled three-replica consensus evidence")
            timing = timings.get(task_id) or {}
            if timing.get("latency_ms") is None or not timing.get("completed_at"):
                raise ValueError(f"task {task_id} has no Nodepool completion timing")
            row = {"stage": stage_index, "item": item_index, "task_id": task_id,
                   "usage_units": task["usage_units"], "billed_cpt": task["billed_amount"],
                   "max_cpt": task.get("max_cpt"), "worker_wall_time_ms": task.get("wall_time_ms"),
                   "consensus": consensus, **timing}
            stage_rows.append(row)
            rows.append(row)
            final_output.append(json.loads(task["output"]))
        stages.append({"stage": stage_index, "items": len(stage_rows),
                       "usage_units": sum(row["usage_units"] for row in stage_rows),
                       "billed_cpt": sum(row["billed_cpt"] for row in stage_rows),
                       "max_task_latency_ms": max(row["latency_ms"] for row in stage_rows)})
    if len(rows) != len(lookup):
        raise ValueError(f"unexpected extra/retried tasks for {prefix}")
    return {"elapsed_seconds": elapsed, "tasks": rows, "stages": stages,
            "usage_units": sum(row["usage_units"] for row in rows),
            "billed_cpt": sum(row["billed_cpt"] for row in rows), "final_output": final_output}


def unused_ports(count):
    # Reserve all candidates together so one OS-selected port cannot be reused.
    sockets = []
    try:
        for _ in range(count):
            server = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
            server.bind(("0.0.0.0", 0))
            sockets.append(server)
        return [server.getsockname()[1] for server in sockets]
    finally:
        for server in sockets:
            server.close()


def local_worker_host():
    # UDP connect selects a local route without sending a datagram.
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as route:
        route.connect(("192.0.2.1", 9))
        return route.getsockname()[0]


def http_json(url, body=None, token=None):
    data = None if body is None else json.dumps(body).encode("utf-8")
    headers = {"Content-Type": "application/json"}
    if token:
        headers["Authorization"] = "Bearer " + token
    request = urllib.request.Request(url, data=data, headers=headers)
    # All calls are to this runner's loopback Master; never inherit a proxy.
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    with opener.open(request, timeout=10) as response:
        return json.load(response)


class LocalStack:
    def __init__(self, binary, output, timeout, worker_host):
        self.binary = binary.resolve()
        self.output = output.resolve()
        self.timeout = timeout
        self.worker_host = worker_host
        self.run_id = uuid.uuid4().hex[:12]
        self.pg_name = f"hm-phase3-{self.run_id}-pg"
        self.redis_name = f"hm-phase3-{self.run_id}-redis"
        self.containers, self.processes, self.logs = [], [], []
        self.pg_password = secrets.token_hex(24)
        self.account_password = secrets.token_hex(24)
        self.jwt_secret = secrets.token_hex(32)
        self.base_env = child_environment()
        self.root = None
        self.master_url = None
        self.token = None

    def scrub(self, value):
        for secret in (self.pg_password, self.account_password, self.jwt_secret, self.token):
            if secret:
                value = value.replace(secret, "<redacted>")
        return value

    def command(self, args, *, env=None, input_text=None, label=None, timeout=120):
        result = subprocess.run([str(arg) for arg in args], env=env or self.base_env,
                                cwd=self.root, input=input_text, text=True, encoding="utf-8",
                                errors="replace", capture_output=True, timeout=timeout,
                                creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0)
        if label:
            (self.output / f"{label}.stdout.log").write_text(self.scrub(result.stdout), encoding="utf-8")
            (self.output / f"{label}.stderr.log").write_text(self.scrub(result.stderr), encoding="utf-8")
        if result.returncode:
            raise RuntimeError(self.scrub(f"{label or Path(str(args[0])).name} exited {result.returncode}: {result.stderr[-4000:]}"))
        return result.stdout.strip()

    def sql(self, sql):
        return self.command(["docker", "exec", "-i", self.pg_name, "psql", "-U", "phase3",
                             "-d", "phase3", "-X", "-A", "-t", "-v", "ON_ERROR_STOP=1"], input_text=sql)

    def wait_for(self, description, condition):
        deadline = time.monotonic() + self.timeout
        last = "not ready"
        while time.monotonic() < deadline:
            for name, process in self.processes:
                if process.poll() is not None:
                    raise RuntimeError(f"{name} exited {process.returncode}; inspect {self.output / (name + '.stderr.log')}")
            try:
                value = condition()
                if value:
                    return value
            except (RuntimeError, OSError, ValueError, urllib.error.URLError) as error:
                last = self.scrub(str(error))
            time.sleep(0.5)
        raise RuntimeError(f"timeout waiting for {description}: {last}")

    def start_process(self, name, role, extra):
        work = self.root / name
        work.mkdir()
        env = {**self.common_env, **extra, "HIVEMIND_DISABLE_OPEN_UI": "1",
               "TORRENT_API_DIR": str(work / "api"),
               "TORRENT_BT_DIR": str(work / "bt"), "EXECUTOR_SANDBOX_DIR": str(work / "sandbox")}
        stdout = (self.output / f"{name}.stdout.log").open("w", encoding="utf-8")
        stderr = (self.output / f"{name}.stderr.log").open("w", encoding="utf-8")
        self.logs.extend([stdout, stderr])
        process = subprocess.Popen([str(self.binary), role], cwd=work, env=env, stdout=stdout, stderr=stderr,
                                   creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0)
        self.processes.append((name, process))
        self.save_resources()

    def save_resources(self, cleanup_errors=None):
        write_json(self.output / "resources.json", {
            "run_id": self.run_id,
            "temporary_workspace": str(self.root) if self.root else None,
            "container_ids": self.containers,
            "processes": [{"role": name, "pid": process.pid, "exit_code": process.poll()}
                          for name, process in self.processes],
            "cleanup_errors": cleanup_errors,
        })

    def start(self):
        self.output.mkdir(parents=True, exist_ok=True)
        self.root = Path(tempfile.mkdtemp(prefix=f"hm-phase3-{self.run_id}-"))
        # The full binary searches parent directories for .env; stop that search
        # inside this run even if the user's temporary directory has a .env.
        (self.root / ".env").write_text("", encoding="utf-8")
        ports = unused_ports(12)
        pg_port, redis_port, nodepool_port, master_port, tracker_port, seed_port = ports[:6]
        worker_ports, control_ports = ports[6:9], ports[9:12]
        nodepool_addr = f"127.0.0.1:{nodepool_port}"
        self.master_url = f"http://127.0.0.1:{master_port}"
        docker_env = {**self.base_env, "POSTGRES_PASSWORD": self.pg_password}
        pg_id = self.command(["docker", "run", "--detach", "--name", self.pg_name,
                              "--publish", f"127.0.0.1:{pg_port}:5432", "--env", "POSTGRES_DB=phase3",
                              "--env", "POSTGRES_USER=phase3", "--env", "POSTGRES_PASSWORD", "postgres:16-alpine"], env=docker_env)
        self.containers.append(pg_id)
        self.save_resources()
        redis_id = self.command(["docker", "run", "--detach", "--name", self.redis_name,
                                 "--publish", f"127.0.0.1:{redis_port}:6379", "redis:7-alpine"])
        self.containers.append(redis_id)
        self.save_resources()
        self.wait_for("isolated PostgreSQL", lambda: self.sql("SELECT 1;") == "1")
        self.wait_for("isolated Redis", lambda: self.command(["docker", "exec", self.redis_name, "redis-cli", "PING"]) == "PONG")
        private_key = self.root / "execution-private.pem"
        public_key = self.root / "execution-public.pem"
        self.command(["openssl", "genpkey", "-algorithm", "ED25519", "-out", private_key])
        self.command(["openssl", "pkey", "-in", private_key, "-pubout", "-out", public_key])
        self.common_env = {
            **self.base_env,
            "DATABASE_URL": f"postgres://phase3:{self.pg_password}@127.0.0.1:{pg_port}/phase3",
            "REDIS_URL": f"redis://127.0.0.1:{redis_port}/0", "JWT_SECRET": self.jwt_secret,
            "NODEPOOL_GRPC_ADDR": nodepool_addr, "NODEPOOL_GRPC_ENDPOINT": nodepool_addr,
            "HIVEMIND_DISABLE_WEBSITE_VPN": "1", "UPDATE_ENABLED": "false",
            "WEBSITE_API_BASE": self.master_url, "HIVEMIND_SEED_DEFAULT_USER": "false",
            "HIVEMIND_WORKER_ADMISSION_MODE": "public_dynamic", "HIVEMIND_ADMIN_USERS": "phase3-requester",
            "MANAGED_CONSENSUS_ROLLOUT_MODE": "enforce", "MANAGED_CONSENSUS_REPLICA_COUNT": "3",
            "MANAGED_CONSENSUS_QUORUM": "2", "MANAGED_CONSENSUS_REQUIRE_DISTINCT_OWNERS": "true",
            "MANAGED_CONSENSUS_TIMEOUT_SECS": "60", "LOG_LEVEL": "info",
            "TORRENT_ALLOW_LOCAL_TASK_ARTIFACTS": "false",
            "TORRENT_TRACKER_LISTEN_ADDR": f"127.0.0.1:{tracker_port}",
            "TORRENT_ANNOUNCE_URL": f"http://127.0.0.1:{tracker_port}/announce",
            "TORRENT_SEED_LISTEN_ADDR": f"127.0.0.1:{seed_port}",
            "TORRENT_SEED_ADVERTISE_HOST": "127.0.0.1",
            "WORKER_EXECUTION_PUBLIC_KEY_PEM": public_key.read_text(encoding="utf-8"),
        }
        self.start_process("nodepool", "nodepool", {
            "WORKER_EXECUTION_PRIVATE_KEY_PEM": private_key.read_text(encoding="utf-8")})
        self.wait_for("Nodepool migrations", lambda: self.sql("SELECT count(*) FROM users;").isdigit())
        usernames = ["phase3-requester"] + [f"phase3-provider-{index}" for index in range(1, 4)]
        self.sql("CREATE EXTENSION IF NOT EXISTS pgcrypto;\n" + "\n".join(
            f"INSERT INTO users (username, password_hash, balance, is_active) VALUES ('{username}', "
            f"crypt('{self.account_password}', gen_salt('bf', 10)), 1000000000, true);"
            for username in usernames))
        self.start_process("master", "master", {"MASTER_HTTP_ADDR": f"127.0.0.1:{master_port}",
                                                "TORRENT_TASK_ARTIFACT_BASE_URL": self.master_url + "/task-artifacts"})
        login = self.wait_for("Master login", lambda: http_json(self.master_url + "/api/login", {
            "username": "phase3-requester", "password": self.account_password}))
        if not login.get("success") or not login.get("token"):
            raise RuntimeError("local Master login did not return a session token")
        self.token = login["token"]
        for index in range(3):
            self.start_process(f"worker-{index + 1}", "worker", {
                "WORKER_NODEPOOL_USERNAME": f"phase3-provider-{index + 1}",
                "WORKER_NODEPOOL_PASSWORD": self.account_password,
                "WORKER_ID": f"phase3-worker-{index + 1}",
                "WORKER_GRPC_ADDR": f"0.0.0.0:{worker_ports[index]}",
                "WORKER_ADVERTISE_ADDR": f"{self.worker_host}:{worker_ports[index]}",
                "WORKER_CONTROL_HTTP_ADDR": f"127.0.0.1:{control_ports[index]}",
                "HIVEMIND_WORKER_STATE_ROOT": str(self.root / f"worker-state-{index + 1}"),
                "WORKER_SHARING_MODE": "always",
            })
        def ready_workers():
            workers = http_json(self.master_url + "/api/workers", token=self.token).get("workers", [])
            expected = {f"phase3-worker-{index}" for index in range(1, 4)}
            ready = [worker for worker in workers if worker.get("worker_id") in expected
                     and str(worker.get("status", "")).upper() in ("IDLE", "ACTIVE")]
            return ready if len(ready) == 3 else None
        workers = self.wait_for("three registered, available Workers", ready_workers)
        return {"same_host": True, "cross_host_evidence": False,
                "hostname": socket.gethostname(), "worker_advertise_host": self.worker_host,
                "master_url": self.master_url, "nodepool_addr": nodepool_addr,
                "processes": [{"role": name, "pid": process.pid} for name, process in self.processes],
                "workers": workers, "isolated_postgres_container": self.pg_name,
                "isolated_redis_container": self.redis_name}

    def cli(self, arguments, label, timeout=120):
        return self.command([self.binary, *arguments], env={**self.base_env, "HIVEMIND_PASSWORD": self.account_password},
                            label=label, timeout=timeout)

    def run_pipeline(self, path, prefix, timeout):
        pipeline = json.loads(path.read_text(encoding="utf-8"))
        started = time.monotonic()
        self.cli(["pipeline", "run", path, "--api", self.master_url,
                  "--username", "phase3-requester", "--task-prefix", prefix,
                  "--max-total-cpt", "10000000", "--out", self.output / f"{prefix}-result.json",
                  "--replicas", "3", "--poll-secs", "1", "--per-minute", "60",
                  "--timeout-secs", str(timeout), "--cpu-score", "0", "--memory-gb", "0",
                  "--storage-gb", "0"], prefix, timeout=timeout * len(pipeline["stages"]) + 60)
        elapsed = time.monotonic() - started
        tasks = http_json(self.master_url + "/api/tasks", token=self.token)["tasks"]
        timing_rows = json.loads(self.sql(
            "SELECT COALESCE(json_agg(t), '[]'::json) FROM (SELECT task_id, created_at, completed_at, "
            "round(extract(epoch from completed_at - created_at)*1000, 3) AS latency_ms FROM tasks "
            f"WHERE task_id LIKE '{prefix}-%' ORDER BY task_id) t;"))
        timings = {row.pop("task_id"): row for row in timing_rows}
        summary = summarize_run(pipeline, tasks, prefix, timings, elapsed)
        write_json(self.output / f"{prefix}-tasks.json", tasks)
        write_json(self.output / f"{prefix}-summary.json", summary)
        return summary

    def evidence(self):
        replicas = json.loads(self.sql(
            "SELECT COALESCE(json_agg(t), '[]'::json) FROM (SELECT task_id, replica_id, worker_id, "
            "provider_user, state, success, usage_units, assigned_at, reported_at "
            "FROM managed_consensus_replicas ORDER BY task_id, replica_id) t;"))
        artifacts = json.loads(self.sql(
            "SELECT COALESCE(json_agg(t), '[]'::json) FROM (SELECT task_id, artifact_id, sha256, "
            "size_bytes, complete, availability_status FROM general_compute_artifacts ORDER BY task_id, artifact_id) t;"))
        expected_workers = {f"phase3-worker-{index}" for index in range(1, 4)}
        if {row["worker_id"] for row in replicas} != expected_workers:
            raise ValueError("the execution evidence does not include all three Workers")
        reported_workers = {row["worker_id"] for row in replicas
                            if row.get("success") is True and row.get("reported_at")
                            and type(row.get("usage_units")) is int and row["usage_units"] > 0}
        if reported_workers != expected_workers:
            missing = ", ".join(sorted(expected_workers - reported_workers))
            raise ValueError(f"missing successful positive-usage report from Workers: {missing}")
        if not artifacts or not all(row["complete"] for row in artifacts):
            raise ValueError("the model has no complete uploaded artifact evidence")
        write_json(self.output / "replicas.json", replicas)
        write_json(self.output / "artifacts.json", artifacts)
        return {"replica_rows": len(replicas), "workers_with_successful_reports": len(reported_workers),
                "artifacts": len(artifacts),
                "artifact_bytes": sum(row["size_bytes"] for row in artifacts)}

    def close(self):
        errors = []
        for name, process in reversed(self.processes):
            try:
                if process.poll() is None:
                    process.terminate()
                    try:
                        process.wait(timeout=15)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait(timeout=15)
            except (OSError, subprocess.TimeoutExpired) as error:
                if process.poll() is None:
                    errors.append(f"{name}: {error}")
        for log in self.logs:
            log.close()
        for container_id in reversed(self.containers):
            # Exact IDs returned by this run, never a project-wide prune or name glob.
            try:
                self.command(["docker", "rm", "--force", "--volumes", container_id], timeout=60)
            except (OSError, RuntimeError, subprocess.TimeoutExpired) as error:
                errors.append(f"container {container_id}: {error}")
        if self.root:
            root = self.root.resolve()
            if root.parent != Path(tempfile.gettempdir()).resolve() or not root.name.startswith(f"hm-phase3-{self.run_id}-"):
                errors.append("refusing to remove an unexpected temporary workspace")
            elif root.exists():
                try:
                    shutil.rmtree(root)
                except OSError as error:
                    errors.append(f"temporary workspace: {error}")
        self.save_resources([self.scrub(error) for error in errors])
        if errors:
            raise RuntimeError("cleanup errors: " + self.scrub("; ".join(errors)))


def final_output(stdout):
    for line in stdout.splitlines():
        if line.startswith("Final output: "):
            return json.loads(line.removeprefix("Final output: "))
    raise ValueError("pipeline check did not print its final JSON output")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=REPO / "hivemind-rs/target/debug/hivemind-bin.exe")
    parser.add_argument("--out-dir", type=Path)
    parser.add_argument("--worker-host", help="local non-loopback IPv4 address; defaults to this host's route address")
    parser.add_argument("--startup-timeout", type=int, default=180)
    parser.add_argument("--task-timeout", type=int, default=120)
    parser.add_argument("--check-only", action="store_true", help="check tools/images; do not write files or start services")
    options = parser.parse_args()
    options.binary = options.binary.resolve()
    if not options.binary.is_file():
        parser.error("build the default-feature hivemind-bin first, or supply --binary")
    for tool in ("docker", "openssl"):
        if shutil.which(tool) is None:
            parser.error(f"{tool} is required")
    for image in ("postgres:16-alpine", "redis:7-alpine"):
        result = subprocess.run(["docker", "image", "inspect", image, "--format", "{{.Id}}"],
                                capture_output=True, text=True, timeout=30)
        if result.returncode:
            parser.error(f"Docker image {image} is not available locally; pull it before running")
    host = options.worker_host or local_worker_host()
    address = ipaddress.ip_address(host)
    if address.version != 4 or address.is_loopback or address.is_unspecified:
        parser.error("--worker-host must be this host's reachable, non-loopback IPv4 address")
    try:
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as local:
            local.bind((host, 0))
    except OSError:
        parser.error("--worker-host is not an address assigned to this machine")
    if options.check_only:
        print(json.dumps({"binary": str(options.binary.resolve()), "worker_host": host,
                          "same_host": True, "prerequisites": "available"}, indent=2))
        return
    output = (options.out_dir or REPO / "test_logs" / f"phase3-{uuid.uuid4().hex[:12]}").resolve()
    if output.exists() and any(output.iterdir()):
        parser.error("--out-dir must be absent or empty so an earlier report cannot be overwritten")
    stack = LocalStack(options.binary, output, options.startup_timeout, host)
    summary = {"status": "running", "same_host": True, "cross_host_evidence": False,
               "started_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
               "binary_sha256": hashlib.sha256(options.binary.read_bytes()).hexdigest()}
    try:
        summary["topology"] = stack.start()
        write_json(output / "summary.json", summary)
        fixture = write_fixtures(output / "fixtures")
        ordinary_local = final_output(stack.cli(["pipeline", "check", fixture["ordinary"]], "ordinary-check"))
        if ordinary_local != [13]:
            raise ValueError("ordinary local pipeline returned an unexpected result")
        summary["ordinary"] = stack.run_pipeline(fixture["ordinary"], "ordinary", options.task_timeout)
        if summary["ordinary"]["final_output"] != ordinary_local:
            raise ValueError("ordinary remote output differs from local execution")
        model_dir = fixture["weights"].parent
        model_json = model_dir / "model.json"
        split_dir = model_dir / "split"
        stack.cli(["model", "init", fixture["weights"], "--activation", "relu", "--final", "softmax",
                   "--out", model_json], "model-init")
        stack.cli(["model", "split", model_json, "--weights", fixture["weights"],
                   "--input", fixture["input"].read_text(encoding="utf-8"),
                   "--out-dir", split_dir, "--max-tensor-bytes", "240", "--max-materialization-bytes", "8000"], "model-split")
        pipeline_path = split_dir / "pipeline.json"
        pipeline = json.loads(pipeline_path.read_text(encoding="utf-8"))
        if len(pipeline["stages"]) < 2 or not any(len(stage["items"]) > 1 for stage in pipeline["stages"]):
            raise ValueError("model split did not exercise both stages and parallel row shards")
        local = final_output(stack.cli(["pipeline", "check", pipeline_path], "model-check"))
        for actual_row, reference_row in zip(local[0], fixture["reference"], strict=True):
            for actual, reference in zip(actual_row, reference_row, strict=True):
                if abs(actual - reference) > 1e-12:
                    raise ValueError("split model differs from independent scalar reference")
        summary["model"] = stack.run_pipeline(pipeline_path, "model", options.task_timeout)
        if summary["model"]["final_output"] != local:
            raise ValueError("remote split model differs from local execution")
        summary["evidence"] = stack.evidence()
        summary["status"] = "passed"
    except Exception as error:
        summary["status"] = "failed"
        summary["error"] = stack.scrub(str(error))
        raise
    finally:
        try:
            stack.close()
            summary["cleanup"] = "stopped the five owned processes and removed only this run's disposable containers and temporary workspace"
        except Exception as error:
            summary["cleanup_error"] = stack.scrub(str(error))
            summary["status"] = "failed"
        summary["finished_at"] = datetime.datetime.now(datetime.timezone.utc).isoformat()
        write_json(output / "summary.json", summary)
        print(f"Same-host phase-3 validation: {summary['status']}. Report: {output / 'summary.json'}")
    if summary["status"] != "passed":
        raise SystemExit(1)


if __name__ == "__main__":
    main()
