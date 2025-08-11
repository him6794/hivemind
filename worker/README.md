# HiveMind Worker 节点文档

## 概述
HiveMind Worker 是分布式计算平台的工作节点组件，负责执行主控节点分配的计算任务，监控系统资源使用情况，并与主控节点保持通信。所有任务在Docker容器中隔离执行，确保安全性和环境一致性。

## 主要功能

### 1. 任务执行
- 通过Docker容器化运行计算任务，使用`justin308/hivemind-worker`基础镜像
- 支持CPU、内存和GPU资源监控与限制
- 任务生命周期管理：启动、监控、终止和结果回传
- 自动处理任务依赖和环境配置

### 2. 资源监控
- 实时采集CPU使用率、内存占用和GPU使用情况
- 每30秒向主控节点报告一次资源使用数据
- 支持多GPU环境的资源监控和分配
- 基于资源使用率动态调整任务优先级

### 3. 节点通信
#### gRPC接口定义
```protobuf
// 节点状态上报接口
service NodeService {
  rpc ReportNodeStatus (NodeStatusRequest) returns (NodeStatusResponse);
  rpc RegisterNode (NodeRegistrationRequest) returns (NodeRegistrationResponse);
  rpc Heartbeat (HeartbeatRequest) returns (HeartbeatResponse);
}

// 任务管理接口
service TaskService {
  rpc AssignTask (TaskAssignmentRequest) returns (TaskAssignmentResponse);
  rpc SubmitTaskResult (TaskResultRequest) returns (TaskResultResponse);
  rpc CancelTask (TaskCancelRequest) returns (TaskCancelResponse);
}
```

#### VPN配置自动生成流程
1. 节点启动时检查wg0.conf文件是否存在
2. 如不存在，调用`vpn_service.generate_config()`生成新配置
3. 通过HTTPS安全获取主控节点公钥
4. 本地生成私钥和IP配置
5. 自动启动WireGuard服务并验证连接
6. 配置变更时自动重启VPN连接

- 使用gRPC协议与主控节点通信
- 实现自动重连机制，处理网络中断情况
- 通过Protobuf定义数据结构，确保通信效率和兼容性
- 支持任务状态实时更新和日志传输

### 4. 安全特性
- 自动生成和管理WireGuard VPN配置，确保节点间安全通信
- 容器化隔离，防止任务间相互干扰
- 资源限制和配额管理
- 节点身份验证和授权

## 安装与配置

### Docker镜像构建
```bash
# 构建worker镜像
python3 build.py --docker

docker build -t justin308/hivemind-worker:latest .

# 推送镜像到仓库
docker push justin308/hivemind-worker:latest
```

### 系统要求
- Windows或Linux操作系统
- Python 3.8+ 
- Docker Engine 20.10+ 
- 至少2GB内存
- 支持虚拟化技术（用于Docker）
- 网络连接（用于下载Docker镜像和与主控节点通信）

### 依赖安装
```bash
# 安装Python依赖
pip install -r requirements.txt

# 确保Docker服务正在运行
systemctl start docker  # Linux
# Windows: 启动Docker Desktop或在PowerShell中执行
# Start-Process 'C:\Program Files\Docker\Docker\Docker Desktop.exe'
# 或在Windows上启动Docker Desktop
```

### 配置选项
worker节点配置主要通过环境变量和配置文件进行：

1. 环境变量配置：
```bash
# 主控节点地址
MASTER_NODE_URL=https://hivemind.justin0711.com

# VPN配置
WIREGUARD_CONFIG_PATH=./wg0.conf

# 资源报告间隔（秒）
RESOURCE_REPORT_INTERVAL=30

# 日志级别
LOG_LEVEL=INFO
```

2. 配置文件：
主要配置文件为`wg0.conf`，包含WireGuard VPN的详细配置：
```
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

### 启动worker节点
```bash
# 直接运行Python脚本
python3 worker_node.py

# 或使用打包好的可执行文件
./HiveMind-Worker.exe  # Windows
# 或
./HiveMind-Worker     # Linux
```

### 命令行参数
```bash
# 指定配置文件
python3 worker_node.py --config ./custom_config.conf

# 启用调试模式
python3 worker_node.py --debug

# 指定日志文件
python3 worker_node.py --log-file ./worker.log

