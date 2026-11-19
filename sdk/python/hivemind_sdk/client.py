"""A client for the Hivemind Master HTTP API. It uses only the Python standard library.

A token is an API token (created with `Client.create_token`, or in the Master console) or a sign-in token.
Pass it to `Client`, or set `HIVEMIND_TOKEN`. The token is sent only in the Authorization header and is never
printed by this module.
"""

from __future__ import annotations

import csv
import json
import os
import time
import urllib.error
import urllib.parse
import urllib.request
from typing import Any, Callable, Dict, Iterator, List, Mapping, Optional, Sequence

TOKEN_ENV = "HIVEMIND_TOKEN"
TERMINAL_JOB_STATES = ("completed", "cancelled")
_RESOURCE_FIELDS = (
    "cpu_score",
    "gpu_score",
    "memory_gb",
    "gpu_memory_gb",
    "storage_gb",
    "host_count",
)


class HivemindError(Exception):
    """A request failed. `status` is the HTTP status, or 0 when the server could not be reached."""

    def __init__(self, status: int, message: str) -> None:
        super().__init__(f"{status}: {message}" if status else message)
        self.status = status
        self.message = message


def _decode(raw: bytes) -> Any:
    if not raw:
        return {}
    try:
        return json.loads(raw.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError):
        return {}


def _resources(values: Mapping[str, Any]) -> Dict[str, Any]:
    fields: Dict[str, Any] = {}
    for name in _RESOURCE_FIELDS:
        value = values.get(name)
        if value:
            fields[name] = value
    return fields


