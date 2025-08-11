# HiveMind Worker Node Documentation

## Overview
HiveMind Worker is the working node component of the distributed computing platform, responsible for executing computational tasks assigned by the master node, monitoring system resource usage, and maintaining communication with the master node. All tasks run in isolated Docker containers to ensure security and environmental consistency.

## Key Features

### 1. Task Execution
- Run computational tasks in Docker containers using the `justin308/hivemind-worker` base image
- Support CPU, memory, and GPU resource monitoring and limitations
- Task lifecycle management: start, monitor, terminate, and result transmission
- Automatic handling of task dependencies and environment configuration

### 2. Resource Monitoring
- Real-time collection of CPU usage, memory consumption, and GPU utilization
- Report resource usage data to the master node every 30 seconds
- Support resource monitoring and allocation in multi-GPU environments
- Dynamically adjust task priorities based on resource utilization

### 3. Node Communication
#### gRPC Interface Definition
```protobuf
// Node status reporting interface
service NodeService {
  rpc ReportNodeStatus (NodeStatusRequest) returns (NodeStatusResponse);
  rpc RegisterNode (NodeRegistrationRequest) returns (NodeRegistrationResponse);
  rpc Heartbeat (HeartbeatRequest) returns (HeartbeatResponse);
}

// Task management interface
service TaskService {
  rpc AssignTask (TaskAssignmentRequest) returns (TaskAssignmentResponse);
  rpc SubmitTaskResult (TaskResultRequest) returns (TaskResultResponse);
  rpc CancelTask (TaskCancelRequest) returns (TaskCancelResponse);
}
```

#### VPN Configuration Auto-generation Process
1. Check if wg0.conf file exists when node starts
2. If not exists, call `vpn_service.generate_config()` to create new configuration
3. Securely obtain master node public key via HTTPS
4. Generate local private key and IP configuration
5. Automatically start WireGuard service and verify connection
6. Automatically restart VPN connection when configuration changes

- Communicate with master node using gRPC protocol
- Implement automatic reconnection mechanism to handle network interruptions
- Use Protobuf to define data structures ensuring communication efficiency and compatibility
- Support real-time task status updates and log transmission

### 4. Security Features
- Automatically generate and manage WireGuard VPN configurations to ensure secure communication between nodes
- Containerized isolation to prevent mutual interference between tasks
- Resource limitation and quota management
- Node authentication and authorization

## Installation and Configuration

### Docker Image Construction
```bash
# Build worker image
python3 build.py --docker

docker build -t justin308/hivemind-worker:latest .

# Push image to repository
docker push justin308/hivemind-worker:latest
```

### System Requirements
- Windows or Linux operating system
- Python 3.8+
- Docker Engine 20.10+
- At least 2GB RAM
- Virtualization technology support (for Docker)
- Network connection (for downloading Docker images and communicating with master node)

### Dependency Installation
```bash
# Install Python dependencies
pip install -r requirements.txt

# Ensure Docker service is running
systemctl start docker  # Linux
# Windows: Start Docker Desktop or execute in PowerShell
# Start-Process 'C:\Program Files\Docker\Docker\Docker Desktop.exe'
# Or start Docker Desktop on Windows
```

### Configuration Options
Worker node configuration is primarily done through environment variables and configuration files:

1. Environment variable configuration:
```bash
# Master node address
MASTER_NODE_URL=https://hivemind.justin0711.com

# VPN configuration
WIREGUARD_CONFIG_PATH=./wg0.conf

# Resource report interval (seconds)
RESOURCE_REPORT_INTERVAL=30

# Log level
LOG_LEVEL=INFO
```

2. Configuration file:
The main configuration file is `wg0.conf`, containing detailed WireGuard VPN configuration:
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

### Starting Worker Node
```bash
# Run Python script directly
python3 worker_node.py

# Or use packaged executable
./HiveMind-Worker.exe  # Windows
# Or
./HiveMind-Worker     # Linux
```

### Command Line Parameters
```bash
# Specify configuration file
python3 worker_node.py --config ./custom_config.conf

# Enable debug mode
python3 worker_node.py --debug

# Specify log file
python3 worker_node.py --log-file ./worker.log

# Override master node address
python3 worker_node.py --master-url https://custom-master-url.com
```

