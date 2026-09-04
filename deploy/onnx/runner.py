#!/usr/bin/env python3
"""Fixed operator-owned ONNX runner for the general-compute OCI guest.

The image, provider selection, and mount layout are operator-owned. This
process writes exactly one ProductionResultEnvelope to stdout; diagnostics go
to stderr and a non-zero exit status is used for failures.
"""

from __future__ import annotations

import hashlib
import json
import struct
import sys
from pathlib import Path
from typing import NoReturn

import numpy as np
import onnx
import onnxruntime as ort
from google.protobuf.message import Message

TENSOR_PROTOCOL = "hivemind-onnx-tensor-v1"
RESULT_PROTOCOL = "general-compute-result-v1"
PROVIDER_FILE = Path("/etc/hivemind/onnx/provider")
WORK_ROOT = Path("/work")
SOURCE_NAME = "source"
INPUT_GLOB = "input-*"
MAX_TENSOR_NAME_LENGTH = 128
MAX_TENSOR_RANK = 8
MAX_TENSOR_ELEMENTS = 16_777_216
MAX_TENSOR_BYTES = 64 * 1024 * 1024

DTYPES = {
    "float32": np.dtype("<f4"),
    "float64": np.dtype("<f8"),
    "int32": np.dtype("<i4"),
    "int64": np.dtype("<i8"),
    "uint8": np.dtype("u1"),
    "bool": np.dtype("u1"),
}
ORT_TYPES = {
    "tensor(float)": "float32",
    "tensor(double)": "float64",
    "tensor(int32)": "int32",
    "tensor(int64)": "int64",
    "tensor(uint8)": "uint8",
    "tensor(bool)": "bool",
}


def fail(message: str) -> "NoReturn":
    print(message, file=sys.stderr)
    raise SystemExit(1)


def compact_json(value: object) -> bytes:
    return json.dumps(value, ensure_ascii=False, separators=(",", ":")).encode("utf-8")


def sha256_digest(data: bytes) -> str:
    return "sha256:" + hashlib.sha256(data).hexdigest()


def reject_external_data(source: bytes) -> None:
    """Reject ONNX external initializers before ONNX Runtime can read paths."""
    try:
        model = onnx.ModelProto()
        model.ParseFromString(source)
    except Exception as exc:
        fail(f"ONNX model is invalid: {exc}")

    def tensors(value: Message):
        if value.DESCRIPTOR.full_name == "onnx.TensorProto":
            yield value
        for field, nested in value.ListFields():
            if not field.message_type:
                continue
            if field.label == field.LABEL_REPEATED:
                for item in nested:
                    if isinstance(item, Message):
                        yield from tensors(item)
            elif isinstance(nested, Message):
                yield from tensors(nested)

    for tensor in tensors(model):
        if tensor.external_data or tensor.data_location == onnx.TensorProto.EXTERNAL:
            fail("ONNX external tensor data is unsupported")


def canonical_input_digest(source: bytes, inputs: list[bytes]) -> str:
    payload = bytearray(b"general-compute-input-v1\0")
    payload.extend(struct.pack(">Q", len(source)))
    payload.extend(source)
    payload.extend(struct.pack(">Q", len(inputs)))
    for value in inputs:
        payload.extend(struct.pack(">Q", len(value)))
        payload.extend(value)
    return sha256_digest(bytes(payload))


def canonical_artifact_root(artifacts: list[dict[str, object]]) -> str:
    canonical = [
        {
            "artifact_id": artifact["artifact_id"],
            "role": artifact["role"],
            "size_bytes": artifact["size_bytes"],
            "mime_type": artifact["mime_type"],
            "sha256": artifact["sha256"],
            "chunks": artifact["chunks"],
        }
        for artifact in artifacts
    ]
    return sha256_digest(compact_json(canonical))


def validate_tensor_name(name: object, context: str) -> str:
    if (
        not isinstance(name, str)
        or not name
        or len(name) > MAX_TENSOR_NAME_LENGTH
        or any(char not in "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_.-" for char in name)
    ):
        fail(f"tensor name is invalid: {context}")
    return name


