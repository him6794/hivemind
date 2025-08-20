# HiveMind Worker 節點文檔

> **Language / 語言選擇**
> 
> - **English**: [README.en.md](README.en.md)
> - **繁體中文**: [README.md](README.md) (本文檔)

## 概述

HiveMind Worker 是分布式運算平台的工作節點組件，負責執行主控節點分配的運算任務，監控系統資源使用情況，並與主控節點保持通訊。所有任務在 Docker 容器中隔離執行，確保安全性和環境一致性。

## 主要功能

### 任務執行
- 透過 Docker 容器化運行運算任務，使用 `justin308/hivemind-worker` 基礎映像檔
- 支援 CPU、記憶體和 GPU 資源監控與限制
- 任務生命週期管理：啟動、監控、終止和結果回傳
- 自動處理任務相依性和環境配置

### 資源監控
- 即時採集 CPU 使用率、記憶體佔用和 GPU 使用情況
- 每 30 秒向主控節點報告一次資源使用數據
- 支援多 GPU 環境的資源監控和分配
- 基於資源使用率動態調整任務優先級

### 節點通訊

#### gRPC 介面定義
```protobuf
// 節點狀態上報介面
service NodeService {
  rpc ReportNodeStatus (NodeStatusRequest) returns (NodeStatusResponse);
  rpc RegisterNode (NodeRegistrationRequest) returns (NodeRegistrationResponse);
  rpc Heartbeat (HeartbeatRequest) returns (HeartbeatResponse);
}

// 任務管理介面
service TaskService {
  rpc AssignTask (TaskAssignmentRequest) returns (TaskAssignmentResponse);
  rpc SubmitTaskResult (TaskResultRequest) returns (TaskResultResponse);
  rpc CancelTask (TaskCancelRequest) returns (TaskCancelResponse);
}
```

#### VPN 配置自動生成流程
1. 節點啟動時檢查 wg0.conf 檔案是否存在
2. 如不存在，呼叫 `vpn_service.generate_config()` 生成新配置
3. 透過 HTTPS 安全獲取主控節點公鑰
4. 本機生成私鑰和 IP 配置
5. 自動啟動 WireGuard 服務並驗證連線
6. 配置變更時自動重啟 VPN 連線

**通訊特色**：
- 使用 gRPC 協定與主控節點通訊
- 實現自動重連機制，處理網路中斷情況
- 透過 Protobuf 定義資料結構，確保通訊效率和相容性
- 支援任務狀態即時更新和日誌傳輸

### 安全特性
- 自動生成和管理 WireGuard VPN 配置，確保節點間安全通訊
- 容器化隔離，防止任務間相互干擾
- 資源限制和配額管理
- 節點身份驗證和授權

## 安裝與配置

### Docker 映像檔建構
```bash
# 建構 worker 映像檔
python3 build.py --docker

docker build -t justin308/hivemind-worker:latest .

# 推送映像檔到倉庫
docker push justin308/hivemind-worker:latest
```

### 系統需求
- **作業系統**：Windows 10/11 或 Linux (Ubuntu 18.04+)
- **Python**：3.8 或更高版本
- **Docker**：Engine 20.10 或更高版本
- **記憶體**：至少 2GB RAM (建議 4GB+)
- **虛擬化**：支援虛擬化技術（用於 Docker）
- **網路**：穩定的網際網路連線（用於下載 Docker 映像檔和與主控節點通訊）

### 相依性安裝
```bash
# 安裝 Python 相依性
pip install -r requirements.txt

# 確保 Docker 服務正在運行
# Linux
sudo systemctl start docker
sudo systemctl enable docker

# Windows: 啟動 Docker Desktop
# 或在 PowerShell 中執行
Start-Process 'C:\Program Files\Docker\Docker\Docker Desktop.exe'
```

### 配置選項

Worker 節點配置主要透過環境變數和配置檔案進行：

