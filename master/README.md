# HiveMind Master 节点文档

## 概述
HiveMind Master 是分布式计算平台的核心控制组件，负责协调节点池、管理用户任务、处理身份验证和维护安全通信。作为中央指挥中心，它实现了基于gRPC的节点通信协议、Web管理界面和任务分配算法，确保分布式系统高效稳定运行。

## 核心功能

### 1. 节点池管理
- **动态节点注册**：支持工作节点自动发现和注册
- **心跳监控机制**：每30秒检查节点状态，超时未响应自动标记为离线
- **资源阈值管理**：基于CPU/内存/GPU使用率实现节点负载均衡
- **信任等级评估**：结合Docker状态和历史任务完成率划分节点信任等级

### 2. 任务协调系统
- **智能任务分配**：根据节点资源、信任等级和地理位置优化任务分配
- **任务优先级队列**：支持normal/high/urgent三级优先级调度
- **故障任务重分配**：检测到节点故障时自动将任务重新分配给健康节点
- **成本计算模型**：基于资源需求和优先级计算任务执行成本

### 3. 用户与信用系统
- **JWT身份验证**：实现安全的用户登录和会话管理
- **CPT代币经济**：基于任务贡献度计算奖励和任务执行成本
- **余额查询接口**：提供用户代币余额实时查询
- **交易记录**：维护完整的任务执行和奖励分配记录

### 4. 安全通信
- **WireGuard VPN集成**：自动生成和管理节点间加密隧道
- **gRPC加密传输**：所有节点通信采用Protobuf序列化和TLS加密
- **权限控制**：基于角色的访问控制(RBAC)系统
- **配置安全**：敏感信息加密存储，配置文件权限管理

## 技术实现细节

### 架构设计
```
┌─────────────────────────────────────────────────┐
│                  Master Node                    │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────┐  │
│  │  gRPC Server │  │ Flask Web UI │  │ VPN     │  │
│  │  (50051)     │  │ (5001)       │  │ Service │  │
│  └─────────────┘  └─────────────┘  └─────────┘  │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────┐  │
│  │ Task        │  │ User        │  │ Node    │  │
│  │ Scheduler   │  │ Auth        │  │ Manager │  │
│  └─────────────┘  └─────────────┘  └─────────┘  │
└─────────────────────────────────────────────────┘
```

### gRPC接口定义
```protobuf
// 用户服务接口
service UserService {
  rpc Login (LoginRequest) returns (LoginResponse);
  rpc Register (RegisterRequest) returns (RegisterResponse);
  rpc Transfer (TransferRequest) returns (TransferResponse);
  rpc GetBalance (GetBalanceRequest) returns (GetBalanceResponse);
}

// 主控节点服务接口
service MasterNodeService {
  rpc UploadTask (UploadTaskRequest) returns (UploadTaskResponse);
  rpc GetAllTasks (GetAllTasksRequest) returns (GetAllTasksResponse);
  rpc CancelTask (TaskCancelRequest) returns (TaskCancelResponse);
}

// 节点管理接口
service NodeManagerService {
  rpc ReportNodeStatus (NodeStatusRequest) returns (NodeStatusResponse);
  rpc RegisterNode (NodeRegistrationRequest) returns (NodeRegistrationResponse);
  rpc Heartbeat (HeartbeatRequest) returns (HeartbeatResponse);
}
```

### 任务成本计算模型
```python
# 基于资源需求的成本计算
memory_gb_val = float(requirements.get("memory_gb", 0))
cpu_score_val = float(requirements.get("cpu_score", 0))
gpu_score_val = float(requirements.get("gpu_score", 0))
gpu_memory_gb_val = float(requirements.get("gpu_memory_gb", 0))

base_cost = max(1, int(memory_gb_val + cpu_score_val/100 + gpu_score_val/100 + gpu_memory_gb_val))

# 优先级乘数
priority_multiplier = {"normal": 1.0, "high": 1.2, "urgent": 1.5}.get(priority, 1.0)
cpt_cost = int(base_cost * priority_multiplier)
```

### VPN配置自动生成流程
1. 启动时检查wg0.conf是否存在
2. 如不存在，调用`vpn_service.generate_config()`生成新配置
3. 通过HTTPS获取服务器公钥
4. 本地生成私钥和IP配置
5. 自动启动WireGuard服务并验证连接
6. 配置变更时自动重启VPN

## 安装与配置

### 系统要求
- Windows或Linux操作系统
- Python 3.8+
- Docker Engine 20.10+
- 至少4GB内存
- 网络连接（用于节点通信）

### 依赖安装
```bash
# 安装Python依赖
pip install -r requirements.txt

# 生成gRPC代码（如未预生成）
python -m grpc_tools.protoc -I. --python_out=. --grpc_python_out=. nodepool.proto
```

### 环境变量配置
```bash
# gRPC服务器地址
GRPC_SERVER_ADDRESS=127.0.0.1:50051

# Flask Web界面端口
UI_PORT=5001

# 日志级别
LOG_LEVEL=INFO

### VPN配置示例
```
[Interface]
PrivateKey = kBjZ8eh/TDx4vWJJGxY4+vJVzpkeq8n37gLN4G63nmU=
Address = 10.0.0.18/24
DNS = 8.8.8.8, 1.1.1.1
MTU = 1420

[Peer]
PublicKey = 9ClTcJ/m1iYXo6CSBZhQhPFeARtKn4pr+sKF/5HXDWs=
Endpoint = hivemindvpn.justin0711.com:51820
AllowedIPs = 10.0.0.0/24
PersistentKeepalive = 25
```

## 使用方法

### 启动主控节点
```bash
# 直接运行Python脚本
python3 master_node.py

### 访问Web管理界面
```bash
# 默认地址
http://localhost:5001/login
```

### gRPC接口
| 服务 | 方法 | 描述 |
|------|------|------|
| UserService | Login | 用户登录并获取令牌 |
| UserService | GetBalance | 查询用户CPT余额 |
| MasterNodeService | UploadTask | 上传新任务 |
| MasterNodeService | GetAllTasks | 获取所有任务状态 |
| NodeManagerService | RegisterNode | 节点注册 |
| NodeManagerService | Heartbeat | 节点心跳报告 |

### Web界面API
| 端点 | 方法 | 描述 |
|------|------|------|
| /login | POST | 用户登录 |
| /upload | POST | 上传任务 |
| /tasks | GET | 获取任务列表 |
| /nodes | GET | 获取节点状态 |
| /balance | GET | 查询余额 |

## 项目结构
```
master/
├── master_node.py          # 主程序入口
├── requirements.txt        # 依赖列表
├── wg0.conf                # WireGuard配置
├── nodepool_pb2.py         # Protobuf生成文件
├── nodepool_pb2_grpc.py    # gRPC生成文件
├── templates_master/       # Web界面模板
│   ├── login.html          # 登录页面
│   ├── master_dashboard.html # 主控面板
│   └── master_upload.html  # 任务上传页面
└── static/          # Web静态资源
```

## 故障排除

### 常见问题
1. **gRPC连接失败**
   - 检查防火墙是否允许50051端口通信
   - 验证服务是否正在运行
   - 检查IP和端口配置是否正确

2. **VPN配置错误**
   - 确认WireGuard服务已安装
   - 检查wg0.conf文件权限
   - 验证服务器端点是否可达

3. **Web界面无法访问**
   - 检查5001端口是否被占用
   - 确认Flask应用已正确初始化
   - 查看日志文件获取详细错误信息

## 许可证
本项目采用GNU General Public License v3.0许可证 - 详见LICENSE文件。