def decode_tensor(path: Path) -> tuple[dict[str, object], np.ndarray]:
    raw = path.read_bytes()
    if len(raw) > MAX_TENSOR_BYTES * 2:
        fail(f"tensor payload is too large: {path.name}")
    try:
        envelope = json.loads(raw.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        fail(f"invalid tensor envelope {path.name}: {exc}")
    if not isinstance(envelope, dict) or set(envelope) != {
        "protocol_version", "name", "dtype", "shape", "data_hex"
    }:
        fail(f"tensor envelope fields are invalid: {path.name}")
    if compact_json(envelope) != raw:
        fail(f"tensor envelope is not canonical: {path.name}")
    if envelope["protocol_version"] != TENSOR_PROTOCOL:
        fail(f"tensor protocol is unsupported: {path.name}")
    name = validate_tensor_name(envelope["name"], path.name)
    dtype_name = envelope["dtype"]
    shape = envelope["shape"]
    data_hex = envelope["data_hex"]
    if (
        not isinstance(dtype_name, str)
        or dtype_name not in DTYPES
        or not isinstance(shape, list)
        or any(
            not isinstance(dim, int) or isinstance(dim, bool) or dim < 0
            for dim in shape
        )
    ):
        fail(f"tensor dtype or shape is invalid: {path.name}")
    if len(shape) > MAX_TENSOR_RANK:
        fail(f"tensor rank is unsupported: {path.name}")
    if not isinstance(data_hex, str) or len(data_hex) % 2 or any(
        char not in "0123456789abcdef" for char in data_hex
    ):
        fail(f"tensor data is not canonical lowercase hex: {path.name}")
    try:
        payload = bytes.fromhex(data_hex)
    except (TypeError, ValueError):
        fail(f"tensor data is not valid hex: {path.name}")
    elements = 1
    for dimension in shape:
        elements *= dimension
    if elements > 16_777_216 or len(payload) > MAX_TENSOR_BYTES:
        fail(f"tensor shape or payload is too large: {path.name}")
    expected = elements * DTYPES[dtype_name].itemsize
    if len(payload) != expected:
        fail(f"tensor byte length does not match shape: {path.name}")
    if dtype_name == "bool" and any(byte not in (0, 1) for byte in payload):
        fail(f"tensor bool payload is invalid: {path.name}")
    array = np.frombuffer(payload, dtype=DTYPES[dtype_name]).reshape(tuple(shape))
    if dtype_name in ("float32", "float64") and not np.isfinite(array).all():
        fail(f"tensor contains a non-finite value: {path.name}")
    return envelope, array


def encode_tensor(name: str, array: np.ndarray, dtype_name: str) -> bytes:
    validate_tensor_name(name, name)
    if dtype_name not in DTYPES:
        fail(f"unsupported ONNX output type: {dtype_name}")
    converted = np.ascontiguousarray(array, dtype=DTYPES[dtype_name])
    if converted.ndim > MAX_TENSOR_RANK or converted.size > MAX_TENSOR_ELEMENTS:
        fail(f"ONNX output shape is too large: {name}")
    if dtype_name in ("float32", "float64") and not np.isfinite(converted).all():
        fail(f"ONNX output contains a non-finite value: {name}")
    if dtype_name == "bool" and not np.isin(converted, (0, 1)).all():
        fail(f"ONNX bool output is invalid: {name}")
    envelope = {
        "protocol_version": TENSOR_PROTOCOL,
        "name": name,
        "dtype": dtype_name,
        "shape": list(converted.shape),
        "data_hex": converted.tobytes(order="C").hex(),
    }
    encoded = compact_json(envelope)
    if len(encoded) > MAX_TENSOR_BYTES:
        fail(f"ONNX output is too large: {name}")
    return encoded


def provider_name() -> str:
    try:
        value = PROVIDER_FILE.read_text(encoding="ascii").strip().lower()
    except OSError as exc:
        fail(f"provider configuration is unavailable: {exc}")
    if value == "cpu":
        return "CPUExecutionProvider"
    if value == "cuda":
        return "CUDAExecutionProvider"
    if value == "tensorrt":
        return "TensorrtExecutionProvider"
    fail("provider configuration is invalid")


def input_index(path: Path) -> int:
    prefix = "input-"
    suffix = path.name[len(prefix) :]
    if not path.name.startswith(prefix) or not suffix or not all(
        "0" <= char <= "9" for char in suffix
    ):
        fail(f"ONNX input mount name is invalid: {path.name}")
    index = int(suffix)
    if str(index) != suffix:
        fail(f"ONNX input mount name is not canonical: {path.name}")
    return index


def main() -> None:
    source = (WORK_ROOT / SOURCE_NAME).read_bytes()
    input_paths = sorted(
        (path for path in WORK_ROOT.glob(INPUT_GLOB) if path.is_file()),
        key=input_index,
    )
    input_indices = [input_index(path) for path in input_paths]
    if input_indices != list(range(len(input_paths))):
        fail("ONNX input mounts must be the contiguous input-0..input-N sequence")
    input_raw = [path.read_bytes() for path in input_paths]
    tensors = [decode_tensor(path) for path in input_paths]
    if len(tensors) == 0:
        fail("at least one ONNX input tensor is required")

    provider = provider_name()
    reject_external_data(source)
    available = ort.get_available_providers()
    if provider not in available:
        fail(f"configured ONNX provider is unavailable: {provider}")
    try:
        session_options = ort.SessionOptions()
        # Graph partitioning may otherwise place unsupported nodes on the CPU
        # execution provider even when the requested accelerator initializes.
        session_options.add_session_config_entry(
            "session.disable_cpu_ep_fallback", "1"
        )
        session = ort.InferenceSession(
            source,
            sess_options=session_options,
            providers=[provider],
        )
    except Exception as exc:
        fail(f"configured ONNX provider failed to initialize: {exc}")
    active_providers = session.get_providers()
    if provider not in active_providers:
        fail(f"configured ONNX provider was not activated: {provider}")
    unexpected_providers = [
        active for active in active_providers
        if active not in (provider, "CPUExecutionProvider")
    ]
    if unexpected_providers:
        fail(
            "configured ONNX session activated unexpected providers: "
            f"{unexpected_providers}"
        )
    graph_inputs = session.get_inputs()
    if len(graph_inputs) != len(tensors):
        fail("ONNX graph input count does not match configured tensor mounts")

    feed: dict[str, np.ndarray] = {}
    seen: set[str] = set()
    for graph_input, (envelope, array) in zip(graph_inputs, tensors, strict=True):
        name = envelope["name"]
        if name in seen or name != graph_input.name:
            fail(f"ONNX input name does not match graph: {name}")
        seen.add(name)
        expected_dtype = ORT_TYPES.get(graph_input.type)
        if expected_dtype != envelope["dtype"]:
            fail(f"ONNX input dtype does not match graph: {name}")
        for actual, expected in zip(array.shape, graph_input.shape, strict=False):
            if isinstance(expected, int) and expected > 0 and actual != expected:
                fail(f"ONNX input shape does not match graph: {name}")
        feed[name] = array

    outputs = session.run(None, feed)
    output_artifacts: list[dict[str, object]] = []
    output_names = [value.name for value in session.get_outputs()]
    for index, (name, value) in enumerate(zip(output_names, outputs, strict=True)):
        expected_dtype = ORT_TYPES.get(session.get_outputs()[index].type)
        if expected_dtype is None:
            fail(f"unsupported ONNX output type: {name}")
        tensor_bytes = encode_tensor(name, np.asarray(value), expected_dtype)
        output_artifacts.append(
            {
                "artifact_id": f"output-{index}",
                "role": "output",
                "size_bytes": len(tensor_bytes),
                "mime_type": "application/vnd.hivemind.onnx.tensor+json",
                "sha256": sha256_digest(tensor_bytes),
                "chunks": [],
                "inline_bytes": list(tensor_bytes),
            }
        )

    output_bytes = sum(int(value["size_bytes"]) for value in output_artifacts)
    result = {
        "protocol_version": RESULT_PROTOCOL,
        "status": "completed",
        "exit_code": 0,
        "error_code": None,
        "stdout": json.dumps({"outputs": output_names}, separators=(",", ":")),
        "stderr": "",
        "output_artifacts": output_artifacts,
        "usage": {
            "cpu_time_ms": 0,
            "wall_time_ms": 0,
            "peak_memory_bytes": 0,
            "input_bytes": sum(len(value) for value in input_raw),
            "output_bytes": output_bytes,
            "gpu_time_ms": 0,
            "gpu_memory_bytes": 0,
        },
        "input_sha256": canonical_input_digest(source, input_raw),
        "output_manifest_root": canonical_artifact_root(output_artifacts),
    }
    print(json.dumps(result, ensure_ascii=False, separators=(",", ":")))


if __name__ == "__main__":
    main()
