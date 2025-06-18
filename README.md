# HiveMind Distributed Computing Platform

[中文說明（繁體）](./README.zh-TW.md)

This platform is a centralized distributed computing system utilizing efficient gRPC communication, combined with Docker, Redis, and Flask technologies to achieve secure and high-performance task distribution and execution.

## Architecture

- **Master Node**  
  - Upload tasks
  - Query task execution results
  - Provides web interface and API (Flask)

- **Node Pool**  
  - Manages worker nodes
  - Assigns tasks to worker nodes
  - Collects and stores execution results (Redis)

- **Worker Node**  
  - Receives tasks and executes them in Docker containers
  - Returns execution results to the node pool
  - Sends heartbeat signals regularly

## System Workflow

1. **Worker Node Startup**  
   Logs in and registers with the node pool, reporting CPU, RAM, GPU, and other information.
2. **Node Pool Registration**  
   Stores worker node information in Redis upon registration.
3. **Task Upload by Master Node**  
   Node pool assigns tasks to suitable worker nodes.
4. **Task Execution**  
   Worker node executes the task in a Docker container and returns the result to the node pool.
5. **Result Query**  
   Master node queries task results or logs, and the node pool returns the corresponding data.
6. **Heartbeat Monitoring**  
   Worker node sends a heartbeat every second. If no heartbeat is received for 10 seconds, the node pool removes the worker node and reassigns the task.
7. **Resource Fee Settlement**  
   During task execution, the master node account is charged CPT tokens based on resource usage, which are transferred to the worker node.

## Token System (CPT)

- **Purpose**: Used as the unit for resource fee calculation
- **Distribution**: Each account receives 150 CPT upon registration; total supply is 1,000,000,000,000 CPT
- **Consumption**: Master node must pay CPT to upload tasks, with fees calculated based on resource usage
- **Earning**: Worker nodes earn CPT by completing tasks
- **Tipping**: Users can tip open-source projects with CPT

## Technology Stack

- **gRPC**: Efficient communication
- **Redis**: Data caching
- **Docker**: Containerized task execution
- **Python**: Backend logic
- **Flask**: Web interface and API
- **VPN**: Secure communication and virtual intranet

## Installation & Startup

1. Install dependencies:
   ```powershell
   pip install -r requirements.txt
   ```
2. Install Docker and Redis services
3. Start services:
   - Master Node Web Interface: `http://127.0.0.1:5001`
   - Worker Node Web Interface: `http://127.0.0.1:5000`
