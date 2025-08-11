# HiveMind Node Pool

## 项目概述
节点池（Node Pool）是HiveMind分布式计算平台的核心组件，负责管理分布式节点网络、协调任务分配、监控资源使用情况，并维护节点信用体系。该组件基于gRPC通信协议构建，采用Redis进行节点状态存储，SQLite管理用户数据，实现了高性能、可扩展的分布式节点管理解决方案。

## 核心功能

### 1. 节点管理系统
- **自动注册机制**：支持工作节点的自动发现与注册，收集节点硬件信息（CPU核心数、内存容量、GPU型号及显存）<mcfile name="node_manager.py" path="d:/hivemind/node_pool/node_manager.py"></mcfile>
- **信任等级划分**：基于节点Docker状态和用户信用评分将节点分为高、中、低三个信任等级，无Docker服务的节点强制归类为低信任等级
- **资源监控**：实时跟踪节点CPU、内存和GPU资源使用情况，维护总资源和可用资源的动态平衡
- **心跳检测**：监控节点存活状态，超过30秒无心跳的节点标记为不可用
- **动态状态管理**：支持节点状态（Idle/Running/Offline）的自动更新与维护

### 2. 任务协调机制
- **智能任务分配**：基于节点信任等级、资源利用率和地理位置实现负载均衡
- **任务生命周期管理**：处理任务创建、分配、执行、结果回收全流程
- **文件存储管理**：通过FileStorageManager管理任务和结果ZIP文件，支持自动清理机制
- **资源预留机制**：为高优先级任务预留计算资源，确保关键任务优先执行

### 3. 用户与信用系统
- **用户认证授权**：基于JWT的身份验证机制，支持用户注册、登录和令牌管理
- **信用评分体系**：维护用户信用评分（初始100分），影响节点信任等级和任务优先级
- **CPT代币管理**：支持用户间代币转账、余额查询和奖励发放
- **电子邮件验证**：集成Resend API实现用户邮箱验证，验证成功奖励100 CPT代币

### 4. 高性能通信
- **gRPC服务**：基于Protobuf定义节点通信协议，支持最大100MB消息传输
- **并发处理**：配置20个工作线程处理并发请求，支持数千节点同时连接
- **连接优化**：实现TCP长连接保持机制，减少连接建立开销
- **数据压缩**：对传输的任务数据进行压缩，降低网络带宽占用

## 系统架构

### 模块组成
```
node_pool/
├── __init__.py             # 包初始化
├── config.py               # 配置管理
├── database_manager.py     # SQLite数据库管理
├── database_migration.py   # 数据库迁移脚本
├── master_node_service.py  # 主节点服务实现
├── node_manager.py         # 节点管理核心逻辑
├── node_manager_service.py # 节点管理gRPC服务
├── node_pool_server.py     # gRPC服务器入口
├── nodepool_pb2.py         # Protobuf生成代码
├── nodepool_pb2_grpc.py    # gRPC生成代码
├── user_manager.py         # 用户管理与认证
└── user_service.py         # 用户服务gRPC实现
```

### 核心类关系
- **NodeManager**：节点注册、状态更新和资源管理
- **TaskManager**：任务存储、分配和生命周期管理
- **FileStorageManager**：任务文件存储与清理
- **DatabaseManager**：用户数据和信用评分管理
- **UserManager**：用户认证、授权和代币管理
- **NodePoolServer**：gRPC服务配置与启动

### 数据存储
1. **Redis**：存储节点实时状态信息
   - 节点信息：`node:{node_id}`哈希结构
   - 资源数据：CPU/内存/GPU使用率和可用资源
   - 任务状态：任务分配和执行状态

2. **SQLite**：存储用户数据和持久化信息
   - 用户表：存储用户名、密码哈希、信用评分和代币余额
   - 验证表：管理电子邮件验证和密码重置令牌

## API接口

