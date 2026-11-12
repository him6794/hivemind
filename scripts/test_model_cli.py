"""Exercise model import, generated files, and pipeline execution through the CLI.

Usage: python scripts/test_model_cli.py path/to/hivemind-bin[.exe]
Uses only the Python standard library and writes into a temporary directory.
"""

import json
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import unittest


BINARY = Path(sys.argv.pop(1)).resolve() if len(sys.argv) > 1 else None


class ModelCliTest(unittest.TestCase):
    def invoke(self, *args):
        return subprocess.run(
            [str(BINARY), *map(str, args)], text=True, capture_output=True, timeout=60
        )

    def test_import_split_and_check_generated_pipeline(self):
        self.assertIsNotNone(BINARY, "pass the hivemind executable as the first argument")
        with tempfile.TemporaryDirectory(prefix="hivemind-model-cli-") as directory:
            root = Path(directory)
            header = {
                "0.weight": {"dtype": "F32", "shape": [2, 2], "data_offsets": [0, 16]},
                "0.bias": {"dtype": "F32", "shape": [2], "data_offsets": [16, 24]},
            }
            encoded = json.dumps(header, separators=(",", ":")).encode()
            weights = root / "weights.safetensors"
            weights.write_bytes(struct.pack("<Q", len(encoded)) + encoded + struct.pack("<6f", 1, 2, 3, 4, 0.5, -0.5))
            model = root / "model.json"
            imported = self.invoke("model", "init", weights, "--final", "relu", "--out", model)
            self.assertEqual(imported.returncode, 0, imported.stderr + imported.stdout)
            description = json.loads(model.read_text())
            self.assertEqual(description["layers"][-1], {"type": "relu"})
            output = root / "split"
            split = self.invoke("model", "split", model, "--weights", weights, "--input", "[[1,2],[3,4]]", "--out-dir", output)
            self.assertEqual(split.returncode, 0, split.stderr + split.stdout)
            pipeline = json.loads((output / "pipeline.json").read_text())
            self.assertGreater(pipeline["stages"][0]["items"][0]["max_cpt"], 0)
            checked = self.invoke("pipeline", "check", output / "pipeline.json")
            self.assertEqual(checked.returncode, 0, checked.stderr + checked.stdout)
            self.assertIn("[[5.5,10.5],[11.5,24.5]]", checked.stdout.replace(" ", ""))
            repeated = self.invoke("model", "split", model, "--weights", weights, "--input", "[[1,2]]", "--out-dir", output)
            self.assertNotEqual(repeated.returncode, 0, "existing generated files must not be overwritten")


if __name__ == "__main__":
    unittest.main()