#### 1. 環境變數配置：
```bash
# 主控節點位址
MASTER_NODE_URL=https://hivemind.justin0711.com

# VPN 配置檔案路徑
WIREGUARD_CONFIG_PATH=./wg0.conf

# 資源報告間隔（秒）
RESOURCE_REPORT_INTERVAL=30

# 日誌等級
LOG_LEVEL=INFO

# 最大並行任務數
MAX_CONCURRENT_TASKS=3

# Docker 映像檔標籤
WORKER_IMAGE_TAG=latest
```

#### 2. WireGuard VPN 配置檔案：
主要配置檔案為 `wg0.conf`，包含 WireGuard VPN 的詳細配置：
```ini
[Interface]
PrivateKey = <worker_private_key>
Address = 10.8.0.2/32
DNS = 8.8.8.8

[Peer]
PublicKey = <server_public_key>
Endpoint = hivemindvpn.justin0711.com:51820
AllowedIPs = 0.0.0.0/0
PersistentKeepalive = 25
```

## 使用方法

### 啟動 Worker 節點

#### 基本啟動方式
```bash
# 直接運行 Python 腳本
python3 worker_node.py

# 使用打包好的可執行檔案
./HiveMind-Worker.exe  # Windows
./HiveMind-Worker      # Linux/macOS
```

#### 命令列參數
```bash
# 指定配置檔案
python3 worker_node.py --config ./custom_config.conf

# 啟用除錯模式
python3 worker_node.py --debug

# 指定日誌檔案
python3 worker_node.py --log-file ./worker.log

# 覆蓋主控節點位址
python3 worker_node.py --master-url https://custom-master-url.com

# 設定最大並行任務數
python3 worker_node.py --max-tasks 5

# 僅限 CPU 模式（不使用 GPU）
python3 worker_node.py --cpu-only
```

### 監控介面

Worker 節點提供了一個簡潔的 Web 監控介面，預設在埠號 5001 上運行：

```bash
# 存取監控介面
http://localhost:5001/monitor.html
```

**監控介面功能**：
- 即時節點狀態顯示
- 運行中任務清單
- 資源使用統計圖表
- 📜 任務歷史記錄
- 錯誤和警告訊息
- 基本配置調整

### 進階配置

#### 自動啟動設定（Linux）
```bash
# 建立 systemd 服務檔案
sudo nano /etc/systemd/system/hivemind-worker.service
```

```ini
[Unit]
Description=HiveMind Worker Node
After=network.target docker.service
Requires=docker.service

[Service]
Type=simple
User=hivemind
WorkingDirectory=/opt/hivemind/worker
Environment=PATH=/opt/hivemind/venv/bin
ExecStart=/opt/hivemind/venv/bin/python worker_node.py
Restart=always
RestartSec=10

[Install]
WantedBy=multi-user.target
```

```bash
# 啟動服務
sudo systemctl daemon-reload
sudo systemctl enable hivemind-worker
sudo systemctl start hivemind-worker
```

#### Windows 服務設定
```powershell
# 使用 NSSM (Non-Sucking Service Manager)
nssm install HiveMindWorker "C:\hivemind\worker\HiveMind-Worker.exe"
nssm set HiveMindWorker AppDirectory "C:\hivemind\worker"
nssm start HiveMindWorker
```

## 技術實現細節

### 任務執行完整生命週期

1. **📥 任務接收**：透過 gRPC 從主控節點接收任務定義和資源需求
2. **環境準備**：
   - 驗證本機 Docker 環境
   - 拉取所需映像檔版本
   - 建立隔離網路和儲存磁碟區
3. **⏰ 任務排程**：
   - 根據節點信任等級分配資源
   - 套用 CPU/記憶體/GPU 限制
   - 設定任務逾時時間
4. **執行監控**：
   - 即時擷取容器輸出
   - 每 5 秒檢查一次運行狀態
   - 資源使用率超過閾值時觸發預警
5. **📤 結果處理**：
   - 任務完成後收集輸出檔案
   - 生成執行報告和資源使用統計
   - 透過 gRPC 串流傳輸結果
