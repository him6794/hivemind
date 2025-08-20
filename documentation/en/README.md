# HiveMind Distributed Computing Platform Documentation

Welcome to the complete documentation for HiveMind Distributed Computing Platform!

## Quick Navigation

### 📚 Core Documentation
- [Project Overview](#project-overview)
- [System Architecture](#system-architecture)
- [Quick Start](deployment.md#quick-start)
- [API Reference](api.md)

### 🔧 Deployment & Operations
- [Deployment Guide](deployment.md)
- [Troubleshooting](troubleshooting.md)
- [Performance Tuning](deployment.md#performance-optimization)

### 👨‍💻 Developer Documentation
- [Developer Guide](developer.md)
- [Contributing Guide](developer.md#contributing-guide)
- [Coding Standards](developer.md#coding-standards)

### 📖 Module Documentation
- [Node Pool Module](modules/node-pool.md)
- [Master Node Module](modules/master.md)
- [Worker Node Module](modules/worker.md)
- [AI Module](modules/ai.md)
- [BT Module](modules/bt.md)
- [TaskWorker Module](modules/taskworker.md)
- [Web Interface Module](modules/web.md)

## Project Overview

HiveMind is an open-source distributed computing platform designed to build a decentralized computing network that allows users to share idle computing resources and earn token rewards.

### Core Features

| Feature | Description | Status |
|---------|-------------|--------|
| 🌐 **Node Pool Management** | Distributed node registration and management | ✅ Implemented |
| 🔄 **Master-Worker Architecture** | Hierarchical task distribution system | ✅ Implemented |
| 💾 **Persistent Storage** | SQLite database with Redis caching | ✅ Implemented |
| 🌍 **Web Dashboard** | Task monitoring and system status | 🚧 In Development |
| 🔒 **User Authentication** | Secure user and permission management | ✅ Implemented |
| 📡 **gRPC Communication** | High-performance inter-service communication | ✅ Implemented |
| 🔧 **Task Worker System** | Distributed task execution framework | ✅ Implemented |
| 📦 **BitTorrent Protocol** | P2P file sharing and distribution | 🚧 In Development |

## System Architecture

```
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│   🎛️ Master     │    │   🌐 Node Pool   │    │   ⚙️ Worker     │
│    Node         │◄──►│    Service      │◄──►│    Node         │
│                 │    │                 │    │                 │
│ • Task Management│    │ • Resource      │    │ • Task Execution│
│ • User Interface │    │   Scheduling    │    │ • Status Monitor│
│ • VPN Management │    │ • Node Management│    │ • Result Report │
└─────────────────┘    └─────────────────┘    └─────────────────┘
```

### Core Components

| Component | Purpose | Implementation Status |
|-----------|---------|----------------------|
| **🌐 Node Pool** | Resource scheduling and task distribution | ✅ Complete |
| **⚙️ Worker Node** | Task execution and resource monitoring | ✅ Complete |
| **🎛️ Master Node** | Task management and user interface | ✅ Complete |
| **🤖 AI Module** | Distributed AI model training | 🚧 In Development |
| **📦 BT Module** | P2P file transfer system | 🚧 In Development |

## Quick Start

### System Requirements

- **Operating System**: Linux (Ubuntu 18.04+), Windows 10+, macOS 10.15+
- **Python**: 3.8 or higher
- **Docker**: 20.10 or higher
- **Memory**: Minimum 2GB, recommended 4GB+

### Installation Steps

```bash
# Clone the repository
git clone https://github.com/him6794/hivemind.git
cd hivemind

# Install dependencies
pip install -r requirements.txt

# Start Node Pool service
cd node_pool
python node_pool_server.py
```

For detailed installation and deployment instructions, see the [Deployment Guide](deployment.md).

## Reward System

Earn **CPT (Computing Power Token)** by contributing computing resources:

| Reward Type | Rate | Description |
|-------------|------|-------------|
| **🏆 Base Reward** | 10 CPT/hour | Standard participation rate |
| **⚡ Performance Bonus** | +50% max | High-efficiency task completion |
| **🔄 Stability Bonus** | +30% | Consistent uptime maintenance |
| **💎 Quality Bonus** | Variable | Complex task execution |

**Note**: Token reward system is currently under development.

## Development Status

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

## Community & Support

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

## License

This project is licensed under the **GNU General Public License v3.0** - see the [LICENSE](../LICENSE.txt) file for details.

---

<div align="center">

**Join the HiveMind Distributed Computing Network**

*Transform your idle computing power into value and help build a stronger computing ecosystem*

[![GitHub Stars](https://img.shields.io/github/stars/him6794/hivemind?style=social)](https://github.com/him6794/hivemind)
[![License](https://img.shields.io/badge/license-GPL--3.0-blue.svg)](../LICENSE.txt)

**[GitHub Repository](https://github.com/him6794/hivemind) | [Issues & Discussions](https://github.com/him6794/hivemind/issues)**

</div>
