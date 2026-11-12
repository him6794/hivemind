"""Run with: python -m unittest discover -s examples/hcode/federated-training."""

import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


EXAMPLE = Path(__file__).resolve().parent


class GeneratorTests(unittest.TestCase):
    def generate(self, directory, *arguments):
        result = subprocess.run(
            [sys.executable, str(EXAMPLE / "generate.py"), "--out-dir", str(directory), *arguments],
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        return json.loads((directory / "pipeline.json").read_text(encoding="utf-8"))

    def test_default_has_three_equal_shards_and_four_training_rounds(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            pipeline = self.generate(directory)
            self.assertEqual(len(pipeline["stages"]), 8)
            self.assertEqual([len(stage["items"]) for stage in pipeline["stages"]], [3, 1] * 4)
            sizes = []
            for item in pipeline["stages"][0]["items"]:
                params = json.loads((directory / item["params_file"]).read_text(encoding="utf-8"))
                sizes.append(len(params["x"]))
                self.assertEqual(len(params["x"]), len(params["y"]))
                self.assertTrue(all(len(row) == 3 and row[-1] == 1.0 for row in params["x"]))
                self.assertTrue(all(sum(row) == 1.0 for row in params["y"]))
            self.assertEqual(sizes, [6, 6, 6])

    def test_generation_is_deterministic_and_matches_committed_example(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            self.generate(directory)
            for path in directory.rglob("*"):
                if path.is_file():
                    expected = EXAMPLE / path.relative_to(directory)
                    self.assertTrue(expected.is_file(), f"missing committed fixture {expected}")
                    self.assertEqual(path.read_bytes(), expected.read_bytes(), str(expected))

    def test_one_round_and_invalid_round_count(self):
        with tempfile.TemporaryDirectory() as temporary:
            pipeline = self.generate(Path(temporary), "--rounds", "1")
            self.assertEqual(len(pipeline["stages"]), 2)
            result = subprocess.run(
                [sys.executable, str(EXAMPLE / "generate.py"), "--out-dir", temporary, "--rounds", "0"],
                capture_output=True, text=True, check=False,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("rounds must be between", result.stderr)

    def test_scalar_reference_matches_fixed_expected_values_and_loss_decreases(self):
        reference_path = EXAMPLE / "reference.py"
        self.assertTrue(reference_path.is_file(), "the independent scalar reference is missing")
        spec = importlib.util.spec_from_file_location("federated_reference", reference_path)
        reference = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(reference)
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            pipeline = self.generate(directory)
            expected = json.loads((EXAMPLE / "expected.json").read_text(encoding="utf-8"))
            result = reference.run(pipeline, directory)
            for actual, fixed in zip(result["loss_history"], expected["loss_history"], strict=True):
                self.assertAlmostEqual(actual, fixed, places=12)
            for actual_row, fixed_row in zip(result["weights"], expected["weights"], strict=True):
                for actual, fixed in zip(actual_row, fixed_row, strict=True):
                    self.assertAlmostEqual(actual, fixed, places=12)
            self.assertTrue(all(after < before for before, after in zip(result["loss_history"], result["loss_history"][1:])))


if __name__ == "__main__":
    unittest.main()
