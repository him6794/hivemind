# HiveMind 分布式運算平台
本平台是一個中心化的分布式運算系統，採用高效的 gRPC 通訊，並結合 Docker、Redis、Flask 等技術，實現高效、安全的任務分發與執行。

## 架構組成

- **主控端**  
  - 上傳任務
  - 查詢任務執行結果
  - 提供 Web 介面與 API（Flask）

- **節點池**  
  - 管理工作端
  - 分配任務給工作端
  - 收集與儲存執行結果（Redis）

- **工作端**  
  - 接收任務並於 Docker 容器中執行
  - 回傳執行結果給節點池
  - 定時回報心跳

## 系統流程

1. **工作端啟動**  
   登入帳戶並向節點池註冊，回報 CPU、RAM、GPU 等資訊。
2. **節點池註冊**  
   收到註冊資訊後，將工作端資訊暫存於 Redis。
3. **主控端上傳任務**  
   節點池分配任務給合適的工作端。
4. **任務執行**  
   工作端於 Docker 容器執行任務，並回傳結果至節點池。
5. **結果查詢**  
   主控端查詢任務結果或日誌，節點池回傳對應資料。
6. **心跳監控**  
   工作端每秒回報心跳，若 10 秒未收到心跳，節點池將其移除並重新分配任務。
7. **資源費用結算**  
   任務執行期間，主控端帳戶會依據資源消耗扣除 CPT 代幣，並轉帳給工作端。

## 代幣體系（CPT）

- **用途**：作為資源費用計算單位
- **分配**：每個帳戶註冊時獲得 150 CPT，總量 1,000,000,000,000 CPT
- **消耗**：主控端上傳任務需支付 CPT，費用依資源消耗計算
- **獲取**：工作端完成任務可獲得 CPT
- **打賞**：用戶可打賞開源項目

## 技術棧

- **gRPC**：高效通訊
- **Redis**：資料暫存
- **Docker**：任務容器化執行
- **Python**：後端邏輯
- **Flask**：Web 介面與 API
- **VPN**：通訊安全與虛擬內網

## 安裝與啟動

1. 安裝依賴：
   ```powershell
   pip install -r requirements.txt
   ```
2. 安裝 Docker 與 Redis 服務
3. 啟動服務：
   - 主控端 Web 介面：`http://127.0.0.1:5001`
   - 工作端 Web 介面：`http://127.0.0.1:5000`

# HiveMind Distributed Computing Platform (English)
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
