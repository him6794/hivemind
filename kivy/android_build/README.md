# HiveMind Android Worker

這是 HiveMind 分散式計算網路的 Android 工作節點應用程式。

## 構建步驟

### 1. 安裝 Buildozer

```bash
pip install buildozer
```

### 2. 安裝依賴項

在 Windows 上：
```bash
# 安裝 Cython 和其他依賴
pip install Cython==0.29.33
pip install --upgrade pip setuptools wheel
```

在 Linux/macOS 上：
```bash
# 安裝系統依賴
sudo apt-get update
sudo apt-get install -y git zip unzip openjdk-8-jdk python3-pip autoconf libtool pkg-config zlib1g-dev libncurses5-dev libncursesw5-dev libtinfo5 cmake libffi-dev libssl-dev
```

### 3. 初始化 Buildozer

```bash
cd d:\hivemind\kivy\android_build
buildozer init
```

### 4. 構建 APK

```bash
# 構建調試版本
buildozer android debug

# 如果遇到問題，可以嘗試清理後重新構建
buildozer android clean
buildozer android debug
```

### 5. 構建發布版本

```bash
buildozer android release
```

### 6. 安裝到設備

```bash
# 安裝調試版本
buildozer android deploy

# 或者手動安裝
adb install bin/hivemindworker-1.0-debug.apk
```

## 故障排除

### 問題 1: "Unknown command/target android"

**解決方案：**
1. 確保您在正確的目錄中（包含 buildozer.spec 文件）
2. 重新初始化 buildozer ：
   ```bash
   buildozer init
   ```
3. 檢查 buildozer.spec 文件是否存在和格式正確

### 問題 2: Java 版本問題

**解決方案：**
```bash
# 檢查 Java 版本
java -version

# 如果版本不對，設置 JAVA_HOME
export JAVA_HOME=/usr/lib/jvm/java-8-openjdk-amd64
```

### 問題 3: NDK 下載失敗

**解決方案：**
```bash
# 手動指定 NDK 路徑
buildozer android debug --verbose
```

### 問題 4: 依賴項問題

**解決方案：**
1. 更新 requirements 中的版本號
2. 使用 `buildozer android clean` 清理構建緩存
3. 重新構建

### 問題 5: 權限問題

**解決方案：**
```bash
# 在 Linux/macOS 上，確保有正確的權限
chmod +x ~/.buildozer/android/platform/android-ndk-*/toolchains/*/prebuilt/*/bin/*
```

## 構建選項

### 指定架構

```bash
# 構建 ARM64 版本
buildozer android debug --arch arm64-v8a

# 構建 ARM32 版本
buildozer android debug --arch armeabi-v7a
```

### 詳細日誌

```bash
# 顯示詳細構建日誌
buildozer android debug --verbose
```

### 清理構建

```bash
# 清理所有構建文件
buildozer android clean

# 清理分發文件
buildozer android distclean
```

## 功能特性

- 基於 Kivy 的 Android 原生界面
- 支持用戶登錄和節點註冊
- 實時狀態監控和日誌顯示
- 地區設置和 CPT 餘額查詢
- 完整的 gRPC 通信支持

## 注意事項

- 需要網路連接才能與主控節點通信
- 預設連接到 10.0.0.1:50051 (可通過環境變數修改)
- 應用程式不支援實際的任務執行（Docker 容器），主要用於監控和管理
- 構建過程可能需要很長時間（首次構建可能需要1-2小時）
- 確保有足夠的磁盤空間（至少 10GB）

## 環境變數

- `MASTER_ADDRESS`: 主控節點地址 (預設: 10.0.0.1:50051)
- `NODE_PORT`: 節點端口 (預設: 50053)
- `NODE_ID`: 節點 ID (自動生成)

## 構建文件說明

- `buildozer.spec`: Buildozer 配置文件
- `main.py`: 應用程式入口點
- `android_worker_node.py`: Android 工作節點實現
- `nodepool_pb2.py`: gRPC 協議緩衝區文件
- `nodepool_pb2_grpc.py`: gRPC 服務定義文件

## 測試

構建完成後，APK 文件將在 `bin/` 目錄中：
- `hivemindworker-1.0-debug.apk`: 調試版本
- `hivemindworker-1.0-release.apk`: 發布版本

可以通過 ADB 安裝到 Android 設備進行測試：
```bash
adb install bin/hivemindworker-1.0-debug.apk
```
