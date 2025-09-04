<h1 align="center">HiveMind</h1>
<h3 align="center">Distributed Computing Platform</h3>

<p align="center">
  <strong>Transform idle computing resources into a powerful distributed network</strong>
</p>

<div align="center">

[![Project Status](https://img.shields.io/badge/status-active%20development-brightgreen.svg)](https://github.com/him6794/hivemind)
[![License](https://img.shields.io/badge/license-GPL%20v3-blue.svg)](LICENSE.txt)
[![Python Version](https://img.shields.io/badge/python-3.8%2B-blue.svg)](https://www.python.org/)
[![Docker](https://img.shields.io/badge/docker-ready-blue.svg)](https://www.docker.com/)
[![Contributions Welcome](https://img.shields.io/badge/contributions-welcome-orange.svg)](CONTRIBUTING.md)

</div>

---

## What is HiveMind?

HiveMind is an **open-source distributed computing platform** that revolutionizes how we utilize computing resources. By connecting idle machines across the globe, HiveMind creates a powerful, decentralized computing network capable of handling complex computational tasks, AI model training, and large-scale data processing.

### Key Highlights

- **Decentralized Network**: No single point of failure, truly distributed architecture
- **High Performance**: Optimized task scheduling and resource allocation  
- **Secure by Design**: End-to-end encryption with WireGuard VPN
- **AI-Powered**: Intelligent task distribution and resource optimization
- **Container-Native**: Full Docker integration for seamless deployment
- **Cross-Platform**: Windows, Linux, and macOS support

## Quick Navigation

<div align="center">

| **Documentation** | **Getting Started** | **Development** |
|:---:|:---:|:---:|
| [📚 官網文檔中心](web/DOCS_README.md) | [Quick Start](#quick-start) | [Contributing](CONTRIBUTING.md) |
| [English Docs](documentation/en/README.md) | [Docker Setup](#docker-deployment) | [API Reference](documentation/en/api.md) |
| [中文文檔](documentation/zh-tw/README.md) | [Installation](#installation) | [Changelog](CHANGELOG.md) |
| [Module Docs](documentation/en/modules/README.md) | [Web Docs](#-新增web-文檔系統) | [GitHub Issues](https://github.com/him6794/hivemind/issues) |

</div>

### 🌐 新增：Web 文檔系統

我們全新打造了現代化的 Web 文檔系統，提供更好的閱讀體驗：

- **🎯 文檔中心：** [啟動 Web 服務](web/DOCS_README.md) 後訪問 `http://localhost:5000/docs/zh/`
- **📥 安裝指南：** 完整的安裝教程和故障排除
- **🚀 快速開始：** 5分鐘體驗 HiveMind 核心功能  
- **🔧 API 文檔：** 完整的 gRPC API 參考和代碼示例
- **❓ 常見問題：** 智能搜索的 FAQ 系統

```bash
# 啟動 Web 文檔系統
cd web
python app.py
# 然後訪問 http://localhost:5000/docs/zh/
```

## System Architecture

HiveMind employs a sophisticated multi-layered architecture designed for scalability, reliability, and performance:

```
┌─────────────────────────────────────────────────────────────┐
│                    HiveMind Platform                         │
├─────────────────────────────────────────────────────────────┤
│  Web Dashboard    │  Master Node   │  Task Manager         │
├─────────────────────────────────────────────────────────────┤
│                    Node Pool Service                         │
│             (Resource Scheduling & Management)               │
├─────────────────────────────────────────────────────────────┤
│  Worker Node 1  │  Worker Node 2  │  Worker Node N        │
├─────────────────────────────────────────────────────────────┤
│  Container Layer │  VPN Network   │  Monitoring           │
└─────────────────────────────────────────────────────────────┘
```

### Core Components

| Component | Purpose | Technology Stack |
|-----------|---------|------------------|
| **Node Pool** | Central resource management and task scheduling | Python, gRPC, SQLite |
| **Worker Nodes** | Distributed task execution units | Docker, Python, Resource Monitoring |
| **Master Node** | System orchestration and web interface | Flask, WireGuard VPN |
| **TaskWorker** | Lightweight task execution framework | gRPC, Protocol Buffers |
| **AI Module** | Intelligent resource optimization | Q-Learning, Model Analysis |
| **BT Module** | P2P file transfer and distribution | BitTorrent Protocol |

## Quick Start

### Prerequisites

- **Python 3.8+** - Core runtime environment
- **Docker 20.10+** - Container orchestration
- **Git** - Version control
- **4GB+ RAM** - Recommended for optimal performance

### Installation

#### Option 1: Quick Setup (Recommended)
```bash
# Clone the repository
git clone https://github.com/him6794/hivemind.git
cd hivemind

# Install Python dependencies
pip install -r requirements.txt

# Quick health check
python -c "import sys; print(f'Python {sys.version} ready')"
```

#### Option 2: Development Setup
```bash
# Clone with full development tools
git clone --recurse-submodules https://github.com/him6794/hivemind.git
cd hivemind

# Setup virtual environment
python -m venv venv
source venv/bin/activate  # On Windows: venv\Scripts\activate

# Install with development dependencies
pip install -r requirements.txt
```

### Deployment Options

<div align="center">

| **Component** | **Launch Command** | **Description** |
|:---:|:---:|:---:|
| **Node Pool** | `cd node_pool && python node_pool_server.py` | Central management service |
| **Worker Node** | `cd worker && python worker_node.py` | Computational worker |
| **Master Node** | `cd master && python master_node.py` | Web interface & orchestration |
| **TaskWorker** | `cd taskworker && python worker.py` | Lightweight task executor |

</div>

### Docker Deployment

```bash
# Build HiveMind containers
docker-compose build

# Launch complete platform
docker-compose up -d

# View system status
docker-compose ps
```

## Feature Status

<div align="center">

| **Feature** | **Description** | **Status** | **Tech Stack** |
|:---:|:---:|:---:|:---:|
| **Node Pool** | Resource scheduling & task distribution | ✅ **Production** | gRPC, SQLite, Redis |
| **Worker Nodes** | Multi-task execution with containerization | ✅ **Production** | Docker, Python, Flask, VPN |
| **Master Node** | Web interface & system orchestration | ✅ **Production** | Flask, WireGuard VPN |
| **TaskWorker** | Lightweight task execution framework | ✅ **Production** | gRPC, Protocol Buffers |
| **AI Module** | Intelligent resource optimization | **Development** | Q-Learning, TensorFlow |
| **BT Module** | P2P file transfer & distribution | ✅ **Beta** | BitTorrent Protocol |
| **Web Dashboard** | Real-time monitoring & management | **Development** | React, Chart.js |
| **Security Layer** | End-to-end encryption & authentication | ✅ **Production** | JWT, WireGuard |

</div>

## Use Cases

<div align="center">

| **Application** | **Use Case** | **Benefits** |
|:---:|:---:|:---:|
| **AI/ML Training** | Distributed model training | Faster convergence, cost reduction |
| **Scientific Computing** | Climate modeling, simulations | Massive parallel processing |
| **Media Processing** | Video encoding, rendering | Resource pooling, faster output |
| **Data Analytics** | Data processing pipelines | Scalable distributed computing |

</div>

### Getting Started in 3 Steps

```bash
# Step 1: Download & Install
git clone https://github.com/him6794/hivemind.git
cd hivemind && pip install -r requirements.txt

# Step 2: Choose Your Role
# Resource Provider: cd worker && python worker_node.py
# Task Creator: cd master && python master_node.py
# Network Admin: cd node_pool && python node_pool_server.py

# Step 3: Access the Dashboard
# Open your browser to http://localhost:5000
```

**Congratulations!** You're now part of the HiveMind distributed computing network!

## Connect & Contribute

<div align="center">

### Join the Community

| **Channel** | **Purpose** | **Link** |
|:---:|:---:|:---:|
| **Star the Project** | Show your support | [GitHub Repository](https://github.com/him6794/hivemind) |
| **Report Issues** | Help us improve | [Issue Tracker](https://github.com/him6794/hivemind/issues) |
| **Feature Requests** | Shape the future | [Discussions](https://github.com/him6794/hivemind/discussions) |
| **Contribute Code** | Build together | [Contributing Guide](CONTRIBUTING.md) |
| **Documentation** | Improve docs | [Documentation Hub](documentation/) |

</div>

## What's Next?

<div align="center">

### Roadmap 2025

| **Quarter** | **Major Features** | **Impact** |
|:---:|:---:|:---:|
| **Q1 2025** | Enhanced Web Dashboard, Mobile App | Better user experience |
| **Q2 2025** | Blockchain Integration, Token Economy | Decentralized incentives |
| **Q3 2025** | GPU Computing Support, AI Workloads | Advanced computation |
| **Q4 2025** | Enterprise Features, SLA Guarantees | Commercial adoption |

</div>

---

<div align="center">

## License & Legal

**HiveMind** is open-source software licensed under the **GPL v3 License**.

[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](https://www.gnu.org/licenses/gpl-3.0)

### Contributing

We welcome contributions from developers worldwide! Please read our [Contributing Guidelines](CONTRIBUTING.md) before submitting pull requests.

### Acknowledgments

Special thanks to all contributors, early adopters, and the open-source community that makes projects like HiveMind possible.

---

<p align="center">
  <strong>Ready to revolutionize computing? Join HiveMind today!</strong>
</p>

<p align="center">
  <a href="https://github.com/him6794/hivemind/stargazers">⭐ Star</a> •
  <a href="https://github.com/him6794/hivemind/fork">🍴 Fork</a> •
  <a href="https://github.com/him6794/hivemind/issues">🐛 Report Bug</a> •
  <a href="https://github.com/him6794/hivemind/discussions">💬 Discussion</a>
</p>

</div>
