# HiveMind Distributed Computing Platform

<div align="center">

[![Project Status](https://img.shields.io/badge/status-active-brightgreen.svg)](https://github.com/him6794/hivemind)
[![License](https://img.shields.io/badge/license-GPL%20v3-blue.svg)](LICENSE.txt)
[![Python Version](https://img.shields.io/badge/python-3.8%2B-blue.svg)](https://www.python.org/)
[![Docker](https://img.shields.io/badge/docker-required-blue.svg)](https://www.docker.com/)
[![Ask DeepWiki](https://deepwiki.com/badge.svg)](https://deepwiki.com/him6794/hivemind)

</div>

<div align="center">

> **🌍 Language / 語言選擇**
> 
> <a href="README.en.md">🇺🇸 English</a> | 
> <a href="README.zh-TW.md">🇹🇼 繁體中文</a> | 
> **📖 Current: English**

</div>

---

<div align="center">

**HiveMind** is an open-source distributed computing platform designed to build a decentralized computing network that allows users to share idle computing resources and earn token rewards.

</div>

## ✨ Core Features

<div align="center">

| Feature | Description | Status |
|---------|-------------|--------|
| 🌐 **Node Pool Management** | Distributed node registration and management | ✅ Active |
| 🔄 **Master-Worker Architecture** | Hierarchical task distribution system | ✅ Active |
| 💾 **Persistent Storage** | SQLite database with Redis caching | ✅ Active |
| 🌍 **Web Dashboard** | Task monitoring and system status | 🚧 Development |
| 🔒 **User Authentication** | Secure user and permission management | ✅ Active |
| 📡 **gRPC Communication** | High-performance inter-service communication | ✅ Active |
| 🔧 **Task Worker System** | Distributed task execution framework | ✅ Active |
| 📦 **BitTorrent Protocol** | P2P file sharing and distribution | 🚧 Development |

</div>

## Quick Start

### System Requirements
- **Operating System**: Linux (Ubuntu 18.04+), Windows 10+, macOS 10.15+
- **Python**: 3.8 or higher
- **Docker**: 20.10 or higher
- **Memory**: Minimum 2GB, recommended 4GB+

### 📥 Installation

<div align="center">

#### Manual Installation

</div>

```bash
# Clone the repository
git clone https://github.com/him6794/hivemind.git
cd hivemind

# Install dependencies
pip install -r requirements.txt
```

**Note**: Automated installation scripts are planned for future development.

### 🚀 Deployment Options

<div align="center">

| Option | Command | Recommended For |
|--------|---------|-----------------|
| **⚙️ Worker Node** | `cd worker && python worker_node.py` | Beginners, Resource Contribution |
| **🌐 Node Pool Service** | `cd node_pool && python node_pool_server.py` | Resource Management |
| **🎛️ Master Node** | `cd master && python master_node.py` | System Administration |

</div>

**Note**: Docker Compose configuration is under development.

> **VPN Configuration**: The VPN configuration file will be in your installation directory, and you will need to manually connect on Windows.

## 🏗️ System Architecture

<div align="center">

```
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│   🎛️ Master     │    │   🌐 Node Pool   │    │   ⚙️ Worker     │
│    Node         │◄──►│    Service      │◄──►│    Node         │
│                 │    │                 │    │                 │
│ • Task Mgmt     │    │ • Resource      │    │ • Task Exec     │
│ • User Interface│    │   Scheduling    │    │ • Status Mon    │
│ • VPN Mgmt      │    │ • Reward Dist   │    │ • Result Send   │
└─────────────────┘    └─────────────────┘    └─────────────────┘
```

</div>

### 🔧 Core Components

<div align="center">

| Component | Purpose | Implementation Status |
|-----------|---------|----------------------|
| **🌐 Node Pool** | Resource scheduling and task distribution | ✅ Complete |
| **⚙️ Worker Node** | Task execution and resource monitoring | ✅ Complete |
| **🎛️ Master Node** | Task management and user interface | ✅ Complete |
| **🤖 AI Module** | Distributed AI model training | 🚧 Development |
| **📦 BT Module** | P2P file transfer system | 🚧 Development |

</div>

## 💰 Reward System

<div align="center">

Earn **CPT (Computing Power Token)** by contributing computing resources:

| Reward Type | Rate | Description |
|-------------|------|-------------|
| **🏆 Base Reward** | 10 CPT/hour | Standard participation rate |
| **⚡ Performance Bonus** | +50% max | High-efficiency task completion |
| **🔄 Stability Bonus** | +30% | Consistent uptime maintenance |
| **💎 Quality Bonus** | Variable | Complex task execution |

</div>

**Note**: Token reward system is currently under development.

## 📚 Documentation

<div align="center">

### 📖 Complete Documentation

| Document | Description | Status |
|----------|-------------|--------|
| **[Main Documentation](documentation/README.md)** | Complete bilingual documentation center | ✅ Available |
| **[API Documentation](documentation/zh-tw/api.md)** | Complete gRPC API reference | ✅ Available |
| **[Deployment Guide](documentation/zh-tw/deployment.md)** | Detailed deployment instructions | ✅ Available |
| **[Troubleshooting](documentation/zh-tw/troubleshooting.md)** | Common issues and solutions | ✅ Available |
| **[Developer Guide](documentation/zh-tw/developer.md)** | Development standards and guidelines | ✅ Available |

### 🔧 Module Documentation

| Module | Description | Status |
|--------|-------------|--------|
| **[Node Pool](documentation/zh-tw/modules/node-pool.md)** | Resource scheduling system | ✅ Available |
| **[Master Node](documentation/zh-tw/modules/master-node.md)** | Management and monitoring | 🚧 Development |
| **[Worker Node](documentation/zh-tw/modules/worker-node.md)** | Worker node implementation | 🚧 Development |
| **[TaskWorker](documentation/zh-tw/modules/taskworker.md)** | Distributed task execution library | ✅ Available |
| **[AI Module](documentation/zh-tw/modules/ai.md)** | Distributed AI model execution | 🚧 Development |
| **[BT Module](documentation/zh-tw/modules/bt.md)** | P2P file transfer system | 🚧 Development |

</div>

## 🚀 Web Interface

After deployment, access the available interfaces:

<div align="center">

| Service | Default Port | Interface Type | Status |
|---------|--------------|----------------|--------|
| **Node Pool Service** | 50051 | gRPC | ✅ Active |
| **Master Node** | 5000 | Web Interface | ✅ Active |
| **Worker Node** | Individual | Status Monitoring | ✅ Active |

</div>

**Note**: Enhanced web dashboard interfaces are under development.

## 🤝 Community & Support

<div align="center">

### 🌟 Join Our Community

| Platform | Purpose | Link |
|----------|---------|------|
| **🔗 GitHub** | Main Repository | [Repository](https://github.com/him6794/hivemind) |
| **🐛 Issues** | Bug Reports & Feature Requests | [GitHub Issues](https://github.com/him6794/hivemind/issues) |
| **💬 Discussions** | Community Q&A | [GitHub Discussions](https://github.com/him6794/hivemind/discussions) |

**Note**: Discord and Telegram communities are planned for future development.

### 🆘 Support Channels

| Type | Platform | Status |
|------|----------|--------|
| **Bug Reports** | [GitHub Issues](https://github.com/him6794/hivemind/issues) | ✅ Active |
| **Feature Requests** | [GitHub Discussions](https://github.com/him6794/hivemind/discussions) | ✅ Active |
| **Contact** | GitHub Profile | ✅ Available |

**Note**: Dedicated support email addresses are planned for future development.

</div>

## 🚧 Development Status

<div align="center">

### ✅ Current Implementation (August 2025)

| Component | Status | Description |
|-----------|--------|-------------|
| **🏗️ Core Architecture** | ✅ Complete | Node Pool, Master, Worker modules |
| **🔧 Basic Functionality** | ✅ Complete | Node registration, task distribution, user management |
| **📡 Communication** | ✅ Complete | gRPC-based inter-node communication |
| **💾 Data Storage** | ✅ Complete | SQLite for user data, Redis for node state |

### 🎯 Future Development Goals

| Goal | Priority | Description |
|------|----------|-------------|
| **🧪 Testing Framework** | High | Comprehensive test suite implementation |
| **🤖 AI Integration** | Medium | Distributed AI model training capabilities |
| **🌍 Web Dashboard** | Medium | Enhanced monitoring and management interfaces |
| **🐳 Docker Compose** | Low | Simplified deployment configuration |

</div>

## License

This project is licensed under the **GNU General Public License v3.0** - see the [LICENSE](LICENSE.txt) file for details.

---

<div align="center">

**Join the HiveMind Distributed Computing Network**

*Transform your idle computing power into value and help build a stronger computing ecosystem*

[![GitHub Stars](https://img.shields.io/github/stars/him6794/hivemind?style=social)](https://github.com/him6794/hivemind)
[![License](https://img.shields.io/badge/license-GPL--3.0-blue.svg)](LICENSE.txt)

**[GitHub Repository](https://github.com/him6794/hivemind) | [Issues & Discussions](https://github.com/him6794/hivemind/issues)**

</div>
