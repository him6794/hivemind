# Fill BASE_IMAGE with an operator-approved digest-pinned Python image.
ARG BASE_IMAGE
FROM ${BASE_IMAGE}

# The operator must include onnx, protobuf, numpy, and the selected
# onnxruntime/provider wheels (plus their reviewed dependencies) in vendor/.
# No dependency is fetched from a package index during the build.
COPY vendor/ /opt/hivemind/vendor/
COPY runner.py /opt/hivemind/onnx-runner
COPY provider /etc/hivemind/onnx/provider

RUN python3 -m pip install --no-index --no-deps /opt/hivemind/vendor/*.whl \
    && chmod 0555 /opt/hivemind/onnx-runner \
    && chmod 0444 /etc/hivemind/onnx/provider \
    && rm -rf /opt/hivemind/vendor

USER 65532:65532
ENTRYPOINT ["/opt/hivemind/onnx-runner"]
