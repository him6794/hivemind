# HiveMind Deployment Guide

## Overview

This guide helps you deploy the HiveMind distributed computing platform. Currently supports basic Python environment deployment, with Docker containerization under development.

## System Requirements

### Minimum Hardware Requirements

#### Node Pool Server
- **CPU**: 4 cores 2.0GHz+
- **Memory**: 8GB RAM
- **Storage**: 100GB SSD
- **Network**: 1Gbps bandwidth
- **OS**: Ubuntu 20.04 LTS / CentOS 8 / Windows Server 2019

#### Master Node Server
- **CPU**: 2 cores 2.0GHz+
- **Memory**: 4GB RAM
- **Storage**: 50GB SSD
- **Network**: 100Mbps bandwidth

#### Worker Node (Recommended)
- **CPU**: 8+ cores
- **Memory**: 16GB+ RAM
- **GPU**: NVIDIA GTX 1060 or higher (optional, requires pynvml)
- **Storage**: 20GB available space
- **Network**: 50Mbps+ bandwidth

### Software Dependencies

- **Python**: 3.8+
- **Redis**: 6.0+ (required)
- **Docker**: 20.10+ (Worker nodes only)

**Important Note**: The project currently uses Redis for node state management and SQLite for user data storage.

## Quick Start

### 1. Clone Project

```bash
git clone https://github.com/him6794/hivemind.git
cd hivemind
```

### 2. Install Python Dependencies

```bash
# Install global dependencies
pip install -r requirements.txt

# Install module-specific dependencies
pip install -r master/requirements.txt
pip install -r worker/requirements.txt
pip install -r taskworker/requirements.txt
```

### 3. Install Redis

#### Ubuntu/Debian
```bash
sudo apt update
sudo apt install redis-server
sudo systemctl start redis-server
sudo systemctl enable redis-server
```

#### CentOS/RHEL
```bash
sudo yum install epel-release
sudo yum install redis
sudo systemctl start redis
sudo systemctl enable redis
```

#### Windows
```bash
# Using Chocolatey
choco install redis-64

# Or download Windows version and install manually
# https://github.com/microsoftarchive/redis/releases
```

### 4. Configure Network (Optional)

HiveMind supports WireGuard VPN for secure node networking:

```bash
# Install WireGuard (Ubuntu)
sudo apt install wireguard

# Configuration will be auto-generated on first startup
```

## Deployment Architecture

### Standard Single-Machine Deployment

Suitable for testing and small-scale use:

```
┌─────────────────────────────────────┐
│           Single Machine            │
│  ┌─────────────┐ ┌─────────────┐    │
│  │ Node Pool   │ │   Master    │    │
│  │ :50051      │ │   Node      │    │
│  └─────────────┘ └─────────────┘    │
│  ┌─────────────┐ ┌─────────────┐    │
│  │  Worker 1   │ │  Worker 2   │    │
│  │   :dynamic  │ │   :dynamic  │    │
│  └─────────────┘ └─────────────┘    │
│  ┌─────────────┐                    │
│  │    Redis    │                    │
│  │   :6379     │                    │
│  └─────────────┘                    │
└─────────────────────────────────────┘
```

### Distributed Deployment

Suitable for production environments:

```
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│   Server A      │    │   Server B      │    │   Server C      │
│                │    │                │    │                │
│ ┌─────────────┐ │    │ ┌─────────────┐ │    │ ┌─────────────┐ │
│ │ Node Pool   │ │    │ │   Master    │ │    │ │  Worker 1   │ │
│ │   :50051    │ │    │ │   Node      │ │    │ │   :dynamic  │ │
│ └─────────────┘ │    │ └─────────────┘ │    │ └─────────────┘ │
│ ┌─────────────┐ │    │ ┌─────────────┐ │    │ ┌─────────────┐ │
│ │    Redis    │ │    │ │ Web UI      │ │    │ │  Worker 2   │ │
│ │   :6379     │ │    │ │   :5000     │ │    │ │   :dynamic  │ │
│ └─────────────┘ │    │ └─────────────┘ │    │ └─────────────┘ │
└─────────────────┘    └─────────────────┘    └─────────────────┘
```

## Detailed Deployment Steps

### Step 1: Node Pool Service Deployment

Node Pool is the core service responsible for node management and task scheduling.

```bash
# Enter node_pool directory
cd node_pool

# Check configuration file
cat config.py

# Start service
python node_pool_server.py
```

**Configuration Notes**:
- Default port: 50051
- Default Redis address: localhost:6379
- User database: users.db (SQLite)

### Step 2: Master Node Deployment

Master node provides management interface and advanced features.

```bash
# Enter master directory
cd master

# Start Master node
python master_node.py
```

**Configuration Notes**:
- Auto-connects to Node Pool
- Provides Web management interface
- Supports VPN configuration

### Step 3: Worker Node Deployment

Worker nodes execute actual computation tasks.

```bash
# Enter worker directory
cd worker

# Start Worker node
python worker_node.py
```

**Multi-Worker Deployment**:
```bash
# On different machines or with different configurations
python worker_node.py --node-id worker-1
python worker_node.py --node-id worker-2
```

### Step 4: Web Interface Deployment (Optional)

Provides user-friendly Web management interface.

```bash
# Enter web directory
cd web

# Start Web service
python app.py
```

Default access address: http://localhost:5000

## Advanced Configuration

### Environment Variable Configuration

Create `.env` file for custom configuration:

