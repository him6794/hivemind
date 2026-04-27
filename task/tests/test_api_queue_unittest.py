import importlib.util
import tempfile
import unittest
from pathlib import Path


def load_task_api_module():
    repo_root = Path(__file__).resolve().parents[2]
    api_path = repo_root / "task" / "api.py"
    spec = importlib.util.spec_from_file_location("task_api", api_path)
    module = importlib.util.module_from_spec(spec)
    assert spec and spec.loader
    spec.loader.exec_module(module)
    return module


class TestApiQueue(unittest.TestCase):
    def setUp(self):
        self.api = load_task_api_module()

        # Redirect persistence to a temp folder to avoid touching real files.
        self.tmp = tempfile.TemporaryDirectory()
        tmp_path = Path(self.tmp.name)
        self.api.STATE_FILE = tmp_path / "task_state.json"
        self.api.QUEUE_FILE = tmp_path / "job_queue.json"
        self.api.RUN_RESULTS_FILE = tmp_path / "run_results.ndjson"

        # Reset globals
        with self.api.lock:
            self.api.job_queue = []
            self.api.current_run = None
            self.api._run_seq = 0
            self.api.max_active_workers = None
            self.api.active_workers = set()

            self.api.next_node_id = 1
            self.api.worker_to_node = {}
            self.api.nodes = {}

            self.api.in_flight = {}
            self.api.completed_task_ids = set()
            self.api.task_cursors = {}

            self.api.remaining_chunks = []
            self.api.num_chunks = 0
            self.api.tasks_completed = 0
            self.api.task_running = True  # prevent auto-start during enqueue
            self.api.task_initialized = True

            self.api.power_n = None
            self.api.start_time = None
            self.api.total_numbers = 0
            self.api.processed_numbers = 0
            self.api.buffered_primes_count = 0

        self.client = self.api.app.test_client()

    def tearDown(self):
        self.tmp.cleanup()

    def test_enqueue_experiment_grid_counts(self):
        resp = self.client.post(
            "/enqueue_experiment_grid",
            json={
                "n_values": [5, 6],
                "worker_values": [1, 10],
                "chunk_values": [50],
                "repeats": 3,
                "clear_queue": True,
            },
        )
        self.assertEqual(resp.status_code, 200)
        data = resp.get_json()
        self.assertEqual(data["combos"], 2 * 2 * 1)
        self.assertEqual(data["enqueued"], 2 * 2 * 1 * 3)

        # Should not auto-start because task_running=True
        self.assertFalse(data["started_now"])

    def test_worker_limit_blocks_extra_workers(self):
        with self.api.lock:
            self.api.max_active_workers = 1
            self.api.remaining_chunks = [{"task_id": "t1", "start": 1, "end": 10}]
            self.api.num_chunks = 1
            self.api.task_running = True
            self.api.task_initialized = True

        # Must register nodes first (API enforces registration before task assignment)
        rreg1 = self.client.post('/register_node', json={'worker_id': 'worker-a'})
        self.assertEqual(rreg1.status_code, 200)
        rreg2 = self.client.post('/register_node', json={'worker_id': 'worker-b'})
        self.assertEqual(rreg2.status_code, 200)

        r1 = self.client.get("/get_task?worker_id=worker-a")
        self.assertEqual(r1.status_code, 200)

        r2 = self.client.get("/get_task?worker_id=worker-b")
        self.assertEqual(r2.status_code, 204)

    def test_enqueue_runs_has_repeat_index(self):
        resp = self.client.post(
            "/enqueue_runs",
            json={"n": 5, "chunks": 50, "repeats": 5, "desired_workers": 1},
        )
        self.assertEqual(resp.status_code, 200)
        data = resp.get_json()
        self.assertEqual(data["queued"], 5)

        with self.api.lock:
            self.assertEqual(len(self.api.job_queue), 5)
            self.assertEqual(self.api.job_queue[0]["rep_index"], 1)
            self.assertEqual(self.api.job_queue[-1]["rep_index"], 5)
            self.assertEqual(self.api.job_queue[0]["rep_total"], 5)


if __name__ == "__main__":
    unittest.main()
