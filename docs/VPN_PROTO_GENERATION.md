# VPN Proto 生成說明

由於系統中未安裝 protoc，需要手動生成 VPN proto 的 Go 代碼。

## 安裝 protoc

### Windows
1. 從 https://github.com/protocolbuffers/protobuf/releases 下載最新版本
2. 解壓並將 bin 目錄添加到 PATH

### Linux
```bash
sudo apt-get install protobuf-compiler
```

### macOS
```bash
brew install protobuf
```

## 安裝 Go 插件

```bash
go install google.golang.org/protobuf/cmd/protoc-gen-go@latest
go install google.golang.org/grpc/cmd/protoc-gen-go-grpc@latest
```

## 生成代碼

```bash
# 在項目根目錄執行
protoc --go_out=services/nodepool --go-grpc_out=services/nodepool proto/vpn.proto
```

或使用提供的腳本：

```bash
bash scripts/generate_vpn_proto.sh
```

## 生成的文件

- `services/nodepool/pb/vpn.pb.go` - VPN 消息定義
- `services/nodepool/pb/vpn_grpc.pb.go` - VPN gRPC 服務定義