### Monitoring Interface
The worker node provides a simple web monitoring interface, running on port 5001 by default:
```bash
# Access monitoring interface
browser http://localhost:5001/monitor.html
```
The monitoring interface displays:
- Current node status
- List of running tasks
- Resource usage statistics charts
- Task history

## Technical Implementation Details

### Complete Task Execution Lifecycle
1. **Task Reception**: Receive task definitions and resource requirements from master node via gRPC
2. **Environment Preparation**:
   - Verify local Docker environment
   - Pull required image versions
   - Create isolated networks and storage volumes
3. **Task Scheduling**:
   - Allocate resources based on node trust level
   - Apply CPU/memory/GPU limitations
   - Set task timeout period
4. **Execution Monitoring**:
   - Capture container output in real-time
   - Check running status every 5 seconds
   - Trigger alerts when resource usage exceeds thresholds
5. **Result Processing**:
   - Collect output files after task completion
   - Generate execution reports and resource usage statistics
   - Stream results via gRPC
6. **Cleanup Work**:
   - Delete temporary containers and networks
   - Preserve debugging data for failed tasks
   - Update local task history database
**Note**: The following is a simplified流程, for detailed lifecycle please refer to the "Complete Task Execution Lifecycle" section above

### Resource Monitoring Implementation
Resource monitoring is implemented through:
- CPU usage: Collected using psutil library
- Memory usage: Obtained through system API
- GPU monitoring: Using nvidia-smi (NVIDIA System Management Interface)
- Resource data is sampled every 30 seconds and sent to master node via gRPC

### Reward Calculation
Worker nodes receive rewards based on resource contributions:
```python
# Simplified reward calculation formula
base_reward = 10  # Base reward
usage_multiplier = 1.0

# Adjust multiplier based on average usage
avg_usage = (cpu_usage + memory_usage) / 2
if avg_usage > 80:
    usage_multiplier = 1.5
elif avg_usage > 50:
    usage_multiplier = 1.2
elif avg_usage > 20:
    usage_multiplier = 1.0
else:
    usage_multiplier = 0.8

# Additional GPU bonus
gpu_bonus = gpu_usage * 0.01

total_reward = int(base_reward * usage_multiplier + gpu_bonus)
```

## Troubleshooting

### Common Issues
1. **Docker Connection Issues**
   - Ensure Docker service is running
   - Check user permissions for Docker socket access
   - Verify network connection status

2. **VPN Configuration Errors**
   - Check if wg0.conf file is correct
   - Verify Endpoint address and port accessibility
   - Ensure firewall allows UDP 51820 port communication

3. **Resource Report Failures**
   - Check network connection with master node
   - Verify gRPC service is running properly
   - Check log files for detailed error information

4. **Task Execution Failures**
   - Check if Docker image is complete
   - Verify task resource requirements do not exceed node capabilities
   - Check task logs for specific error causes

### Project Structure
```
worker/
├── Dockerfile           # Docker image build file
├── README.md            # This document
├── build.py             # Executable build script
├── hivemind_worker/
│   ├── __init__.py
│   ├── main.py          # Entry point
│   ├── src/
│   │   └── hivemind_worker/
│   │       ├── communication/
│   │       │   ├── grpc_client.py
│   │       │   └── vpn_configurator.py
│   │       ├── monitoring/
│   │       │   ├── resource_collector.py
│   │       │   └── stats_aggregator.py
│   │       └── task_management/
│   │       ├── communication/
│   │       │   ├── grpc_client.py
│   │       │   └── vpn_configurator.py
│   │       ├── monitoring/
│   │       │   ├── resource_collector.py
│   │       │   └── stats_aggregator.py
│   │       └── task_management/
│   │           ├── docker_handler.py
│   │           └── task_executor.py
│   ├── pyproject.toml
│   ├── setup_logic.ps1
│   └── setup_worker.iss
├── install.sh           # Installation script
├── make.py              # Build script
├── requirements.txt     # Python dependencies
├── run_task.sh          # Task execution script
├── setup.py             # Package installation configuration
├── static/              # Web monitoring interface static files
├── templates/           # Web interface templates
├── wg0.conf             # WireGuard configuration
└── worker_node.py       # Main program
```

## License
This project is licensed under the GNU General Public License v3.0 - see the LICENSE file for details.

## Contact Information
- Project website: https://hivemind.justin0711.com
- Support email: hivemind@justin0711.com