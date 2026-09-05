$ErrorActionPreference = "Stop"

$runnerPath = Join-Path $PSScriptRoot "..\deploy\onnx\runner.py"
if (!(Test-Path -LiteralPath $runnerPath -PathType Leaf)) {
    throw "deploy/onnx/runner.py must exist."
}
$runnerText = [IO.File]::ReadAllText((Resolve-Path -LiteralPath $runnerPath), [Text.Encoding]::UTF8)

foreach ($expected in @(
    "ort.SessionOptions()",
    '"session.disable_cpu_ep_fallback"',
    '"1"',
    "sess_options=session_options",
    "providers=[provider]",
    "active_providers = session.get_providers()",
    "configured ONNX provider was not activated",
    "session.run(None, feed)"
)) {
    if (!$runnerText.Contains($expected)) {
        throw "ONNX runner must enforce the provider-only execution contract via '$expected'."
    }
}

if ($runnerText.Contains("InferenceSession(source, providers=[provider])")) {
    throw "ONNX runner must pass the fallback-disabled SessionOptions when creating a session."
}

Write-Output "ONNX runner provider fallback contract passed"