class Client:
    """Talks to one Master. Every method raises `HivemindError` when the request fails."""

    def __init__(
        self,
        base_url: str,
        token: Optional[str] = None,
        *,
        timeout: float = 30.0,
        sleep: Callable[[float], None] = time.sleep,
        clock: Callable[[], float] = time.monotonic,
    ) -> None:
        resolved = (token if token is not None else os.environ.get(TOKEN_ENV, "")).strip()
        if not resolved:
            raise HivemindError(0, f"a token is required: pass one or set {TOKEN_ENV}")
        self._base = base_url.rstrip("/")
        self._token = resolved
        self._timeout = timeout
        self._sleep = sleep
        self._clock = clock

    def __repr__(self) -> str:
        return f"Client({self._base!r}, token=***)"

    def _request(
        self,
        method: str,
        path: str,
        body: Any = None,
        query: Optional[Mapping[str, Any]] = None,
        extra_headers: Optional[Mapping[str, str]] = None,
    ) -> Any:
        url = self._base + path
        if query:
            url += "?" + urllib.parse.urlencode(query)
        headers = {"Authorization": f"Bearer {self._token}", "Accept": "application/json"}
        if extra_headers:
            headers.update(extra_headers)
        data = None
        if body is not None:
            data = json.dumps(body).encode("utf-8")
            headers["Content-Type"] = "application/json"
        request = urllib.request.Request(url, data=data, method=method, headers=headers)
        try:
            with urllib.request.urlopen(request, timeout=self._timeout) as response:
                return _decode(response.read())
        except urllib.error.HTTPError as error:
            payload = _decode(error.read())
            message = None
            if isinstance(payload, dict):
                err_obj = payload.get("error")
                if isinstance(err_obj, dict):
                    message = err_obj.get("message")
                if not message:
                    message = payload.get("message")
            raise HivemindError(error.code, str(message or error.reason or "request failed")) from None
        except OSError as error:
            raise HivemindError(0, f"could not reach {self._base}: {error.__class__.__name__}") from None

    # Tasks, one function run each.

    def dry_run(self, source: str, input_json: str, *, replicas: Optional[int] = None) -> Dict[str, Any]:
        """Runs the function once on the Master's machine and prices it. Nothing is charged.

        The answer carries `success`. A program that fails returns `success: false` with `error`, not an
        exception, so the caller can show the line and column.
        """
        body: Dict[str, Any] = {"task_source": source, "torrent": input_json}
        if replicas is not None:
            body["managed_replica_count"] = replicas
        return self._request("POST", "/api/tasks/dry-run", body)

    def quote(self, source: str, input_json: str, **resources: Any) -> int:
        """The price of the computer requirements a task reserves, in CPT."""
        body: Dict[str, Any] = {"runtime": "hcode", "task_source": source, "torrent": input_json, "max_cpt": 1}
        body.update(_resources(resources))
        payload = self._request("POST", "/api/tasks/quote", body)
        return int(payload.get("quoted_cpt", 0))

    def submit(
        self,
        task_id: str,
        source: str,
        input_json: str,
        *,
        max_cpt: int,
        replicas: Optional[int] = None,
        **resources: Any,
    ) -> Dict[str, Any]:
        """Submits one task. `max_cpt` is the task-wide cap, fee included."""
        body: Dict[str, Any] = {
            "task_id": task_id,
            "runtime": "hcode",
            "task_source": source,
            "torrent": input_json,
            "max_cpt": max_cpt,
        }
        body.update(_resources(resources))
        if replicas is not None:
            body["managed_replica_count"] = replicas
            body["managed_quorum"] = replicas // 2 + 1
        return self._request("POST", "/api/tasks", body)

    def task(self, task_id: str) -> Optional[Dict[str, Any]]:
        """The task's record, or None when the account has no task with that id."""
        payload = self._request("GET", "/api/tasks", query={"task_ids": task_id})
        for task in payload.get("tasks", []):
            if task.get("task_id") == task_id:
                return task
        return None

    def task_log(self, task_id: str) -> str:
        """All of the task's output."""
        payload = self._request("GET", f"/api/tasks/{urllib.parse.quote(task_id, safe='')}/log")
        return str(payload.get("log", ""))

    # Jobs, one function over many inputs. Nodepool runs them; a script may disconnect after creating one.

    def create_job(
        self,
        source: str,
        inputs: Sequence[Any],
        *,
        per_item_max_cpt: int,
        job_max_cpt: Optional[int] = None,
        name: str = "",
        replicas: Optional[int] = None,
        **resources: Any,
    ) -> Dict[str, Any]:
        """Creates a job. Each input is a JSON value. Returns the job's id and its total cap.

        `job_max_cpt` caps the whole job and defaults to `per_item_max_cpt` times the number of inputs.
        """
        body: Dict[str, Any] = {
            "name": name,
            "task_source": source,
            "inputs": list(inputs),
            "per_item_max_cpt": per_item_max_cpt,
        }
        if job_max_cpt is not None:
            body["job_max_cpt"] = job_max_cpt
        if replicas is not None:
            body["replica_count"] = replicas
        body.update(_resources(resources))
        return self._request("POST", "/api/jobs", body)

    def job(self, job_id: str) -> Dict[str, Any]:
        """The job's state and counts. `state` is running, cancelling, completed, or cancelled."""
        payload = self._request("GET", f"/api/jobs/{_path_id(job_id)}")
        if not payload.get("job"):
            raise HivemindError(404, payload.get("message") or "Job not found")
        return payload["job"]

    def jobs(self) -> List[Dict[str, Any]]:
        """The account's most recent jobs."""
        return list(self._request("GET", "/api/jobs").get("jobs", []))

    def job_items(self, job_id: str, *, offset: int = 0, limit: int = 50) -> Dict[str, Any]:
        """One page of a job's items, with the total. Output is a preview; `task_log` returns all of it."""
        return self._request(
            "GET",
            f"/api/jobs/{_path_id(job_id)}/items",
            query={"offset": offset, "limit": limit},
        )

    def iter_job_items(self, job_id: str, *, page_size: int = 100) -> Iterator[Dict[str, Any]]:
        """Every item of a job, in order, page by page."""
        offset = 0
        while True:
            page = self.job_items(job_id, offset=offset, limit=page_size)
            items = page.get("items", [])
            for item in items:
                yield item
            offset += len(items)
            if not items or offset >= int(page.get("total", 0)):
                return

    def cancel_job(self, job_id: str) -> Dict[str, Any]:
        """Asks for the job to stop. Items that have not started are cancelled, and running tasks are stopped."""
        return self._request("POST", f"/api/jobs/{_path_id(job_id)}/cancel")

    def wait_for_job(
        self,
        job_id: str,
        *,
        poll_seconds: float = 5.0,
        timeout_seconds: float = 3600.0,
    ) -> Dict[str, Any]:
        """Polls until the job completes or is cancelled, then returns its final state."""
        deadline = self._clock() + timeout_seconds
        while True:
            job = self.job(job_id)
            if job.get("state") in TERMINAL_JOB_STATES:
                return job
            if self._clock() >= deadline:
                raise HivemindError(
                    0,
                    f"job {job_id} is still {job.get('state')} after {timeout_seconds:g} seconds",
                )
            self._sleep(poll_seconds)

    def claim_allowance(self) -> Dict[str, Any]:
        """Claims the daily CPT allowance if the network offers one: once in every 24 hours per account."""
        return self._request("POST", "/api/allowance/claim")

    # API tokens. Creating, listing, and revoking them needs a sign-in token, not an API token.

    def create_token(self, name: str, *, expires_in_days: Optional[int] = None) -> Dict[str, Any]:
        """Creates an API token. `api_token` is returned once: store it, because it cannot be shown again."""
        body: Dict[str, Any] = {"name": name}
        if expires_in_days is not None:
            body["expires_in_days"] = expires_in_days
        return self._request("POST", "/api/tokens", body)

    def list_tokens(self) -> List[Dict[str, Any]]:
        """The account's tokens, without their secrets."""
        return list(self._request("GET", "/api/tokens").get("tokens", []))

    def revoke_token(self, token_id: str) -> Dict[str, Any]:
        """Revokes one token. It stops working at once."""
        return self._request("DELETE", f"/api/tokens/{_path_id(token_id)}")



    # Unified OpenAI-compatible LLM inference endpoints

    @staticmethod
    def _routing_headers(routing: Optional[Mapping[str, Any]]) -> Dict[str, str]:
        if not routing:
            return {}
        headers: Dict[str, str] = {}
        mapping = {
            "mode": "X-Hivemind-Route-Mode",
            "target_worker": "X-Hivemind-Target-Worker",
            "device_tag": "X-Hivemind-Device-Tag",
            "hardware": "X-Hivemind-Hardware",
            "trust_tier": "X-Hivemind-Trust-Tier",
            "overflow_policy": "X-Hivemind-Overflow-Policy",
        }
        for key, header in mapping.items():
            if routing.get(key) is not None:
                headers[header] = str(routing[key])
        if "own_devices_only" in routing and routing["own_devices_only"] is not None:
            headers["X-Hivemind-Own-Devices-Only"] = "true" if routing["own_devices_only"] else "false"
        if routing.get("target_workers"):
            headers["X-Hivemind-Target-Workers"] = ",".join(str(w) for w in routing["target_workers"])
        return headers

    def list_models(self) -> List[Dict[str, Any]]:
        """Lists online models exposed through the OpenAI-compatible `/v1/models` endpoint."""
        return list(self._request("GET", "/v1/models").get("data", []))

    def chat_completions(
        self,
        model: str,
        messages: Sequence[Mapping[str, Any]],
        *,
        max_tokens: Optional[int] = None,
        temperature: Optional[float] = None,
        top_p: Optional[float] = None,
        stop: Optional[Sequence[str]] = None,
        routing: Optional[Mapping[str, Any]] = None,
    ) -> Dict[str, Any]:
        """Calls `POST /v1/chat/completions` in non-streaming mode."""
        body: Dict[str, Any] = {"model": model, "messages": list(messages), "stream": False}
        if max_tokens is not None:
            body["max_tokens"] = max_tokens
        if temperature is not None:
            body["temperature"] = temperature
        if top_p is not None:
            body["top_p"] = top_p
        if stop is not None:
            body["stop"] = list(stop)
        return self._request(
            "POST",
            "/v1/chat/completions",
            body,
            extra_headers=self._routing_headers(routing),
        )

    def stream_chat_completions(
        self,
        model: str,
        messages: Sequence[Mapping[str, Any]],
        *,
        max_tokens: Optional[int] = None,
        temperature: Optional[float] = None,
        routing: Optional[Mapping[str, Any]] = None,
    ) -> Iterator[Dict[str, Any]]:
        """Streams SSE chunks from `POST /v1/chat/completions` with `stream=true`."""
        body: Dict[str, Any] = {"model": model, "messages": list(messages), "stream": True}
        if max_tokens is not None:
            body["max_tokens"] = max_tokens
        if temperature is not None:
            body["temperature"] = temperature
        headers = {
            "Authorization": f"Bearer {self._token}",
            "Accept": "text/event-stream",
            "Content-Type": "application/json",
        }
        headers.update(self._routing_headers(routing))
        request = urllib.request.Request(
            self._base + "/v1/chat/completions",
            data=json.dumps(body).encode("utf-8"),
            method="POST",
            headers=headers,
        )
        try:
            with urllib.request.urlopen(request, timeout=self._timeout) as response:
                for raw_line in response:
                    line = raw_line.decode("utf-8", errors="replace").strip()
                    if not line.startswith("data:"):
                        continue
                    payload = line[5:].strip()
                    if payload == "[DONE]":
                        return
                    if payload:
                        yield json.loads(payload)
        except urllib.error.HTTPError as error:
            payload = _decode(error.read())
            err_msg = (payload.get("error") or {}).get("message") if isinstance(payload, dict) else None
            raise HivemindError(error.code, str(err_msg or payload.get("message") or error.reason)) from None
        except OSError as error:
            raise HivemindError(0, f"could not reach {self._base}: {error.__class__.__name__}") from None

    # WASM/WASI tasks & DAG pipelines

    def submit_wasm_task(
        self,
        task_id: str,
        wasm_base64: str,
        *,
        entrypoint: str = "_start",
        input_json: str = "{}",
        max_fuel: int = 10_000_000,
        max_memory_pages: int = 256,
        max_cpt: int = 50,
    ) -> Dict[str, Any]:
        """Submits a bounded WASM/WASI general-compute task."""
        body = {
            "task_id": task_id,
            "runtime": "wasm-wasi-v1",
            "task_source": wasm_base64,
            "torrent": json.dumps({
                "entrypoint": entrypoint,
                "input": input_json,
                "max_fuel": max_fuel,
                "max_memory_pages": max_memory_pages,
            }),
            "max_cpt": max_cpt,
        }
        return self._request("POST", "/api/tasks", body)

    def create_dag_pipeline(
        self,
        name: str,
        nodes: Sequence[Mapping[str, Any]],
        *,
        budget_cpt: int,
    ) -> Dict[str, Any]:
        """Creates a server-side DAG workflow with map/reduce dependencies and a total CPT budget."""
        return self._request(
            "POST",
            "/api/pipelines",
            {"name": name, "budget_cpt": budget_cpt, "nodes": list(nodes)},
        )

    def get_dag_pipeline(self, pipeline_id: str) -> Dict[str, Any]:
        return self._request("GET", f"/api/pipelines/{_path_id(pipeline_id)}")

    def cancel_dag_pipeline(self, pipeline_id: str) -> Dict[str, Any]:
        return self._request("POST", f"/api/pipelines/{_path_id(pipeline_id)}/cancel")

    def list_devices(self) -> List[Dict[str, Any]]:
        return list(self._request("GET", "/api/devices").get("devices", []))

    def network_status(self) -> Dict[str, Any]:
        return self._request("GET", "/api/network/status")


def _path_id(value: str) -> str:
    return urllib.parse.quote(str(value), safe="")


_FORMULA_PREFIXES = ("=", "+", "-", "@", "\t", "\r")


def _spreadsheet_safe(value: Any) -> Any:
    """Marks a cell that a spreadsheet would read as a formula, so opening the file cannot run it."""
    if isinstance(value, str) and value.startswith(_FORMULA_PREFIXES):
        return "'" + value
    return value


def write_results_csv(client: Client, job_id: str, path: str) -> int:
    """Writes one row per item and returns the number of rows. Output is the preview; use `task_log` for all of it."""
    fields = ["index", "task_id", "state", "billed_cpt", "output_bytes", "output_preview", "status_message"]
    rows = 0
    with open(path, "w", newline="", encoding="utf-8") as handle:
        writer = csv.writer(handle)
        writer.writerow(fields)
        for item in client.iter_job_items(job_id):
            writer.writerow([_spreadsheet_safe(item.get(name, "")) for name in fields])
            rows += 1
    return rows
