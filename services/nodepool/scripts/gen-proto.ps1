param(
    [string]$ProtocPath = ""
)

$ErrorActionPreference = "Stop"

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..\..")).Path
$protoFile = Join-Path $repoRoot "proto\hivemind.proto"
$outDir = Join-Path $repoRoot "services\nodepool\pb"

if (-not (Test-Path $protoFile)) {
    throw "proto file not found: $protoFile"
}

if ([string]::IsNullOrWhiteSpace($ProtocPath)) {
    $cmd = Get-Command protoc -ErrorAction SilentlyContinue
    if ($cmd) {
        $ProtocPath = $cmd.Source
    }
}

if ([string]::IsNullOrWhiteSpace($ProtocPath)) {
    $defaultUserProtoc = "C:\Users\user\protoc-34.0-win64\bin\protoc.exe"
    if (Test-Path $defaultUserProtoc) {
        $ProtocPath = $defaultUserProtoc
    }
}

if ([string]::IsNullOrWhiteSpace($ProtocPath) -or -not (Test-Path $ProtocPath)) {
    throw "protoc not found. Please pass -ProtocPath <path-to-protoc.exe>"
}

$gobin = (go env GOBIN)
if ([string]::IsNullOrWhiteSpace($gobin)) {
    $gobin = Join-Path (go env GOPATH) "bin"
}

$protocGenGo = Join-Path $gobin "protoc-gen-go.exe"
$protocGenGoGrpc = Join-Path $gobin "protoc-gen-go-grpc.exe"

if (-not (Test-Path $protocGenGo)) {
    Write-Host "Installing protoc-gen-go..."
    go install google.golang.org/protobuf/cmd/protoc-gen-go@latest
}
if (-not (Test-Path $protocGenGoGrpc)) {
    Write-Host "Installing protoc-gen-go-grpc..."
    go install google.golang.org/grpc/cmd/protoc-gen-go-grpc@latest
}

if (-not (Test-Path $protocGenGo) -or -not (Test-Path $protocGenGoGrpc)) {
    throw "protoc plugins not found in $gobin"
}

Write-Host "Using protoc: $ProtocPath"

& $ProtocPath \
  "--plugin=protoc-gen-go=$protocGenGo" \
  "--plugin=protoc-gen-go-grpc=$protocGenGoGrpc" \
  "--proto_path=$(Join-Path $repoRoot 'proto')" \
  "--go_out=$outDir" \
  "--go-grpc_out=$outDir" \
  "--go_opt=paths=source_relative" \
  "--go-grpc_opt=paths=source_relative" \
  "--go_opt=Mhivemind.proto=hivemind/services/nodepool/pb;pb" \
  "--go-grpc_opt=Mhivemind.proto=hivemind/services/nodepool/pb;pb" \
  $protoFile

Write-Host "Generated: $(Join-Path $outDir 'hivemind.pb.go')"
Write-Host "Generated: $(Join-Path $outDir 'hivemind_grpc.pb.go')"