### 节点管理API
```protobuf
// 节点注册
rpc RegisterNode(RegisterNodeRequest) returns (RegisterNodeResponse)

// 状态报告
rpc ReportStatus(StatusRequest) returns (StatusResponse)

// 获取节点列表
rpc GetNodeList(NodeListRequest) returns (NodeListResponse)

// 获取节点详情
rpc GetNodeDetails(NodeDetailsRequest) returns (NodeDetailsResponse)
```

### 用户服务API
```protobuf
// 用户注册
rpc RegisterUser(RegisterRequest) returns (RegisterResponse)

// 用户登录
rpc LoginUser(LoginRequest) returns (LoginResponse)

// 查询余额
rpc GetBalance(BalanceRequest) returns (BalanceResponse)

// 转账
rpc TransferTokens(TransferRequest) returns (TransferResponse)
```

### 任务管理API
```protobuf
// 创建任务
rpc CreateTask(CreateTaskRequest) returns (CreateTaskResponse)

// 分配任务
rpc AssignTask(AssignTaskRequest) returns (AssignTaskResponse)

// 提交结果
rpc SubmitResult(ResultRequest) returns (ResultResponse)
```

## 配置说明

### 核心配置项
```python
# gRPC服务器配置
GRPC_PORT = 50051
MAX_MESSAGE_SIZE = 100 * 1024 * 1024  # 100MB
THREAD_POOL_SIZE = 20

# Redis配置
REDIS_HOST = 'localhost'
REDIS_PORT = 6379
REDIS_DB = 0

# 任务存储配置
TASK_STORAGE_PATH = '/mnt/myusb/hivemind/task_storage'
CLEANUP_DELAY_SECONDS = 5

# 认证配置
TOKEN_EXPIRY_MINUTES = 60
SECRET_KEY = 'your-secret-key'

# 节点配置
HEARTBEAT_TIMEOUT = 30  # 秒
MIN_CREDIT_SCORE = 50
```

### 信任等级配置
| 信用评分范围 | 信任等级 | 任务分配优先级 | 资源限制 |
|------------|---------|--------------|---------|
| ≥ 100      | 高      | 最高         | 无限制   |
| 50-99      | 中      | 中等         | 80% 资源 |
| < 50       | 低      | 最低         | 50% 资源 |
| Docker禁用 | 低      | 最低         | 30% 资源 |

## 使用方法

### 启动服务器
```bash
# 直接运行
python3 node_pool_server.py

# 使用nohup后台运行
nohup python3 node_pool_server.py > node_pool.log 2>&1 &
```

### 节点注册流程
1. 节点启动时调用`RegisterNode` API提交节点信息
2. 服务器验证节点信息并分配信任等级
3. 节点定期（每10秒）调用`ReportStatus`更新状态
4. 服务器根据资源使用情况分配任务

### 监控与管理
- **节点状态监控**：通过`GetNodeList` API获取所有节点状态
- **资源使用统计**：解析Redis中的节点资源数据
- **日志查看**：服务器日志默认输出到控制台，可重定向到文件

## 开发指南

### 环境依赖
```
grpcio>=1.48.0
redis>=4.3.4
sqlite3>=2.6.0
bcrypt>=4.0.1
pyjwt>=2.6.0
python-dotenv>=1.0.0
```

### 代码规范
- 遵循PEP 8编码规范
- 使用类型注解提高代码可读性
- 关键函数添加文档字符串
- 使用logging模块记录日志，而非print
```

### 常见问题
1. **节点注册失败**
   - 检查Redis服务是否正常运行
   - 验证节点ID是否已存在
   - 检查网络连接和防火墙设置

2. **任务分配失败**
   - 检查节点资源是否充足
   - 验证节点信任等级是否满足任务要求
   - 查看任务存储目录权限

3. **认证失败**
   - 检查JWT令牌是否过期
   - 验证密钥是否匹配
   - 确认用户信用评分是否达标


## 许可证
本项目采用GNU General Public License v3.0许可证 - 详见项目根目录LICENSE文件。

## 联系信息
- 项目维护者: Justin
- 电子邮件: justin@hivemind.com
- 项目主页: https://hivemind.justin0711.com