6. **🧹 清理工作**：
   - 刪除暫存容器和網路
   - 保留失敗任務的除錯資料
   - 更新本機任務歷史資料庫

### 資源監控實現

資源監控透過以下方式實現：

```python
import psutil
import GPUtil
from typing import Dict

class ResourceMonitor:
    """資源監控器"""
    
    def collect_system_stats(self) -> Dict[str, float]:
        """收集系統資源統計"""
        return {
            'cpu_percent': psutil.cpu_percent(interval=1),
            'memory_percent': psutil.virtual_memory().percent,
            'disk_percent': psutil.disk_usage('/').percent,
            'network_io': self._get_network_io(),
            'gpu_stats': self._get_gpu_stats()
        }
    
    def _get_gpu_stats(self) -> Dict[str, float]:
        """獲取 GPU 使用統計"""
        try:
            gpus = GPUtil.getGPUs()
            if gpus:
                gpu = gpus[0]  # 使用第一張 GPU
                return {
                    'gpu_percent': gpu.load * 100,
                    'gpu_memory_percent': gpu.memoryUtil * 100,
                    'gpu_temperature': gpu.temperature
                }
        except Exception:
            pass
        return {'gpu_percent': 0, 'gpu_memory_percent': 0}
```

**監控頻率**：
- **CPU 使用率**：使用 psutil 函式庫採集
- **記憶體使用**：透過系統 API 獲取記憶體佔用
- **GPU 監控**：使用 nvidia-smi（NVIDIA 系統管理介面）
- **資源資料**：每 30 秒採樣一次，並透過 gRPC 發送給主控節點

### 💰 獎勵計算機制

Worker 節點根據資源貢獻獲得 CPT 代幣獎勵：

```python
def calculate_reward(task_duration: int, resource_usage: Dict[str, float]) -> int:
    """
    計算節點獎勵
    
    Args:
        task_duration: 任務執行時間（秒）
        resource_usage: 平均資源使用率
        
    Returns:
        獎勵金額（CPT 代幣）
    """
    base_reward = 10  # 基礎獎勵
    
    # 根據平均使用率調整倍數
    avg_usage = (resource_usage['cpu'] + resource_usage['memory']) / 2
    
    if avg_usage > 80:
        usage_multiplier = 1.5      # 高使用率獎勵
    elif avg_usage > 50:
        usage_multiplier = 1.2      # 中等使用率獎勵
    elif avg_usage > 20:
        usage_multiplier = 1.0      # 標準獎勵
    else:
        usage_multiplier = 0.8      # 低使用率懲罰
    
    # GPU 額外獎勵
    gpu_bonus = resource_usage.get('gpu', 0) * 0.01
    
    # 時間獎勵（長時間任務額外獎勵）
    time_bonus = min(task_duration / 3600, 2.0)  # 最多 2 倍時間獎勵
    
    total_reward = int(base_reward * usage_multiplier * time_bonus + gpu_bonus)
    return max(total_reward, 1)  # 最少 1 CPT
```

### 安全機制

#### 容器隔離
```python
# Docker 容器安全配置
container_config = {
    'security_opt': ['no-new-privileges:true'],
    'cap_drop': ['ALL'],
    'cap_add': ['CHOWN', 'SETUID', 'SETGID'],
    'read_only': True,
    'tmpfs': {'/tmp': 'noexec,nosuid,size=100m'},
    'ulimits': [
        docker.types.Ulimit(name='nofile', soft=1024, hard=1024),
        docker.types.Ulimit(name='nproc', soft=512, hard=512)
    ]
}
```

#### 資源限制
```python
# 資源使用限制
resource_limits = {
    'mem_limit': '2g',           # 記憶體限制
    'cpuset_cpus': '0-3',        # CPU 核心限制
    'cpu_percent': 80,           # CPU 使用率限制
    'pids_limit': 100,           # 行程數限制
    'storage_opt': {'size': '1g'} # 儲存空間限制
}
```

