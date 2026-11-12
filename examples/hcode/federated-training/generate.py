"""Generate a tiny, deterministic FedAvg pipeline using only Python's standard library."""

import argparse
import json
import math
from pathlib import Path


HERE = Path(__file__).resolve().parent


def synthetic_shard(index):
    """Two examples per class; the constant third feature represents the bias."""
    centers = [(-1.0, -1.0), (1.0, -1.0), (0.0, 1.0)]
    features, targets = [], []
    for label, (center_x, center_y) in enumerate(centers):
        for sample in range(2):
            # Rational offsets avoid relying on a PRNG implementation/version.
            offset_x = ((index * 7 + label * 3 + sample * 5) % 9 - 4) / 20.0
            offset_y = ((index * 5 + label * 7 + sample * 3) % 9 - 4) / 20.0
            features.append([center_x + offset_x, center_y + offset_y, 1.0])
            targets.append([float(candidate == label) for candidate in range(3)])
    return {"x": features, "y": targets, "learning_rate": 0.4, "local_steps": 3}


def write_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, allow_nan=False) + "\n", encoding="utf-8", newline="\n")


def generate(directory, rounds):
    directory.mkdir(parents=True, exist_ok=True)
    for name in ("train.hcode", "average.hcode"):
        source = (HERE / name).read_text(encoding="utf-8")
        (directory / name).write_text(source, encoding="utf-8", newline="\n")
    shards = [synthetic_shard(index) for index in range(3)]
    for index, shard in enumerate(shards, start=1):
        write_json(directory / "params" / f"shard-{index}.json", shard)
    evaluation = {
        "x": [row for shard in shards for row in shard["x"]],
        "y": [row for shard in shards for row in shard["y"]],
    }
    write_json(directory / "params" / "evaluation.json", evaluation)
    stages = []
    for _ in range(rounds):
        stages.append({"items": [
            {"source": "train.hcode", "params_file": f"params/shard-{index}.json"}
            for index in range(1, 4)
        ]})
        stages.append({"items": [{"source": "average.hcode", "params_file": "params/evaluation.json"}]})
    pipeline = {
        "input": {"weights": [[0.0] * 3 for _ in range(3)], "loss_history": [math.log(3.0)]},
        "stages": stages,
    }
    write_json(directory / "pipeline.json", pipeline)
    return pipeline


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out-dir", type=Path, default=HERE)
    parser.add_argument("--rounds", type=int, default=4)
    options = parser.parse_args()
    if not 1 <= options.rounds <= 100:
        parser.error("rounds must be between 1 and 100")
    generate(options.out_dir, options.rounds)
    print(f"Wrote {options.rounds} FedAvg rounds (3 equal shards) to {options.out_dir / 'pipeline.json'}")


if __name__ == "__main__":
    main()
