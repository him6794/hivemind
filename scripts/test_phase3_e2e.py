"""Read-only unit checks for the local phase-3 E2E runner; no services are started."""

import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import Mock, patch


SCRIPT = Path(__file__).with_name("phase3_e2e.py")


class Phase3HarnessTests(unittest.TestCase):
    def setUp(self):
        self.assertTrue(SCRIPT.is_file(), "phase3_e2e.py has not been implemented")
        spec = importlib.util.spec_from_file_location("phase3_e2e", SCRIPT)
        self.runner = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.runner)

    def test_child_environment_cannot_inherit_production_targets_or_tokens(self):
        source = {
            "PATH": "tools", "SystemRoot": "windows", "TEMP": "temporary",
            "DATABASE_URL": "production", "REDIS_URL": "production",
            "HIVEMIND_CONFIG": "production.json", "JWT_SECRET": "production-secret",
            "WORKER_NODEPOOL_TOKEN": "production-token", "NODEPOOL_GRPC_ENDPOINT": "remote:50051",
            "MASTER_WEBSITE_API_BASE": "https://production", "UPDATE_ENABLED": "true",
            "HTTP_PROXY": "http://external-proxy", "https_proxy": "http://external-proxy",
            "ALL_PROXY": "socks5://external-proxy", "NO_PROXY": "unrelated.example",
        }
        clean = self.runner.child_environment(source)
        self.assertEqual(clean["PATH"], "tools")
        self.assertEqual(clean["SystemRoot"], "windows")
        for key in source.keys() - {"PATH", "SystemRoot", "TEMP", "NO_PROXY"}:
            self.assertNotIn(key, clean)
        self.assertEqual(clean["NO_PROXY"], "*")

    def test_fixtures_have_parallel_pipeline_and_valid_mixed_precision_model(self):
        with tempfile.TemporaryDirectory() as temporary:
            folder = Path(temporary)
            fixture = self.runner.write_fixtures(folder)
            pipeline = json.loads(fixture["ordinary"].read_text(encoding="utf-8"))
            self.assertEqual([len(stage["items"]) for stage in pipeline["stages"]], [2, 1])
            data = fixture["weights"].read_bytes()
            header_size = int.from_bytes(data[:8], "little")
            header = json.loads(data[8:8 + header_size])
            self.assertEqual(header["0.weight"]["dtype"], "F32")
            self.assertEqual(header["2.weight"]["dtype"], "F64")
            expected = fixture["reference"]
            self.assertEqual(len(expected), 2)
            for row in expected:
                self.assertEqual(len(row), 3)
                self.assertAlmostEqual(sum(row), 1.0, places=14)

    def test_model_split_receives_inline_json_not_the_fixture_path(self):
        with tempfile.TemporaryDirectory() as temporary:
            folder = Path(temporary)
            binary = folder / "binary.exe"
            binary.touch()
            output = folder / "report"
            calls = []

            def cli(stack, arguments, label, timeout=120):
                calls.append(arguments)
                if arguments[:2] == ["pipeline", "check"]:
                    return "Final output: [13]"
                if arguments[:2] == ["model", "split"]:
                    raise RuntimeError("stop after inspecting model input")
                return ""

            with patch.object(self.runner.shutil, "which", return_value="tool"), \
                    patch.object(self.runner.subprocess, "run", return_value=Mock(returncode=0)), \
                    patch.object(self.runner, "local_worker_host", return_value="192.0.2.1"), \
                    patch.object(self.runner.socket, "socket"), \
                    patch.object(self.runner.LocalStack, "start", return_value={}), \
                    patch.object(self.runner.LocalStack, "cli", cli), \
                    patch.object(self.runner.LocalStack, "run_pipeline", return_value={"final_output": [13]}), \
                    patch.object(self.runner.LocalStack, "close"), \
                    patch("builtins.print"), \
                    patch("sys.argv", [str(SCRIPT), "--binary", str(binary), "--out-dir", str(output)]):
                with self.assertRaisesRegex(RuntimeError, "stop after inspecting model input"):
                    self.runner.main()

            arguments = next(args for args in calls if args[:2] == ["model", "split"])
            actual = arguments[arguments.index("--input") + 1]
            self.assertIsInstance(actual, str, "--input requires inline JSON, not a Path")
            expected = json.loads((output / "fixtures/model/input.json").read_text(encoding="utf-8"))
            self.assertEqual(json.loads(actual), expected)

    def test_services_explicitly_disable_automatic_ui_windows(self):
        with tempfile.TemporaryDirectory() as temporary:
            folder = Path(temporary)
            stack = self.runner.LocalStack(folder / "binary.exe", folder, 10, "192.0.2.1")
            stack.root = folder
            stack.common_env = {"HIVEMIND_DISABLE_OPEN_UI": "0"}
            process = Mock(pid=123, poll=Mock(return_value=None))
            try:
                with patch.object(self.runner.subprocess, "Popen", return_value=process) as launch:
                    stack.start_process("worker", "worker", {})
                self.assertEqual(launch.call_args.kwargs["env"].get("HIVEMIND_DISABLE_OPEN_UI"), "1")
            finally:
                for log in stack.logs:
                    log.close()

    def test_workspace_stops_dotenv_search_before_any_service_starts(self):
        with tempfile.TemporaryDirectory() as temporary:
            folder = Path(temporary)
            isolated = folder / "workspace"
            isolated.mkdir()
            stack = self.runner.LocalStack(folder / "binary.exe", folder / "report", 10, "192.0.2.1")
            stack.command = Mock(side_effect=RuntimeError("stop before Docker"))
            with patch.object(self.runner.tempfile, "mkdtemp", return_value=str(isolated)), \
                    patch.object(self.runner, "unused_ports", return_value=list(range(30000, 30012))):
                with self.assertRaisesRegex(RuntimeError, "stop before Docker"):
                    stack.start()
            dotenv = isolated / ".env"
            self.assertTrue(dotenv.is_file(), "a local empty .env must block ancestor configuration loading")
            self.assertEqual(dotenv.read_text(encoding="utf-8"), "")

    def test_summary_requires_every_task_settled_and_keeps_per_stage_metrics(self):
        pipeline = {"stages": [{"items": [{"source": "x"}, {"source": "x"}]}, {"items": [{"source": "y"}]}]}
        tasks = []
        timings = {}
        for index, (stage, item, output) in enumerate([(1, 1, 6), (1, 2, 7), (2, 1, 13)]):
            task_id = f"smoke-{stage}-{item}"
            tasks.append({"task_id": task_id, "status": "COMPLETED", "billing_settled": True,
                          "billed_amount": 33 + index, "usage_units": 30, "max_cpt": 100,
                          "wall_time_ms": 2, "output": json.dumps(output),
                          "managed_consensus": {"replica_count": 3, "required_quorum": 2,
                                                "matching_votes": 3, "settlement_authorized": True}})
            timings[task_id] = {"created_at": "2026-11-12T00:00:00Z", "completed_at": "2026-11-12T00:00:01Z", "latency_ms": 1000}
        summary = self.runner.summarize_run(pipeline, tasks, "smoke", timings, 2.5)
        self.assertEqual(summary["final_output"], [13])
        self.assertEqual(summary["billed_cpt"], 102)
        self.assertEqual([stage["usage_units"] for stage in summary["stages"]], [60, 30])
        self.assertEqual(summary["tasks"][0]["latency_ms"], 1000)
        with self.assertRaisesRegex(ValueError, "missing"):
            self.runner.summarize_run(pipeline, tasks[:-1], "smoke", timings, 2.5)
        tasks[0]["billing_settled"] = False
        with self.assertRaisesRegex(ValueError, "settled"):
            self.runner.summarize_run(pipeline, tasks, "smoke", timings, 2.5)

    def test_summary_does_not_replace_missing_usage_with_zero(self):
        pipeline = {"stages": [{"items": [{"source": "x"}]}]}
        task = {"task_id": "smoke-1-1", "status": "COMPLETED", "billing_settled": True,
                "billed_amount": 20, "output": "1", "managed_consensus": {
                    "replica_count": 3, "required_quorum": 2, "matching_votes": 2,
                    "settlement_authorized": True}}
        with self.assertRaisesRegex(ValueError, "usage"):
            self.runner.summarize_run(pipeline, [task], "smoke", {}, 1.0)

    def test_evidence_requires_successful_positive_usage_reports_from_all_workers(self):
        with tempfile.TemporaryDirectory() as temporary:
            stack = self.runner.LocalStack(Path("binary.exe"), Path(temporary), 10, "192.0.2.1")
            reports = [{"worker_id": f"phase3-worker-{index}", "success": True,
                        "usage_units": 10, "reported_at": "2026-11-12T00:00:01Z"}
                       for index in range(1, 4)]
            artifacts = [{"complete": True, "size_bytes": 128}]
            for invalid in ({"success": False}, {"success": None}, {"usage_units": 0},
                            {"usage_units": None}, {"usage_units": True}, {"reported_at": None}):
                with self.subTest(invalid=invalid):
                    incomplete = [dict(report) for report in reports]
                    incomplete[2].update(invalid)
                    stack.sql = Mock(side_effect=[json.dumps(incomplete), json.dumps(artifacts)])
                    with self.assertRaisesRegex(ValueError, "successful positive-usage report"):
                        stack.evidence()

            # A failed attempt does not invalidate a worker's later successful report.
            reports.append({"worker_id": "phase3-worker-3", "success": False,
                            "usage_units": None, "reported_at": None})
            stack.sql = Mock(side_effect=[json.dumps(reports), json.dumps(artifacts)])
            summary = stack.evidence()
            self.assertEqual(summary["replica_rows"], 4)
            self.assertEqual(summary.get("workers_with_successful_reports"), 3)

    def test_cleanup_attempts_all_owned_containers_even_if_one_removal_fails(self):
        with tempfile.TemporaryDirectory() as temporary:
            stack = self.runner.LocalStack(Path("binary.exe"), Path(temporary), 10, "192.0.2.1")
            stack.containers = ["a" * 64, "b" * 64]
            stack.command = Mock(side_effect=[RuntimeError("docker unavailable"), "removed"])
            with self.assertRaisesRegex(RuntimeError, "cleanup"):
                stack.close()
            self.assertEqual(stack.command.call_count, 2)
            self.assertEqual(stack.command.call_args_list[0].args[0][-1], "b" * 64)
            self.assertEqual(stack.command.call_args_list[1].args[0][-1], "a" * 64)


if __name__ == "__main__":
    unittest.main()
