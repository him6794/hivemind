"""Independent scalar Python reference, without hcode, tensor libraries, or NumPy."""

import argparse
import json
import math
from pathlib import Path


def probabilities(features, weights):
    logits = [sum(value * weights[row][label] for row, value in enumerate(features))
              for label in range(len(weights[0]))]
    largest = max(logits)
    exponentials = [math.exp(value - largest) for value in logits]
    denominator = sum(exponentials)
    return [value / denominator for value in exponentials]


def update(weights, params):
    weights = [row[:] for row in weights]
    for _ in range(params["local_steps"]):
        gradient = [[0.0 for _ in row] for row in weights]
        for features, target in zip(params["x"], params["y"], strict=True):
            predicted = probabilities(features, weights)
            for row, value in enumerate(features):
                for label in range(len(target)):
                    gradient[row][label] += value * (predicted[label] - target[label])
        for row in range(len(weights)):
            for label in range(len(weights[row])):
                weights[row][label] -= gradient[row][label] / len(params["x"]) * params["learning_rate"]
    return weights


def run(pipeline, directory):
    weights = pipeline["input"]["weights"]
    history = list(pipeline["input"]["loss_history"])
    stages = pipeline["stages"]
    for index in range(0, len(stages), 2):
        updates = []
        for item in stages[index]["items"]:
            params = json.loads((directory / item["params_file"]).read_text(encoding="utf-8"))
            updates.append(update(weights, params))
        weights = [[sum(update[row][label] for update in updates) / len(updates)
                    for label in range(len(weights[0]))] for row in range(len(weights))]
        evaluation = json.loads((directory / stages[index + 1]["items"][0]["params_file"]).read_text(encoding="utf-8"))
        loss = 0.0
        for features, target in zip(evaluation["x"], evaluation["y"], strict=True):
            predicted = probabilities(features, weights)
            loss -= sum(math.log(predicted[label]) for label, value in enumerate(target) if value > 0.0)
        history.append(loss / len(evaluation["x"]))
    return {"weights": weights, "loss_history": history}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("pipeline", nargs="?", type=Path, default=Path(__file__).with_name("pipeline.json"))
    options = parser.parse_args()
    pipeline = json.loads(options.pipeline.read_text(encoding="utf-8"))
    print(json.dumps(run(pipeline, options.pipeline.parent), indent=2, allow_nan=False))
