# HiveMind Worker Node Documentation

## Overview
HiveMind Worker is the worker node component of the distributed computing platform. It is responsible for executing computing tasks assigned by the master node, monitoring system resource usage, and maintaining communication with the master node. All tasks are executed in isolated Docker containers to ensure security and environmental consistency.

## Main Features

### 1. Task Execution
- Runs computing tasks through Docker containers, using the `justin308/hivemind-worker` base image
- Supports CPU, memory, and GPU resource monitoring and limits
- Task lifecycle management: launching, monitoring, terminating, and transmitting results
- Automatically handles task dependencies and environment configuration

### 2. Resource Monitoring
- Real-time collection of CPU usage, memory usage, and GPU usage
- Reports resource usage data to the master node every 30 seconds
- Supports resource monitoring and allocation in multi-GPU environments
- Dynamically adjusts task priorities based on resource usage

### 3. Node Communication
- Uses the gRPC protocol to communicate with the master node
- Implements an automatic reconnection mechanism to handle network outages
- Defines data structures using Protobuf to ensure communication efficiency and compatibility
- Supports real-time task status updates and log transmission

### 4. Security Features
- Automatically generates and manages WireGuard VPN configurations to ensure secure communication between nodes
- Containerized isolation prevents tasks from interfering with each other.
- Resource limits and quota management
- Node authentication and authorization

## Installation and Configuration

### System Requirements
- Windows or Linux operating system
- Python 3.8+
- Docker Engine 20.10+
- At least 2GB of RAM
- Support for virtualization technology (for Docker)
- Network connection (for downloading Docker images and communicating with the master node)

### Dependency Installation
```bash
# Install Python dependencies
pip install -r requirements.txt

# Ensure the Docker service is running
systemctl start docker # Linux
# Or start Docker Desktop on Windows
```

### Configuration Options
Worker node configuration is primarily performed through environment variables and configuration files:

1. Environment variable configuration:
```bash
# Master node address
MASTER_NODE_URL=https://hivemind.justin0711.com

# VPN Configuration
WIREGUARD_CONFIG_PATH=./wg0.conf

# Resource report interval (seconds)
RESOURCE_REPORT_INTERVAL=30

# Log level
LOG_LEVEL=INFO
```

2. Configuration File:
The main configuration file is `wg0.conf`, which contains the detailed WireGuard VPN configuration:
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

## Usage

### Starting a Worker Node
```bash
# Run the Python script directly:
python worker_node.py

# Or use the packaged executable:
./HiveMind-Worker.exe # Windows
# Or
./HiveMind-Worker # Linux
```

### Command Line Parameters
```bash
# Specify the configuration file:
python worker_node.py --config ./custom_config.conf

# Enable debug mode:
python worker_node.py --debug

# Specify the log file:
python worker_node.py --log-file ./worker.log

# Override the master node address:
python worker_node.py --master-url https://custom-master-url.com
```

### Monitoring Interface
The worker node provides a simple web monitoring interface, running on port 5001 by default:
```bash
# Access the monitoring interface:
browser: http://localhost:5001/monitor.html
```
The monitoring interface displays:
- Current node status
- List of running tasks
- Resource Usage Statistics Chart
- Task History

## Technical Implementation Details

### Task Execution Process
1. Receive tasks assigned by the master node
2. Pull the necessary Docker images (e.g., justin308/hivemind-worker)
3. Create a container and configure resource limits
4. Mount the task data volume
5. Start the container and monitor the execution progress
6. Collect task output and logs
7. Compress the results and return them to the master node
8. Clean up the container and temporary files

### Resource Monitoring Implementation
Resource monitoring is implemented using the following methods:
- CPU usage: Collected using the psutil library
- Memory usage: Obtained through system APIs
- GPU monitoring: Using nvidia-smi (NVIDIA System Management Interface)
- Resource data is sampled every 30 seconds and sent to the master node via gRPC

### Reward Calculation
Worker nodes receive rewards based on their resource contribution:
```python
# Simplified reward calculation formula
base_reward = 10 # Base Reward
usage_multiplier = 1.0

# Adjust the multiplier based on average usage
avg_usage = (cpu_usage + memory_usage) / 2
if avg_usage > 80:
usage_multiplier = 1.5
elif avg_usage > 50:
usage_multiplier = 1.2
elif avg_usage > 20:
usage_multiplier = 1.0
else:
usage_multiplier = 0.8

# GPU Bonus
gpu_bonus = gpu_usage * 0.01

total_reward = int(base_reward * usage_multiplier + gpu_bonus)
```

## Troubleshooting

### Common Issues
1. **Docker Connection Issue**
- Ensure the Docker service is running
- Check that the user has access to the Docker socket
- Verify network connectivity

2. **VPN Configuration Error**
- Check the wg0.conf file for correctness
- Verify that the endpoint address and port are reachable
- Ensure the firewall allows communication on UDP port 51820

3. **Resource Report Failure**
- Check network connectivity to the master node
- Verify that the gRPC service is running properly
- Check the log file for detailed error information

4. **Task Execution Failure**
- Check the Docker image for integrity
- Verify that the task resource requirements exceed the node capacity
- Check the task log for the specific error cause

### Project Structure
```
worker/
├── Dockerfile # Docker image build file
├── README.md # This document
├── build.py # Executable build script
├── hivemind_worker/ # Python package source code
│ ├── __init__.py
│ ├── main.py # Entry point
│ └── src/ # Source Code Directory
├── install.sh # Installation Script
├── make.py # Build Script
├── requirements.txt # Python Dependencies
├── run_task.sh # Task Execution Script
├── setup.py # Package Installation and Configuration
├── static/ # Web Monitoring Interface Static Files
├── templates/ # Web Interface Templates
├── wg0.conf # WireGuard Configuration
└── worker_node.py # Main Program
```

## License
This project is licensed under the GNU General Public License v3.0 - see the LICENSE file for details.

## Contact Information
- Project Website: https://hivemind.justin0711.com
- Support Email: hivemind@justin0711.com