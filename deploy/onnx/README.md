# Operator-pinned ONNX guest

This directory contains the fixed runner used inside an operator-built OCI
rootfs. It is not a Worker-side inference dependency. The operator must provide
an ONNX Runtime wheel (CPU, CUDA, or TensorRT), a base image pinned by digest,
and the matching CUDA/TensorRT libraries when a GPU provider is selected.

The runner expects:

- `/work/source`: one ONNX protobuf model;
- `/work/input-0`, `/work/input-1`, ...: contiguous, ordered, read-only
  `hivemind-onnx-tensor-v1` JSON envelopes;
- `/etc/hivemind/onnx/provider`: the image-owned provider name `cpu`, `cuda`, or
  `tensorrt`.

The input mount numbering is part of the operator backend contract; arbitrary
`/work/input-*` names, gaps, and non-canonical numeric suffixes are rejected.

The OCI backend policy must mount the source and inputs at those operator-owned
locations. The runner discovers only regular `/work/input-*` files, validates
names, dtypes, shapes, finite values, and graph compatibility, then emits one
`general-compute-result-v1` envelope. Runtime logs go to stderr. Output tensor
bytes are bounded inline artifacts because the current Worker does not yet
upload files from an output mount.

The operator must include reviewed `onnx`, `protobuf`, `numpy`, and the selected
ONNX Runtime/provider wheels (plus their dependencies) under `vendor/`. The
runner parses the model before creating an inference session and rejects any
`external_data` initializer. For accelerator routes it configures
`session.disable_cpu_ep_fallback` and requires the configured provider to be
active; the only additional provider ORT may report is its default
`CPUExecutionProvider`, which is explicitly disabled for graph fallback. If
session initialization or execution would assign a node to CPU, ORT rejects the
session instead of completing it. This prevents ONNX Runtime graph partitioning
from silently placing unsupported nodes on CPU. This protocol accepts only
self-contained ONNX protobufs; external data cannot be used to make the guest
read files from its root filesystem.

Build requirements are deliberately explicit rather than downloading native
libraries during a build. Place the operator-reviewed wheel and its dependency
wheels under `vendor/`, choose a digest-pinned base image, and build with a
reproducible Docker/BuildKit configuration. Record the resulting image digest
in `ProductionBackendConfig.guest_image_digest` and set `runner_sha256` to the
fixed executable digest.

The runner has been exercised against a locally built CUDA 11.8 / ONNX Runtime
1.16.3 smoke image with Docker GPU passthrough and a generated Add model. This
proves provider activation and CUDA execution for the smoke environment; it is
not a substitute for the operator's production Linux rootless OCI image,
capability registration, or Nodepool settlement verification.