## 故障排除

### ❓ 常見問題

#### 1. 🐳 Docker 連線問題
**症狀**：
- 無法啟動 Docker 容器
- "Cannot connect to Docker daemon" 錯誤
- 權限被拒絕錯誤

**解決方案**：
```bash
# 檢查 Docker 服務狀態
sudo systemctl status docker

# 啟動 Docker 服務
sudo systemctl start docker

# 將使用者加入 docker 群組
sudo usermod -aG docker $USER
newgrp docker

# 測試 Docker 連線
docker run hello-world
```

#### 2. VPN 配置錯誤
**症狀**：
- 無法連線到主控節點
- WireGuard 介面啟動失敗
- 網路逾時錯誤

**解決方案**：
```bash
# 檢查 WireGuard 配置
sudo wg show

# 檢查配置檔案語法
wg-quick down wg0
wg-quick up wg0

# 測試網路連通性
ping 10.8.0.1

# 檢查防火牆設定
sudo ufw allow 51820/udp

# 重新生成配置
rm wg0.conf
python3 worker_node.py --generate-vpn-config
```

#### 3. 資源報告失敗
**症狀**：
- 節點顯示為離線狀態
- gRPC 連線失敗
- 資源監控數據遺失

**診斷步驟**：
```bash
# 檢查網路連線
curl -I https://hivemind.justin0711.com

# 測試 gRPC 連線
grpcurl -plaintext localhost:50051 list

# 檢查日誌檔案
tail -f worker.log

# 手動測試資源收集
python3 -c "import psutil; print(psutil.cpu_percent())"
```

#### 4. 任務執行失敗
**症狀**：
- 任務始終失敗
- 容器無法啟動
- 記憶體不足錯誤

**解決方案**：
```bash
# 檢查 Docker 映像檔
docker images | grep hivemind-worker

# 檢查可用資源
free -h
df -h

# 清理 Docker 資源
docker system prune -f

# 檢查任務日誌
docker logs <container_id>

# 調整資源限制
export MAX_MEMORY_LIMIT=4g
export MAX_CPU_CORES=4
```

### 診斷工具

#### 系統健康檢查腳本
```bash
#!/bin/bash
# worker_health_check.sh

echo "=== HiveMind Worker 健康檢查 ==="
echo "時間: $(date)"
echo

# 檢查 Docker 狀態
echo "1. Docker 狀態:"
if systemctl is-active --quiet docker; then
    echo "✓ Docker 服務: 運行中"
    docker version --format "✓ Docker 版本: {{.Server.Version}}"
else
    echo "✗ Docker 服務: 停止"
fi

# 檢查 VPN 狀態
echo "2. VPN 狀態:"
if ip link show wg0 >/dev/null 2>&1; then
    echo "✓ WireGuard 介面: 啟用"
    echo "✓ VPN IP: $(ip addr show wg0 | grep inet | awk '{print $2}')"
else
    echo "✗ WireGuard 介面: 未啟用"
fi

# 檢查資源使用
echo "3. 資源使用:"
echo "CPU: $(grep 'cpu ' /proc/stat | awk '{usage=($2+$4)*100/($2+$3+$4+$5)} END {print usage "%"}')"
echo "記憶體: $(free | grep Mem | awk '{printf("%.1f%%", $3/$2 * 100.0)}')"
echo "磁碟: $(df / | awk 'NR==2{printf "%s", $5}')"

# 檢查網路連線
echo "4. 網路連線:"
if ping -c 1 8.8.8.8 >/dev/null 2>&1; then
    echo "✓ 網際網路連線: 正常"
else
    echo "✗ 網際網路連線: 失敗"
fi

echo "=== 檢查完成 ==="
```

