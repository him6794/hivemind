# Module Documentation

This directory contains detailed technical documentation for all HiveMind modules.

## Module List

### 📦 Core Modules

- **[TaskWorker](taskworker.md)** - Task Execution Library
  - Lightweight task execution framework
  - gRPC service interfaces
  - Independent deployment capability

### 🔄 Core Services

- **[Node Pool](node-pool.md)** - Node Pool Service
  - Node management and registration
  - Task scheduling and allocation
  - User authentication and management

- **[Master Node](master-node.md)** - Master Control Node
  - Web management interface
  - VPN network management
  - System monitoring and reporting

- **[Worker Node](worker-node.md)** - Worker Nodes
  - Task execution engine
  - Resource monitoring
  - Status reporting

### � Artificial Intelligence Module

- **[AI Module](ai.md)** - Machine Learning Service
  - Model identification and analysis
  - Reinforcement learning algorithms
  - Distributed training support

### 🌐 File Transfer Module

- **[BT Module](bt.md)** - BitTorrent P2P Transfer
  - Torrent file creation and management
  - P2P network integration
  - Distributed file sharing

### 🌐 Web Services

- **[Web Module](web.md)** - Web Service Module
  - Flask web application
  - VPN service management
  - WireGuard server
  - User interface

## Module Dependencies

```
Node Pool (Core)
├── Master Node (depends on Node Pool)
├── Worker Node (depends on Node Pool)
└── TaskWorker (independent, optional integration)

AI Module (depends on Node Pool)
BT Module (independent)
Web Module (independent, optional integration)
```

## Development Status

| Module | Status | Completion | Description |
|--------|--------|------------|-------------|
| Node Pool | ✅ Complete | 100% | Core functionality complete |
| Master Node | ✅ Complete | 100% | Management features complete |
| Worker Node | ✅ Complete | 100% | Execution features complete |
| TaskWorker | ✅ Complete | 100% | Independent library complete |
| AI Module | 🔄 In Development | 30% | Basic features implemented |
| BT Module | ✅ Complete | 100% | P2P functionality complete |
| Web Module | ✅ Complete | 90% | Basic features complete |

## Quick Navigation

### By Development Role

**System Administrators**:
- [Deployment Guide](../deployment.md)
- [Troubleshooting](../troubleshooting.md)
- [Node Pool](node-pool.md)

**Developers**:
- [Developer Guide](../developer.md)
- [TaskWorker](taskworker.md)
- [API Documentation](../api.md)

**Users**:
- [Quick Start](../README.md#quick-start)
- [Master Node](master-node.md)
- [Web Module](web.md)

### By Functionality

**Core Architecture**:
- [Node Pool](node-pool.md) - Node management
- [Master Node](master-node.md) - System control
- [Worker Node](worker-node.md) - Task execution

**Extended Features**:
- [AI Module](ai.md) - Intelligent features
- [BT Module](bt.md) - File transfer
- [Web Module](web.md) - Web services

**Development Tools**:
- [TaskWorker](taskworker.md) - Task library
- [API Documentation](../api.md) - Interface specifications

---

**Updated**: January 2024  
**Maintenance Status**: All module documentation synchronized with actual code
