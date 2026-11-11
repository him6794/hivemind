"""Tests for the SDK against a local stand-in for the Master. Run with: python -m unittest discover -s tests"""

import csv
import json
import os
import socket
import tempfile
import threading
import unittest
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlparse

import sys

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

from hivemind_sdk import Client, HivemindError, write_results_csv  # noqa: E402


class FakeMaster:
    """Answers each (method, path) from a queue of responses, and records every request it receives."""

    def __init__(self):
        self.responses = {}
        self.requests = []
        fake = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *args):
                pass

            def _serve(self, method):
                length = int(self.headers.get("Content-Length") or 0)
                raw = self.rfile.read(length) if length else b""
                parsed = urlparse(self.path)
                record = {
                    "method": method,
                    "path": parsed.path,
                    "query": {k: v[0] for k, v in parse_qs(parsed.query).items()},
                    "headers": dict(self.headers),
                    "body": json.loads(raw) if raw else None,
                }
                fake.requests.append(record)
                queue = fake.responses.get((method, parsed.path))
                if not queue:
                    status, payload = 404, {"success": False, "message": "no fake response"}
                elif len(queue) > 1:
                    status, payload = queue.pop(0)
                else:
                    status, payload = queue[0]
                body = json.dumps(payload).encode("utf-8")
                self.send_response(status)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

            def do_GET(self):
                self._serve("GET")

            def do_POST(self):
                self._serve("POST")

            def do_DELETE(self):
                self._serve("DELETE")

        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.base = f"http://127.0.0.1:{self.server.server_address[1]}"
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)

    def start(self):
        self.thread.start()
        return self

    def stop(self):
        self.server.shutdown()
        self.server.server_close()

    def answer(self, method, path, status, payload):
        self.responses.setdefault((method, path), []).append((status, payload))

    def answer_sequence(self, method, path, *replies):
        self.responses[(method, path)] = list(replies)