#### 效能監控腳本
```python
#!/usr/bin/env python3
# performance_monitor.py

import psutil
import time
import json
from datetime import datetime

def collect_metrics():
    """收集系統效能指標"""
    metrics = {
        'timestamp': datetime.now().isoformat(),
        'cpu_percent': psutil.cpu_percent(interval=1),
        'memory': {
            'total': psutil.virtual_memory().total,
            'available': psutil.virtual_memory().available,
            'percent': psutil.virtual_memory().percent
        },
        'disk': {
            'total': psutil.disk_usage('/').total,
            'free': psutil.disk_usage('/').free,
            'percent': psutil.disk_usage('/').percent
        },
        'network': dict(psutil.net_io_counters()._asdict()),
        'load_avg': psutil.getloadavg() if hasattr(psutil, 'getloadavg') else None
    }
    
    # GPU 資訊（如果可用）
    try:
        import GPUtil
        gpus = GPUtil.getGPUs()
        if gpus:
            gpu = gpus[0]
            metrics['gpu'] = {
                'name': gpu.name,
                'load': gpu.load * 100,
                'memory_util': gpu.memoryUtil * 100,
                'temperature': gpu.temperature
            }
    except ImportError:
        pass
    
    return metrics

if __name__ == "__main__":
    print("開始效能監控...")
    try:
        while True:
            metrics = collect_metrics()
            print(json.dumps(metrics, indent=2, ensure_ascii=False))
            time.sleep(30)
    except KeyboardInterrupt:
        print("\n監控已停止")
```

### 效能優化

#### 系統調校建議
```bash
# 1. 調整 Docker 儲存驅動程式
echo '{"storage-driver": "overlay2"}' | sudo tee /etc/docker/daemon.json
sudo systemctl restart docker

# 2. 優化網路設定
echo 'net.core.rmem_max = 16777216' | sudo tee -a /etc/sysctl.conf
echo 'net.core.wmem_max = 16777216' | sudo tee -a /etc/sysctl.conf
sudo sysctl -p

# 3. 增加檔案描述符限制
echo 'fs.file-max = 65536' | sudo tee -a /etc/sysctl.conf
echo '* soft nofile 65536' | sudo tee -a /etc/security/limits.conf
echo '* hard nofile 65536' | sudo tee -a /etc/security/limits.conf

# 4. 優化記憶體管理
echo 'vm.swappiness = 10' | sudo tee -a /etc/sysctl.conf
echo 'vm.vfs_cache_pressure = 50' | sudo tee -a /etc/sysctl.conf
```

#### 容器最佳化
```python
# 優化的容器配置
optimized_config = {
    'mem_limit': '2g',
    'memswap_limit': '2g',  # 禁用 swap
    'cpu_quota': 100000,    # 100% CPU
    'cpu_period': 100000,
    'blkio_weight': 500,    # 中等 I/O 優先級
    'restart_policy': {"Name": "unless-stopped"},
    'log_config': {
        'type': 'json-file',
        'config': {
            'max-size': '10m',
            'max-file': '3'
        }
    }
}
```

## 📁 專案結構

```
worker/
├── Dockerfile               # Docker 映像檔建置檔案
├── README.md                # 本說明文件
├── build.py                 # 可執行檔建置腳本
├── worker_node.py           # 主程式
├── make.py                  # 建置腳本
├── requirements.txt         # Python 依賴套件
├── run_task.sh              # 任務執行腳本
├── setup.py                 # 套件安裝配置
├── install.sh               # 安裝腳本
├── wg0.conf                 # WireGuard VPN 配置檔案
├── file.ico                 # 應用程式圖示
├── 📁 hivemind_worker/         # Worker 應用程式套件
│   ├── __init__.py
│   ├── main.py              # 程式進入點
│   ├── pyproject.toml       # 專案配置檔案
│   ├── setup_logic.ps1      # Windows 設定腳本
│   ├── setup_worker.iss     # Inno Setup 安裝腳本
│   └── 📁 src/
│       └── 📁 hivemind_worker/
│           ├── 📁 communication/     # 通訊模組
│           │   ├── grpc_client.py       # gRPC 客戶端
│           │   └── vpn_configurator.py  # VPN 配置器
│           ├── 📁 monitoring/        # 監控模組
│           │   ├── resource_collector.py  # 資源收集器
│           │   └── stats_aggregator.py   # 統計資料聚合器
│           └── 📁 task_management/   # 任務管理模組
│               ├── docker_handler.py    # Docker 處理器
│               └── task_executor.py     # 任務執行器
├── 📁 static/                  # 網頁監控介面靜態檔案
│   ├── css/                # 樣式表檔案
│   ├── 📜 js/                 # JavaScript 檔案
│   └── images/             # 圖片資源
├── 📁 templates/               # 網頁介面範本
│   ├── dashboard.html       # 儀表板頁面
│   ├── status.html          # 狀態監控頁面
│   └── settings.html        # 設定頁面
├── 📁 __pycache__/            # Python 位元碼快取
└── 📁 HiveMind-Worker-Release/ # 發布版本目錄
    ├── hivemind_worker.exe  # Windows 可執行檔
    ├── start_worker.cmd     # Windows 啟動腳本
    ├── start_worker.sh      # Linux 啟動腳本
    └── 📁 Output/              # 建置輸出目錄
```

