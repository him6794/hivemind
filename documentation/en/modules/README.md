# Module Documentation

This directory contains detailed technical documentation for all HiveMind modules.

## Module List

### 🔄 Core Service Modules

- **[Node Pool](node-pool.md)** - Node Pool Service
  - Multi-level trust system management
  - Intelligent task scheduling and allocation
  - Dynamic resource tracking and allocation
  - JWT user authentication system
  - High-performance gRPC communication

- **[Master Node](master-node.md)** - Master Control Node
  - Web management interface
  - VPN network management
  - System monitoring and reporting
  - Task coordination and distribution

- **[Worker Node](worker-node.md)** - Worker Execution Nodes
  - Multi-task parallel execution engine
  - Docker containerized secure execution  
  - VPN auto-connection management
  - Trust scoring system
  - Modern Web management interface
  - Real-time resource monitoring and health checks

### 📦 Task Execution Framework

- **[TaskWorker](taskworker.md)** - Task Execution Library
  - Lightweight task execution framework
  - gRPC service interfaces
  - Independent deployment capability
  - Optional integration with Node Pool

### 🌐 Web Service Module

- **[Web Module](web.md)** - Web Service Interface
  - Flask web application
  - VPN service management
  - WireGuard server integration
  - User interface and management tools

### 🤖 Artificial Intelligence Module

- **[AI Module](ai.md)** - Machine Learning Service
  - Model identification and analysis
  - Reinforcement learning algorithms
  - Distributed training support
  - Q-learning task scheduling optimization

### 🌐 File Transfer Module

- **[BT Module](bt.md)** - BitTorrent P2P Transfer
  - Torrent file creation and management
  - P2P network integration
  - Distributed file sharing
  - Tracker and seeder services

## Module Dependencies

```
Node Pool (Core Scheduling Service)
├── Master Node (depends on Node Pool API)
├── Worker Node (depends on Node Pool registration)
└── TaskWorker (independent, optional integration)

AI Module (depends on Node Pool resource management)
BT Module (independent P2P service)
Web Module (independent web service, optional integration)
```

## Development Status

| Module | Status | Completion | Key Features |
|--------|--------|------------|--------------|
| Node Pool | ✅ Production Ready | 100% | Multi-trust, Dynamic Resources, gRPC |
| Master Node | ✅ Production Ready | 100% | VPN Management, Web Interface |
| Worker Node | ✅ Production Ready | 100% | Multi-task, Docker, Monitoring |
| TaskWorker | ✅ Production Ready | 100% | Lightweight, Independent Deployment |
| AI Module | 🔄 In Development | 30% | Q-learning, Model Analysis |
| BT Module | ✅ Beta Version | 85% | P2P Transfer, Seed Management |
| Web Module | ✅ Production Ready | 90% | Flask App, VPN Services |

## Architecture Overview

### Trust Level System (Node Pool Core)
```
High Trust Nodes (Credit ≥ 100, Docker Enabled)
├── Priority task allocation
├── Full feature access
└── Maximum reward coefficient

Normal Trust Nodes (Credit 50-99, Docker Enabled)
├── Standard task allocation
├── General feature access
└── Standard reward coefficient

Low Trust Nodes (Credit < 50 or No Docker)
├── Limited task types
├── Basic feature access
└── Basic reward coefficient
```

### Resource Management System
- **Dynamic Allocation**: Real-time tracking of total and available resources
- **Multi-task Support**: Parallel execution of multiple tasks on single nodes
- **Load Balancing**: Intelligent distribution algorithm
- **Geographic Awareness**: Support for location-based prioritization

## Quick Navigation

### By Development Role

**System Administrators**:
- [Deployment Guide](../deployment.md)
- [Troubleshooting](../troubleshooting.md)
- [Node Pool Configuration](node-pool.md#deployment-and-configuration)

**Developers**:
- [Developer Guide](../developer.md)
- [TaskWorker API](taskworker.md)
- [gRPC API Documentation](../api.md)

**Users**:
- [Quick Start](../README.md#quick-start)
- [Master Node Usage](master-node.md)
- [Web Interface](web.md)

### By Functionality

**Core Architecture**:
- [Node Pool](node-pool.md) - Central scheduling service
- [Master Node](master-node.md) - System control console
- [Worker Node](worker-node.md) - Compute execution units

**Extended Features**:
- [AI Module](ai.md) - Intelligent optimization features
- [BT Module](bt.md) - P2P file sharing
- [Web Module](web.md) - Web management interface

**Development Tools**:
- [TaskWorker](taskworker.md) - Task execution library
- [API Documentation](../api.md) - Complete interface specifications

### By Technology Stack

**Python + gRPC**:
- Node Pool, Worker Node, TaskWorker

**Python + Flask**:
- Master Node, Web Module

**Python + ML/AI**:
- AI Module

**Python + P2P**:
- BT Module

---

**Last Updated**: September 5, 2025  
**Maintenance Status**: All module documentation synchronized with actual code  
**Documentation Version**: v2.0 (Based on actual code analysis)
