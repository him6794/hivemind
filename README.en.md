# HiveMind Distributed Computing Platform

[![Project Status](https://img.shields.io/badge/status-active-brightgreen.svg)](https://github.com/him6794/hivemind)
[![License](https://img.shields.io/badge/license-GPL%20v3-blue.svg)](LICENSE.txt)
[![Python Version](https://img.shields.io/badge/python-3.8%2B-blue.svg)](https://www.python.org/)
[![Docker](https://img.shields.io/badge/docker-required-blue.svg)](https://www.docker.com/)
[![Ask DeepWiki](https://deepwiki.com/badge.svg)](https://deepwiki.com/him6794/hivemind)

HiveMind is an open-source distributed computing platform designed to build a decentralized computing network that allows users to share idle computing resources and earn token rewards.

## System Architecture

HiveMind adopts a three-tier distributed architecture consisting of the following core components:

```
┌─────────────┐    ┌─────────────┐    ┌─────────────┐
│   Master    │    │ Node Pool   │    │   Worker    │
│ Master Node │◄──►│ Node Pool   │◄──►│ Worker Node │
│             │    │             │    │             │
│ Task Mgmt   │    │ Resource    │    │ Task Exec   │
│ User Interface│  │ Scheduling  │    │ Status Mon  │
│ VPN Mgmt    │    │ Load Balance│    │ Result Send │
└─────────────┘    └─────────────┘    └─────────────┘
```

### Core Functional Modules

#### Node Pool
- **Resource Scheduling**: Intelligently allocates computing tasks to the most suitable worker nodes
- **Task Distribution**: Maintains task queues, handles task priorities and dependencies
- **Reward System**: Calculates and distributes CPT token rewards based on contribution
- **Node Management**: Monitors node status and maintains node trust levels

#### Worker (Worker Node)
- **Task Execution**: Safely executes computing tasks in Docker containers
- **Resource Monitoring**: Real-time monitoring of CPU, memory, and GPU usage
- **Status Reporting**: Regularly reports node health status to the node pool
- **Result Transmission**: Securely returns task results to the node pool

#### Master (Master Node)
- **Task Management**: Provides task creation, monitoring, and management interface
- **User Authentication**: Handles user registration, login, and permission management
- **VPN Service**: Manages secure communication tunnels between nodes
- **Monitoring Dashboard**: Provides visualization interface for system status and task execution

### Auxiliary Modules

#### 🤖 AI (Artificial Intelligence)
- **Model Splitting**: Breaks down large AI models into distributable small tasks
- **Intelligent Scheduling**: Optimizes task allocation strategies based on model characteristics
- **Status**: 🚧 Under Development

#### 📁 BT (P2P Transfer)
- **Large File Transfer**: Supports peer-to-peer transfer of large task files
- **Torrent Management**: Creates and manages BitTorrent seed files
- **Status**: Completed, pending integration

#### Web (Official Website)
- **Project Showcase**: Project introduction and feature description
- **User Registration**: Online user registration and management
- **Status Monitoring**: Real-time system status display
- **Access URL**: https://hivemind.justin0711.com

## 🔌 Communication Protocol & Data Format

HiveMind uses **gRPC** as the inter-node communication protocol and **Protocol Buffers** to define data formats, ensuring high performance and cross-platform compatibility.

### Main API Interfaces

#### User Authentication Service
```protobuf
// User Login
message LoginRequest {
  string username = 1;    // Username
  string password = 2;    // Password
}

message LoginResponse {
  bool success = 1;       // Whether login is successful
  string token = 2;       // JWT authentication token
  string message = 3;     // Response message
}

// User Registration
message RegisterRequest {
  string username = 1;    // Username
  string password = 2;    // Password
  string email = 3;       // Email address
}

message RegisterResponse {
  bool success = 1;       // Whether registration is successful
  string message = 2;     // Response message
}
```

#### Token Management Service
```protobuf
// Token Transfer
message TransferRequest {
  string token = 1;           // User authentication token
  string to_username = 2;     // Recipient username
  int64 amount = 3;          // Transfer amount (in CPT)
}

message TransferResponse {
  bool success = 1;          // Whether transfer is successful
  string message = 2;        // Response message
  int64 new_balance = 3;     // New balance after transfer
}

// Balance Query
message BalanceRequest {
  string token = 1;          // User authentication token
}

message BalanceResponse {
  bool success = 1;          // Whether query is successful
  int64 balance = 2;         // Current balance (in CPT)
  string message = 3;        // Response message
}
```

#### Task Management Service
```protobuf
// Task Submission
message SubmitTaskRequest {
  string token = 1;              // User authentication token
  string task_name = 2;          // Task name
  string docker_image = 3;       // Docker image name
  string command = 4;            // Command to execute
  repeated string files = 5;     // Task files (Base64 encoded)
  int32 priority = 6;           // Task priority (1-10)
  int64 max_runtime = 7;        // Maximum runtime (seconds)
  int64 required_memory = 8;     // Required memory (MB)
  int32 required_cpu_cores = 9;  // Required CPU cores
}

message SubmitTaskResponse {
  bool success = 1;              // Whether submission is successful
  string task_id = 2;            // Task ID
  string message = 3;            // Response message
}

// Task Status Query
message TaskStatusRequest {
  string token = 1;              // User authentication token
  string task_id = 2;            // Task ID
}

message TaskStatusResponse {
  bool success = 1;              // Whether query is successful
  string status = 2;             // Task status (pending/running/completed/failed)
  string result = 3;             // Task result (if completed)
  string error_message = 4;      // Error message (if failed)
  int32 progress = 5;           // Task progress (0-100)
}
```

#### Node Registration Service
```protobuf
// Worker Node Registration
message RegisterNodeRequest {
  string node_id = 1;            // Node unique identifier
  string ip_address = 2;         // Node IP address
  int32 port = 3;               // Node port
  int32 cpu_cores = 4;          // Available CPU cores
  int64 memory_mb = 5;          // Available memory (MB)
  int64 disk_gb = 6;            // Available disk space (GB)
  bool has_gpu = 7;             // Whether GPU is available
  string gpu_info = 8;          // GPU information
}

message RegisterNodeResponse {
  bool success = 1;              // Whether registration is successful
  string message = 2;            // Response message
  string vpn_config = 3;         // VPN configuration file
}

// Resource Status Report
message ResourceReportRequest {
  string node_id = 1;            // Node ID
  float cpu_usage = 2;           // CPU usage percentage
  float memory_usage = 3;        // Memory usage percentage
  float disk_usage = 4;          // Disk usage percentage
  float gpu_usage = 5;           // GPU usage percentage (if available)
  int32 active_tasks = 6;        // Number of active tasks
}

message ResourceReportResponse {
  bool success = 1;              // Whether report is successful
  string message = 2;            // Response message
}
```

### Security Features

#### Secure Communication
- **WireGuard VPN**: All node communication is protected by WireGuard VPN tunnels
- **TLS Encryption**: gRPC communications use TLS encryption
- **JWT Authentication**: User authentication uses JSON Web Tokens
- **Certificate Management**: Automatic certificate generation and rotation

#### Container Security
- **Resource Isolation**: Each task runs in an isolated Docker container
- **Resource Limits**: Strict limits on CPU, memory, and disk usage
- **Network Isolation**: Tasks cannot access external networks without permission
- **Privilege Control**: Containers run with minimal privileges

## Quick Start

### System Requirements

- **Operating System**: Linux (Ubuntu 18.04+), Windows 10+, macOS 10.15+
- **Python**: 3.8 or higher
- **Docker**: 20.10 or higher
- **Memory**: Minimum 2GB, recommended 4GB+
- **Storage**: Minimum 10GB free space
- **Network**: Stable internet connection

### Installation Steps

#### 1. Install Dependencies

**Ubuntu/Debian:**
```bash
# Update system packages
sudo apt update && sudo apt upgrade -y

# Install Docker
curl -fsSL https://get.docker.com -o get-docker.sh
sudo sh get-docker.sh
sudo usermod -aG docker $USER

# Install Python and pip
sudo apt install python3 python3-pip git -y

# Install WireGuard
sudo apt install wireguard -y
```

**Windows:**
```powershell
# Install Docker Desktop
# Download from: https://www.docker.com/products/docker-desktop

# Install Python
# Download from: https://www.python.org/downloads/

# Install WireGuard
# Download from: https://www.wireguard.com/install/

# Install Git
# Download from: https://git-scm.com/download/win
```

#### 2. Clone Repository
```bash
git clone https://github.com/him6794/hivemind.git
cd hivemind
```

#### 3. Install Python Dependencies
```bash
pip install -r requirements.txt
```

### Deployment Options

#### Option 1: Deploy as Worker Node (Recommended for beginners)
```bash
cd worker
python worker_node.py --register
```

#### Option 2: Deploy Node Pool Service
```bash
cd node_pool
python node_pool_server.py
```

#### Option 3: Deploy Master Node
```bash
cd master
python master_node.py
```

#### Option 4: Deploy Complete System (Advanced)
```bash
# Deploy using Docker Compose
docker-compose up -d

# Or deploy using the provided script
./deploy.sh --mode=full
```

### Web Interface

After successful deployment, you can access:

- **Master Node Dashboard**: http://localhost:8080
- **Node Pool Monitoring**: http://localhost:8081  
- **Worker Node Status**: http://localhost:8082
- **Official Website**: https://hivemind.justin0711.com

## Reward Mechanism

### 💰 CPT Token System

HiveMind uses CPT (Computing Power Token) as the internal reward mechanism:

- **Base Reward**: 10 CPT per hour of active participation
- **Performance Bonus**: Additional rewards based on task completion efficiency
- **Stability Bonus**: Extra rewards for maintaining consistent uptime
- **Quality Bonus**: Rewards for successfully completing complex tasks

### Reward Calculation

```python
# Reward calculation formula
base_reward = runtime_hours * 10
performance_bonus = (completed_tasks / total_tasks) * base_reward * 0.5
stability_bonus = (uptime_percentage / 100) * base_reward * 0.3
quality_bonus = successful_complex_tasks * 5

total_reward = base_reward + performance_bonus + stability_bonus + quality_bonus
```

### Contribution Levels

| Level | Requirements | Multiplier | Benefits |
|-------|--------------|------------|----------|
| Bronze | 100+ hours | 1.0x | Basic rewards |
| Silver | 500+ hours | 1.2x | Priority task assignment |
| Gold | 1000+ hours | 1.5x | Advanced task access |
| Platinum | 2000+ hours | 2.0x | Beta feature access |
| Diamond | 5000+ hours | 3.0x | Network governance rights |

## Documentation

### Complete Documentation
- **[API Documentation](docs/API.md)**: Complete gRPC API reference
- **[Deployment Guide](docs/DEPLOYMENT.md)**: Detailed deployment instructions
- **[Troubleshooting](docs/TROUBLESHOOTING.md)**: Common issues and solutions
- **👨‍[Developer Guide](docs/DEVELOPER.md)**: Development standards and guidelines
- **[Change Log](docs/CHANGELOG.md)**: Version history and updates
- **[Contributing Guide](docs/CONTRIBUTING.md)**: How to contribute to the project

### Module Documentation
- **🤖 [AI Module](ai/README.md)**: Distributed AI model execution
- **📁 [BT Module](bt/README.md)**: P2P file transfer system  
- **[Worker Module](worker/README.md)**: Worker node implementation
- **[Node Pool](node_pool/README.md)**: Resource scheduling system
- **[Master Node](master/README.md)**: Management and monitoring

### Language Support
- **English**: [README.en.md](README.en.md) (This document)
- **繁體中文**: [README.zh-TW.md](README.zh-TW.md)

## Community & Support

### Join Our Community
- **Discord**: [Join HiveMind Community](https://discord.gg/hivemind)
- **Telegram**: [@HiveMindEN](https://t.me/HiveMindEN)
- **🐦 Twitter**: [@HiveMindNet](https://twitter.com/HiveMindNet)
- **📺 YouTube**: [HiveMind Channel](https://youtube.com/@hivemind)

### Support Channels
- **📧 Technical Support**: [support@hivemind.justin0711.com](mailto:support@hivemind.justin0711.com)
- **Bug Reports**: [GitHub Issues](https://github.com/him6794/hivemind/issues)
- **Feature Requests**: [GitHub Discussions](https://github.com/him6794/hivemind/discussions)
- **Documentation**: [docs.hivemind.justin0711.com](https://docs.hivemind.justin0711.com)

### 🔄 Regular Updates
- **📅 Release Schedule**: Monthly releases on the 15th
- **🔔 Announcements**: Follow our social media for updates
- **📰 Newsletter**: Subscribe for development updates
- **Roadmap**: Available on our project website

## Roadmap

### Phase 1: Foundation (Q1 2025) - Core node pool architecture
- Basic worker node implementation  
- Simple task execution system
- Web monitoring interface

### 🧠 Phase 2: AI Integration (Q2 2025) 🚧
- 🚧 Distributed AI model training
- 🚧 Model splitting and merging
- Federated learning support
- GPU resource optimization

### Phase 3: Network Expansion (Q3 2025) - Multi-region deployment
- Cross-platform mobile apps
- Enhanced security features
- Performance optimizations

### Phase 4: Governance (Q4 2025) - Decentralized governance system
- Community voting mechanisms
- Advanced reward algorithms
- Enterprise solutions

### Legend:
- Completed
- 🚧 In Progress  
- Planned

## License

This project is licensed under the **GNU General Public License v3.0** - see the [LICENSE](LICENSE.txt) file for details.

### License Summary
- **Commercial Use**: Permitted for commercial purposes
- **Modification**: Allowed to modify the source code
- **Distribution**: Allowed to distribute modified versions
- **Patent License**: Provides patent protection
- **Private Use**: Allowed for private use

### License Conditions
- **Disclose Source**: Must provide source code when distributing
- **License Notice**: Must include license and copyright notice
- 🔄 **Same License**: Derivative works must use the same license
- **State Changes**: Must document changes when modifying files

## 🙏 Acknowledgments

### Inspirations
- **BOINC**: Pioneering distributed computing platform
- **Folding@home**: Medical research through distributed computing
- **IPFS**: Decentralized file storage inspiration
- **Kubernetes**: Container orchestration concepts

### Technologies Used
- **gRPC**: High-performance RPC framework
- **Protocol Buffers**: Efficient data serialization
- **Docker**: Container platform for secure task execution
- **WireGuard**: Modern VPN protocol for secure communication
- **Redis**: In-memory data structure store
- **SQLite/PostgreSQL**: Database solutions
- **React**: Modern web framework for user interfaces

### 👨‍Contributors
We thank all contributors who have helped make HiveMind better:
- See [CONTRIBUTORS.md](CONTRIBUTORS.md) for the complete list

---

<div align="center">

**Join the HiveMind Distributed Computing Network **

*Transform your idle computing power into value and help build a stronger computing ecosystem*

[![GitHub Stars](https://img.shields.io/github/stars/him6794/hivemind?style=social)](https://github.com/him6794/hivemind)
[![Discord](https://img.shields.io/discord/123456789?style=social&logo=discord)](https://discord.gg/hivemind)
[![License](https://img.shields.io/badge/license-GPL--3.0-blue.svg)](LICENSE.txt)

**[Official Website](https://hivemind.justin0711.com) | [Documentation](docs/README.md) | [Community](https://discord.gg/hivemind) | [Report Issues](https://github.com/him6794/hivemind/issues)**

</div>