### 核心模組說明

#### 📡 communication/ - 通訊模組
- **grpc_client.py**: 與主控節點的 gRPC 通訊實現
- **vpn_configurator.py**: WireGuard VPN 自動配置和管理

#### monitoring/ - 監控模組  
- **resource_collector.py**: 系統資源資料收集（CPU、記憶體、GPU）
- **stats_aggregator.py**: 效能統計資料聚合和分析

#### task_management/ - 任務管理模組
- **docker_handler.py**: Docker 容器生命週期管理
- **task_executor.py**: 分散式任務執行引擎

## 授權條款

本專案採用 **GNU General Public License v3.0** 授權條款 - 詳見 [LICENSE](../LICENSE.txt) 檔案。

### 授權摘要
- **商業使用**: 允許商業用途
- **修改**: 允許修改原始碼
- **散布**: 允許散布修改版本
- **專利授權**: 提供專利保護
- **私人使用**: 允許私人使用

### 授權條件
- **公開原始碼**: 散布時必須提供原始碼
- **授權聲明**: 必須包含授權和版權聲明
- 🔄 **相同授權**: 衍生作品必須使用相同授權
- **狀態變更**: 修改檔案時須說明變更內容

## 聯絡資訊

### 官方網站
- **專案首頁**: [https://hivemind.justin0711.com](https://hivemind.justin0711.com)
- **文件中心**: [https://docs.hivemind.justin0711.com](https://docs.hivemind.justin0711.com)
- **API 文件**: [https://api.hivemind.justin0711.com](https://api.hivemind.justin0711.com)

### 📧 支援服務
- **技術支援**: [hivemind@justin0711.com](mailto:hivemind@justin0711.com)
- **錯誤回報**: [GitHub Issues](https://github.com/justin0711/hivemind/issues)
- **功能建議**: [GitHub Discussions](https://github.com/justin0711/hivemind/discussions)

### 社群
- **Discord 伺服器**: [加入我們的社群](https://discord.gg/hivemind)
- **Telegram 群組**: [@HiveMindTW](https://t.me/HiveMindTW)
- **Reddit**: [r/HiveMindComputing](https://reddit.com/r/HiveMindComputing)

### 貢獻指南
歡迎提交問題回報、功能請求或拉取請求。請參閱我們的 [貢獻指南](../CONTRIBUTING.md) 了解詳細資訊。

---

<div align="center">

**加入 HiveMind 分散式運算網路 **

*讓您的閒置算力創造價值，共同建構更強大的運算生態系統*

[![GitHub Stars](https://img.shields.io/github/stars/justin0711/hivemind?style=social)](https://github.com/justin0711/hivemind)
[![Discord](https://img.shields.io/discord/123456789?style=social&logo=discord)](https://discord.gg/hivemind)
[![License](https://img.shields.io/badge/license-GPL--3.0-blue.svg)](../LICENSE.txt)

</div>
