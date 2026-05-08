#!/bin/bash

# 生成 VPN proto 的 Go 代碼
# 需要先安裝 protoc 和相關插件

# 檢查 protoc 是否安裝
if ! command -v protoc &> /dev/null; then
    echo "Error: protoc is not installed"
    echo "Please install protoc from: https://github.com/protocolbuffers/protobuf/releases"
    exit 1
fi

# 檢查 Go 插件是否安裝
if ! command -v protoc-gen-go &> /dev/null; then
    echo "Installing protoc-gen-go..."
    go install google.golang.org/protobuf/cmd/protoc-gen-go@latest
fi

if ! command -v protoc-gen-go-grpc &> /dev/null; then
    echo "Installing protoc-gen-go-grpc..."
    go install google.golang.org/grpc/cmd/protoc-gen-go-grpc@latest
fi

# 生成 VPN proto
echo "Generating VPN proto files..."
protoc --go_out=services/nodepool --go-grpc_out=services/nodepool proto/vpn.proto

if [ $? -eq 0 ]; then
    echo "VPN proto files generated successfully"
    echo "Generated files:"
    echo "  - services/nodepool/pb/vpn.pb.go"
    echo "  - services/nodepool/pb/vpn_grpc.pb.go"
else
    echo "Error: Failed to generate VPN proto files"
    exit 1
fi
