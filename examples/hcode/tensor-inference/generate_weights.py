"""Create a deterministic safetensors fixture using only Python's standard library."""
import json
from pathlib import Path
import struct
import sys


def model_bytes():
    # A tensor larger than the JSON/decoded-value limit, with a small useful slice.
    header = json.dumps(
        {"weights": {"dtype": "F32", "shape": [300000], "data_offsets": [0, 1200000]}},
        separators=(",", ":"),
    ).encode("utf-8")
    header += b" " * (-len(header) % 8)
    return (
        struct.pack("<Q", len(header))
        + header
        + bytes(1200000 - 16)
        + struct.pack("<4f", 1, 2, 3, 4)
    )


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit("Usage: python generate_weights.py <new-output.safetensors>")
    output = Path(sys.argv[1])
    data = model_bytes()
    with output.open("xb") as file:
        file.write(data)
    print(f"Wrote {len(data)} bytes to {output}")
