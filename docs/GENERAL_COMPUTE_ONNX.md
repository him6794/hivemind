# Operator-pinned ONNX inference

Hivemind's ONNX path uses the existing `general-compute-v1alpha1` production OCI
backend. The Worker does not load ONNX Runtime, TensorRT, CUDA libraries, or
task-provided native code. Those components belong in an operator-built guest
image whose digest and runner executable are pinned in the production backend
registry.

## Backend registration

A production registration may include an `onnx` section:

```json
{
  "backend_id": "onnx-cuda-ort",
  "guest_image_digest": "sha256:<operator-pinned-guest-image>",
  "bundle_root": "/var/lib/hivemind/onnx/bundles",
  "artifact_root": "/var/lib/hivemind/onnx/artifacts",
  "runner_executable": "/usr/bin/runc",
  "runner_state_root": "/var/lib/hivemind/onnx/runc-state",
  "seccomp_profile_path": "/etc/hivemind/onnx/seccomp.json",
  "runner_prefix_args": [],
  "runner_sha256": "sha256:<operator-pinned-runner>",
  "entrypoint": ["/opt/hivemind/onnx-runner"],
  "policy": { "...": "the normal validated Linux OCI policy" },
  "gpu_device_mappings": [
    {
      "device_id": "gpu-0",
      "devices": [
        { "path": "/dev/nvidia0", "device_type": "char", "major": 195, "minor": 0, "access": "rw" },
        { "path": "/dev/nvidiactl", "device_type": "char", "major": 195, "minor": 255, "access": "rwm" },
        { "path": "/dev/nvidia-uvm", "device_type": "char", "major": 511, "minor": 0, "access": "rwm" }
      ]
    }
  ],
  "onnx": {
    "protocol_version": "general-compute-onnx-v1",
    "model_artifact_id": "source",
    "input_artifact_ids": ["input-0"],
    "execution_provider": "cuda"
  },
  "max_output_bytes": 16777216
}
```

The device list is operator-owned. A task cannot submit a `/dev` path or select
a device outside the configured mapping. CUDA and TensorRT registrations
require a trusted CUDA GPU selection; CPU registrations reject a GPU selection.

## Tensor ABI

The model is the request's `source` artifact and must be an ONNX protobuf.
External-data sidecar files are not part of `general-compute-onnx-v1`; a model
that requires them must be repackaged into one operator-approved source artifact.
The fixed runner parses the protobuf with external-data loading disabled and
rejects every `external_data` initializer before invoking ONNX Runtime. It also
checks the providers reported by the created session and rejects a configured
CUDA/TensorRT provider if ONNX Runtime silently fell back to CPU. This prevents
a task-controlled model from making the guest runtime resolve arbitrary paths
relative to its OCI working directory, and prevents a GPU-admitted task from
being reported as GPU-backed when only CPU execution was activated.
Input tensor artifacts use the canonical JSON envelope below, and output tensor
artifacts use the same envelope:

```json
{
  "protocol_version": "hivemind-onnx-tensor-v1",
  "name": "input_0",
  "dtype": "float32",
  "shape": [1, 3, 224, 224],
  "data_hex": "<little-endian tensor bytes as lowercase hex>"
}
```

The envelope is serialized without whitespace and must use exactly these fields.
Tensor names are ASCII `[A-Za-z0-9_.-]`, at most 128 bytes, and must match the
operator-pinned model input/output names. The runner applies the same name, rank,
element-count, and payload-size limits to output tensors; it never emits an
output envelope that the shared Rust validator would reject. Supported dtypes are
`float32`,
`float64`, `int32`, `int64`, `uint8`, and `bool`; numeric values are
little-endian, and floating-point NaN/Infinity plus boolean values other than
`0` or `1` are rejected. Rank is at most 8, dimensions are explicit (including
the batch dimension), and there is no implicit batching. The shared validator
bounds a tensor at 16,777,216 elements and 64 MiB of payload bytes.

Input artifacts remain in the exact order declared by the ONNX backend
configuration. The operator policy must mount the model at `/work/source` and
input artifact index `i` at `/work/input-i`; gaps, arbitrary suffixes, or
filename-based reordering are rejected. The runner binds each envelope to the
ONNX graph by its name, dtype, and shape; it must reject missing, duplicate, or
unexpected graph inputs rather than guessing from filenames. Output artifacts are ordered by the
runner's fixed operator-owned output contract and carry the same validated
tensor envelope.

Artifact bytes continue to use the existing inline/CAS verification and transfer
protocol; no URL or task-provided host path is accepted. The runner must read the
model and tensor files from explicit read-only artifact mounts and emit exactly
one validated `general-compute-result-v1` result envelope. Raw task stdout is
never treated as a successful result.

The current Worker result transport validates the output manifest and its root, but does not yet ingest arbitrary files from `/work/output`. Until a separate verified output-artifact upload protocol is added, a guest must include bounded inline output bytes in each `ArtifactManifest`; completed production results are rejected when an output is only a metadata/chunk claim without verifiable bytes. Output claims that refer only to untransferred host files are not evidence of inference output. The typed result also has an end-to-end RPC size limit, so deployments must keep the complete envelope within the configured protocol limit.

A reference operator runner template is provided at `deploy/onnx/runner.py`, with a digest-pinned-image Dockerfile template. Its CPU path has been exercised locally and on the Linux host against a generated two-element ONNX `Add` model, and its CUDA path has been exercised with Docker GPU passthrough using a locally built CUDA 11.8 / ONNX Runtime 1.16.3 smoke image. These runs validate the tensor decoding, graph binding, provider activation, output envelope, and canonical digest path; they do not substitute for a production rootless OCI image or Nodepool settlement run.

## Current verification boundary

The shared runtime now verifies ONNX registration identity, ordered artifact
binding, provider/GPU compatibility, exact OCI device and cgroup entries, and
result transport. Nodepool scheduler admission resolves the typed GPU capability
snapshot before ranking a Worker; the legacy PullBatch path excludes modern
runtimes so it cannot bypass those gates. A real ONNX Runtime/TensorRT guest
image and Linux Worker-to-Nodepool inference/settlement run are still deployment
work; generating a provider-side claim alone is not evidence of Nodepool
verification or billing settlement.