class SdkTests(unittest.TestCase):
    def setUp(self):
        self.master = FakeMaster().start()
        self.addCleanup(self.master.stop)
        self.client = Client(self.master.base, "test-token")

    def test_every_request_carries_the_bearer_token_and_a_json_body(self):
        self.master.answer("POST", "/api/jobs", 201, {"success": True, "job_id": "j1", "item_count": 2, "job_max_cpt": 40})
        created = self.client.create_job("let x = 1;\nx;", [{"n": 1}, {"n": 2}], per_item_max_cpt=10, job_max_cpt=40, name="sweep", cpu_score=250, replicas=5)
        self.assertEqual(created["job_id"], "j1")
        request = self.master.requests[-1]
        self.assertEqual(request["headers"]["Authorization"], "Bearer test-token")
        self.assertEqual(request["body"]["inputs"], [{"n": 1}, {"n": 2}])
        self.assertEqual(request["body"]["per_item_max_cpt"], 10)
        self.assertEqual(request["body"]["job_max_cpt"], 40)
        self.assertEqual(request["body"]["replica_count"], 5)
        self.assertEqual(request["body"]["cpu_score"], 250)
        self.assertNotIn("memory_gb", request["body"], "zero resources are not sent")

    def test_the_default_job_cap_is_left_to_the_server(self):
        self.master.answer("POST", "/api/jobs", 201, {"success": True, "job_id": "j2", "item_count": 1, "job_max_cpt": 10})
        self.client.create_job("1;", [1], per_item_max_cpt=10)
        self.assertNotIn("job_max_cpt", self.master.requests[-1]["body"])
        self.assertNotIn("replica_count", self.master.requests[-1]["body"])

    def test_the_token_comes_from_the_environment_and_is_never_printed(self):
        previous = os.environ.get("HIVEMIND_TOKEN")
        os.environ["HIVEMIND_TOKEN"] = "env-token"
        try:
            client = Client(self.master.base)
        finally:
            if previous is None:
                os.environ.pop("HIVEMIND_TOKEN", None)
            else:
                os.environ["HIVEMIND_TOKEN"] = previous
        self.master.answer("GET", "/api/jobs", 200, {"success": True, "jobs": []})
        client.jobs()
        self.assertEqual(self.master.requests[-1]["headers"]["Authorization"], "Bearer env-token")
        self.assertNotIn("env-token", repr(client))

    def test_a_missing_token_is_refused_before_any_request(self):
        previous = os.environ.pop("HIVEMIND_TOKEN", None)
        try:
            with self.assertRaises(HivemindError) as raised:
                Client(self.master.base)
        finally:
            if previous is not None:
                os.environ["HIVEMIND_TOKEN"] = previous
        self.assertIn("a token is required", raised.exception.message)
        self.assertEqual(self.master.requests, [])

    def test_an_error_answer_raises_with_the_status_and_the_server_message(self):
        self.master.answer("POST", "/api/jobs", 400, {"success": False, "message": "job source is required"})
        with self.assertRaises(HivemindError) as raised:
            self.client.create_job("", [1], per_item_max_cpt=10)
        self.assertEqual(raised.exception.status, 400)
        self.assertEqual(raised.exception.message, "job source is required")

    def test_an_unreachable_master_raises_with_status_zero(self):
        probe = socket.socket()
        probe.bind(("127.0.0.1", 0))
        port = probe.getsockname()[1]
        probe.close()
        client = Client(f"http://127.0.0.1:{port}", "t", timeout=2)
        with self.assertRaises(HivemindError) as raised:
            client.jobs()
        self.assertEqual(raised.exception.status, 0)

    def test_wait_for_job_polls_until_the_job_is_terminal(self):
        self.master.answer_sequence(
            "GET",
            "/api/jobs/j3",
            (200, {"success": True, "job": {"state": "running"}}),
            (200, {"success": True, "job": {"state": "running"}}),
            (200, {"success": True, "job": {"state": "completed", "completed": 2}}),
        )
        sleeps = []
        client = Client(self.master.base, "t", sleep=sleeps.append)
        job = client.wait_for_job("j3", poll_seconds=7)
        self.assertEqual(job["state"], "completed")
        self.assertEqual(sleeps, [7, 7])

    def test_wait_for_job_gives_up_after_its_timeout(self):
        self.master.answer("GET", "/api/jobs/j4", 200, {"success": True, "job": {"state": "running"}})
        ticks = iter([0.0, 5.0, 11.0])
        client = Client(self.master.base, "t", sleep=lambda _: None, clock=lambda: next(ticks))
        with self.assertRaises(HivemindError) as raised:
            client.wait_for_job("j4", timeout_seconds=10)
        self.assertIn("still running after 10 seconds", raised.exception.message)

    def test_items_are_read_page_by_page_until_the_total(self):
        pages = {
            0: {"success": True, "total": 5, "items": [{"index": 1}, {"index": 2}]},
            2: {"success": True, "total": 5, "items": [{"index": 3}, {"index": 4}]},
            4: {"success": True, "total": 5, "items": [{"index": 5}]},
        }
        self.master.answer_sequence(
            "GET",
            "/api/jobs/j5/items",
            (200, pages[0]),
            (200, pages[2]),
            (200, pages[4]),
        )
        seen = [item["index"] for item in self.client.iter_job_items("j5", page_size=2)]
        self.assertEqual(seen, [1, 2, 3, 4, 5])
        offsets = [r["query"]["offset"] for r in self.master.requests if r["path"] == "/api/jobs/j5/items"]
        self.assertEqual(offsets, ["0", "2", "4"])

    def test_token_management_uses_the_documented_routes(self):
        self.master.answer("POST", "/api/tokens", 201, {"success": True, "api_token": "hmk_x", "token_id": "t1"})
        self.master.answer("GET", "/api/tokens", 200, {"success": True, "tokens": [{"token_id": "t1"}]})
        self.master.answer("DELETE", "/api/tokens/t1", 200, {"success": True, "message": "OK"})
        created = self.client.create_token("ci", expires_in_days=30)
        self.assertEqual(created["api_token"], "hmk_x")
        self.assertEqual(self.master.requests[-1]["body"], {"name": "ci", "expires_in_days": 30})
        self.assertEqual(self.client.list_tokens()[0]["token_id"], "t1")
        self.assertTrue(self.client.revoke_token("t1")["success"])
        self.assertEqual(self.master.requests[-1]["method"], "DELETE")

    def test_dry_run_and_quote_send_the_source_and_the_input(self):
        self.master.answer("POST", "/api/tasks/dry-run", 200, {"success": True, "output": "60"})
        self.master.answer("POST", "/api/tasks/quote", 200, {"success": True, "quoted_cpt": 105})
        self.assertEqual(self.client.dry_run("1;", "{}", replicas=2)["output"], "60")
        body = self.master.requests[-1]["body"]
        self.assertEqual(body, {"task_source": "1;", "torrent": "{}", "managed_replica_count": 2})
        self.assertEqual(self.client.quote("1;", "{}", cpu_score=250, host_count=3), 105)
        self.assertEqual(self.master.requests[-1]["body"]["max_cpt"], 1)

    def test_the_daily_allowance_is_one_post_and_returns_the_grant(self):
        self.master.answer(
            "POST",
            "/api/allowance/claim",
            200,
            {"success": True, "message": "OK", "granted_cpt": 50, "balance": 50, "next_claim_at": "2026-11-12T00:00:00+00:00"},
        )
        result = self.client.claim_allowance()
        self.assertEqual(result["granted_cpt"], 50)
        self.assertEqual(self.master.requests[-1]["method"], "POST")
        self.assertEqual(self.master.requests[-1]["path"], "/api/allowance/claim")
        self.assertIsNone(self.master.requests[-1]["body"])

    def test_a_refused_allowance_claim_raises_with_the_reason(self):
        self.master.answer(
            "POST",
            "/api/allowance/claim",
            409,
            {"success": False, "message": "already claimed; the next claim is at 2026-11-12T00:00:00+00:00"},
        )
        with self.assertRaises(HivemindError) as raised:
            self.client.claim_allowance()
        self.assertEqual(raised.exception.status, 409)
        self.assertIn("already claimed", raised.exception.message)

    def test_a_job_name_and_item_ids_are_escaped_in_the_path(self):
        self.master.answer("POST", "/api/jobs/a%2Fb/cancel", 200, {"success": True})
        self.client.cancel_job("a/b")
        self.assertEqual(self.master.requests[-1]["path"], "/api/jobs/a%2Fb/cancel")

    def test_results_csv_neutralises_cells_a_spreadsheet_would_run(self):
        self.master.answer(
            "GET",
            "/api/jobs/j6/items",
            200,
            {
                "success": True,
                "total": 2,
                "items": [
                    {"index": 1, "task_id": "t", "state": "completed", "billed_cpt": 8, "output_bytes": 2, "output_preview": "=1+1", "status_message": ""},
                    {"index": 2, "task_id": "u", "state": "failed", "billed_cpt": 0, "output_bytes": 0, "output_preview": "", "status_message": "@cmd"},
                ],
            },
        )
        with tempfile.TemporaryDirectory() as folder:
            path = os.path.join(folder, "results.csv")
            self.assertEqual(write_results_csv(self.client, "j6", path), 2)
            with open(path, newline="", encoding="utf-8") as handle:
                rows = list(csv.DictReader(handle))
        self.assertEqual(rows[0]["output_preview"], "'=1+1")
        self.assertEqual(rows[1]["status_message"], "'@cmd")
        self.assertEqual(rows[0]["billed_cpt"], "8")


if __name__ == "__main__":
    unittest.main()