```bash
# Node Pool configuration
NODEPOOL_HOST=0.0.0.0
NODEPOOL_PORT=50051
REDIS_HOST=localhost
REDIS_PORT=6379

# Master configuration
MASTER_HOST=0.0.0.0
MASTER_PORT=50052

# Worker configuration
WORKER_HOST=0.0.0.0
WORKER_PORT=0  # Use dynamic port

# Web configuration
WEB_HOST=0.0.0.0
WEB_PORT=5000
WEB_DEBUG=false
```

### Redis Configuration Optimization

Edit `/etc/redis/redis.conf`:

```conf
# Memory limit
maxmemory 2gb
maxmemory-policy allkeys-lru

# Persistence configuration
save 900 1
save 300 10
save 60 10000

# Network configuration
bind 0.0.0.0
protected-mode no
```

### Firewall Configuration

```bash
# Ubuntu UFW
sudo ufw allow 50051  # Node Pool
sudo ufw allow 50052  # Master Node
sudo ufw allow 5000   # Web Interface
sudo ufw allow 6379   # Redis

# CentOS firewalld
sudo firewall-cmd --permanent --add-port=50051/tcp
sudo firewall-cmd --permanent --add-port=50052/tcp
sudo firewall-cmd --permanent --add-port=5000/tcp
sudo firewall-cmd --permanent --add-port=6379/tcp
sudo firewall-cmd --reload
```

## Service Management

### Using systemd (Recommended)

Create service files:

**Node Pool Service** (`/etc/systemd/system/hivemind-nodepool.service`):
```ini
[Unit]
Description=HiveMind Node Pool Service
After=network.target redis.service

[Service]
Type=simple
User=hivemind
WorkingDirectory=/opt/hivemind/node_pool
ExecStart=/usr/bin/python3 node_pool_server.py
Restart=always
RestartSec=10

[Install]
WantedBy=multi-user.target
```

**Master Service** (`/etc/systemd/system/hivemind-master.service`):
```ini
[Unit]
Description=HiveMind Master Node Service
After=network.target hivemind-nodepool.service

[Service]
Type=simple
User=hivemind
WorkingDirectory=/opt/hivemind/master
ExecStart=/usr/bin/python3 master_node.py
Restart=always
RestartSec=10

[Install]
WantedBy=multi-user.target
```

Start services:
```bash
sudo systemctl daemon-reload
sudo systemctl enable hivemind-nodepool
sudo systemctl enable hivemind-master
sudo systemctl start hivemind-nodepool
sudo systemctl start hivemind-master
```

### Using Docker Compose (Under Development)

```yaml
version: '3.8'
services:
  redis:
    image: redis:6.2-alpine
    ports:
      - "6379:6379"
    volumes:
      - redis_data:/data

  nodepool:
    build: ./node_pool
    ports:
      - "50051:50051"
    depends_on:
      - redis
    environment:
      - REDIS_HOST=redis

  master:
    build: ./master
    ports:
      - "50052:50052"
    depends_on:
      - nodepool

  worker:
    build: ./worker
    depends_on:
      - nodepool
    deploy:
      replicas: 2

volumes:
  redis_data:
```

## Monitoring and Logging

### Logging Configuration

```python
# Add logging configuration in each service
import logging

logging.basicConfig(
    level=logging.INFO,
    format='%(asctime)s - %(name)s - %(levelname)s - %(message)s',
    handlers=[
        logging.FileHandler('/var/log/hivemind/service.log'),
        logging.StreamHandler()
    ]
)
```

### Performance Monitoring

```bash
# Monitor Redis performance
redis-cli info stats

# Monitor system resources
htop
iotop
netstat -tulpn
```

## Security Considerations

### Network Security

1. **Use VPN**: Enable WireGuard VPN to protect inter-node communication
2. **Firewall Configuration**: Only open necessary ports
3. **TLS Encryption**: Enable gRPC TLS in production environments

### Authentication Configuration

```python
# Modify default keys in production
SECRET_KEY = "your-secret-key-here"
JWT_SECRET = "your-jwt-secret-here"
```

## Troubleshooting

### Common Issues

1. **Connection Problems**
   ```bash
   # Check service status
   netstat -tulpn | grep :50051
   
   # Check firewall
   sudo ufw status
   ```

2. **Redis Connection Failure**
   ```bash
   # Test Redis connection
   redis-cli ping
   
   # Check Redis logs
   sudo journalctl -u redis
   ```

3. **Worker Node Registration Failure**
   ```bash
   # Check network connectivity
   telnet nodepool-server 50051
   
   # Check logs
   tail -f /var/log/hivemind/worker.log
   ```

### Performance Tuning

1. **Redis Optimization**
   - Adjust `maxmemory` settings
   - Use appropriate data persistence strategy

2. **gRPC Optimization**
   - Adjust connection pool size
   - Enable compression

3. **Python Optimization**
   - Use PyPy for better performance
   - Configure appropriate worker process count

## Scaling Deployment

### Horizontal Scaling

1. **Add More Worker Nodes**
   ```bash
   # Repeat Worker deployment steps on new machines
   cd hivemind/worker
   python worker_node.py --node-id worker-new
   ```

2. **Load Balancing**
   - Use HAProxy or Nginx for load balancing
   - Configure Redis Cluster

### High Availability

1. **Redis Master-Slave Replication**
2. **Service Fault Tolerance**
3. **Automatic Failover**

---

**Updated**: January 2024  
**Version**: v1.0  
**Status**: Accurate deployment guide based on actual implementation

**Note**: This is a deployment guide based on current actual code. Docker containerization and some advanced features are still under development.