# 覆盖主控节点地址
python3 worker_node.py --master-url https://custom-master-url.com
```

### 监控界面
worker节点提供了一个简单的Web监控界面，默认在端口5001上运行：
```bash
# 访问监控界面
browser http://localhost:5001/monitor.html
```
监控界面显示：
- 当前节点状态
- 运行中任务列表
- 资源使用统计图表
- 任务历史记录

## 技术实现细节

### 任务执行完整生命周期
1. **任务接收**：通过gRPC从主控节点接收任务定义和资源需求
2. **环境准备**：
   - 验证本地Docker环境
   - 拉取所需镜像版本
   - 创建隔离网络和存储卷
3. **任务调度**：
   - 根据节点信任等级分配资源
   - 应用CPU/内存/GPU限制
   - 设置任务超时时间
4. **执行监控**：
   - 实时捕获容器输出
   - 每5秒检查一次运行状态
   - 资源使用率超过阈值时触发预警
5. **结果处理**：
   - 任务完成后收集输出文件
   - 生成执行报告和资源使用统计
   - 通过gRPC流式传输结果
6. **清理工作**：
   - 删除临时容器和网络
   - 保留失败任务的调试数据
   - 更新本地任务历史数据库
**注**：以下为简化流程，详细生命周期请参见上文「任务执行完整生命周期」章节

### 资源监控实现
资源监控通过以下方式实现：
- CPU使用率：使用psutil库采集
- 内存使用：通过系统API获取内存占用
- GPU监控：使用nvidia-smi（NVIDIA系统管理接口）
- 资源数据每30秒采样一次，并通过gRPC发送给主控节点

### 奖励计算
worker节点根据资源贡献获得奖励：
```python
# 简化的奖励计算公式
base_reward = 10  # 基础奖励
usage_multiplier = 1.0

# 根据平均使用率调整倍数
avg_usage = (cpu_usage + memory_usage) / 2
if avg_usage > 80:
    usage_multiplier = 1.5
elif avg_usage > 50:
    usage_multiplier = 1.2
elif avg_usage > 20:
    usage_multiplier = 1.0
else:
    usage_multiplier = 0.8

# GPU额外奖励
gpu_bonus = gpu_usage * 0.01

total_reward = int(base_reward * usage_multiplier + gpu_bonus)
```

## 故障排除

### 常见问题
1. **Docker连接问题**
   - 确保Docker服务正在运行
   - 检查用户是否有权限访问Docker套接字
   - 验证网络连接是否正常

2. **VPN配置错误**
   - 检查wg0.conf文件是否正确
   - 验证Endpoint地址和端口是否可达
   - 确保防火墙允许UDP 51820端口通信

3. **资源报告失败**
   - 检查与主控节点的网络连接
   - 验证gRPC服务是否正常运行
   - 查看日志文件获取详细错误信息

4. **任务执行失败**
   - 检查Docker镜像是否完整
   - 验证任务资源需求是否超过节点能力
   - 查看任务日志了解具体错误原因


### 项目结构
```
worker/
├── Dockerfile           # Docker镜像构建文件
├── README.md            # 本文档
├── build.py             # 可执行文件构建脚本
├── hivemind_worker/
│   ├── __init__.py
│   ├── main.py          # 入口点
│   ├── src/
│   │   └── hivemind_worker/
│   │       ├── communication/
│   │       │   ├── grpc_client.py
│   │       │   └── vpn_configurator.py
│   │       ├── monitoring/
│   │       │   ├── resource_collector.py
│   │       │   └── stats_aggregator.py
│   │       └── task_management/
│   │       ├── communication/
│   │       │   ├── grpc_client.py
│   │       │   └── vpn_configurator.py
│   │       ├── monitoring/
│   │       │   ├── resource_collector.py
│   │       │   └── stats_aggregator.py
│   │       └── task_management/
│   │           ├── docker_handler.py
│   │           └── task_executor.py
│   ├── pyproject.toml
│   ├── setup_logic.ps1
│   └── setup_worker.iss
├── install.sh           # 安装脚本
├── make.py              # 构建脚本
├── requirements.txt     # Python依赖
├── run_task.sh          # 任务执行脚本
├── setup.py             # 包安装配置
├── static/              # Web监控界面静态文件
├── templates/           # Web界面模板
├── wg0.conf             # WireGuard配置
└── worker_node.py       # 主程序
```

## 许可证
本项目采用GNU General Public License v3.0许可证 - 详见LICENSE文件。

## 联系信息
- 项目网站: https://hivemind.justin0711.com
- 支持邮箱: hivemind@justin0711.com
