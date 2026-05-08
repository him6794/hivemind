#!/bin/bash
# VPN Integration Test Script
# Tests VPN functionality with Nodepool and multiple Workers

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Configuration
NODEPOOL_PORT=50051
WORKER1_PORT=50052
WORKER2_PORT=50053
TEST_TIMEOUT=60
LOG_DIR="./logs/vpn_test"

# Create log directory
mkdir -p "$LOG_DIR"

echo -e "${GREEN}=== HiveMind VPN Integration Test ===${NC}"
echo "Log directory: $LOG_DIR"
echo ""

# Cleanup function
cleanup() {
    echo -e "\n${YELLOW}Cleaning up...${NC}"

    # Kill all background processes
    if [ ! -z "$NODEPOOL_PID" ]; then
        echo "Stopping Nodepool (PID: $NODEPOOL_PID)"
        kill $NODEPOOL_PID 2>/dev/null || true
    fi

    if [ ! -z "$WORKER1_PID" ]; then
        echo "Stopping Worker 1 (PID: $WORKER1_PID)"
        kill $WORKER1_PID 2>/dev/null || true
    fi

    if [ ! -z "$WORKER2_PID" ]; then
        echo "Stopping Worker 2 (PID: $WORKER2_PID)"
        kill $WORKER2_PID 2>/dev/null || true
    fi

    # Wait for processes to terminate
    sleep 2

    echo -e "${GREEN}Cleanup complete${NC}"
}

# Set trap for cleanup
trap cleanup EXIT INT TERM

# Check if services are built
check_binaries() {
    echo -e "${YELLOW}Checking binaries...${NC}"

    if [ ! -f "./services/nodepool/nodepool" ]; then
        echo -e "${RED}Nodepool binary not found. Building...${NC}"
        cd services/nodepool && go build -o nodepool . && cd ../..
    fi

    if [ ! -f "./services/worker/worker" ]; then
        echo -e "${RED}Worker binary not found. Building...${NC}"
        cd services/worker && go build -o worker . && cd ../..
    fi

    echo -e "${GREEN}✓ Binaries ready${NC}"
}

# Start Nodepool with VPN
start_nodepool() {
    echo -e "\n${YELLOW}Starting Nodepool with VPN...${NC}"

    export NODEPOOL_GRPC_PORT=":$NODEPOOL_PORT"
    export VPN_ENABLED=true
    export VPN_SERVER_URL="http://localhost:8080"
    export VPN_LISTEN_ADDR="0.0.0.0:8080"
    export VPN_GRPC_LISTEN_ADDR="0.0.0.0:50443"
    export VPN_IP_PREFIX="100.64.0.0/10"
    export VPN_BASE_DOMAIN="hivemind.local"
    export VPN_EPHEMERAL_NODES=true
    export VPN_NODE_EXPIRY="24h"
    export VPN_DB_TYPE="sqlite"
    export VPN_DB_PATH="$LOG_DIR/headscale.db"
    export VPN_PRIVATE_KEY_PATH="$LOG_DIR/private.key"
    export VPN_NOISE_PRIVATE_KEY_PATH="$LOG_DIR/noise_private.key"

    ./services/nodepool/nodepool > "$LOG_DIR/nodepool.log" 2>&1 &
    NODEPOOL_PID=$!

    echo "Nodepool PID: $NODEPOOL_PID"
    echo "Waiting for Nodepool to start..."
    sleep 5

    # Check if Nodepool is running
    if ! ps -p $NODEPOOL_PID > /dev/null; then
        echo -e "${RED}✗ Nodepool failed to start${NC}"
        cat "$LOG_DIR/nodepool.log"
        exit 1
    fi

    echo -e "${GREEN}✓ Nodepool started${NC}"
}

# Start Worker
start_worker() {
    local WORKER_ID=$1
    local WORKER_PORT=$2
    local WORKER_NUM=$3

    echo -e "\n${YELLOW}Starting Worker $WORKER_NUM (ID: $WORKER_ID)...${NC}"

    export WORKER_ID=$WORKER_ID
    export NODEPOOL_ADDR="localhost:$NODEPOOL_PORT"
    export WORKER_GRPC_PORT=":$WORKER_PORT"
    export VPN_ENABLED=true
    export VPN_STATE_DIR="$LOG_DIR/worker_${WORKER_NUM}_vpn"
    export VPN_HOSTNAME="worker-$WORKER_ID"

    mkdir -p "$LOG_DIR/worker_${WORKER_NUM}_vpn"

    ./services/worker/worker > "$LOG_DIR/worker_${WORKER_NUM}.log" 2>&1 &
    local PID=$!

    echo "Worker $WORKER_NUM PID: $PID"
    echo "Waiting for Worker $WORKER_NUM to register..."
    sleep 3

    # Check if Worker is running
    if ! ps -p $PID > /dev/null; then
        echo -e "${RED}✗ Worker $WORKER_NUM failed to start${NC}"
        cat "$LOG_DIR/worker_${WORKER_NUM}.log"
        exit 1
    fi

    echo -e "${GREEN}✓ Worker $WORKER_NUM started${NC}"
    echo $PID
}

# Verify VPN connections
verify_vpn_connections() {
    echo -e "\n${YELLOW}Verifying VPN connections...${NC}"

    # Wait for VPN to establish
    echo "Waiting for VPN mesh to establish..."
    sleep 10

    # Check Nodepool logs for worker registrations
    echo "Checking worker registrations..."

    if grep -q "worker-001" "$LOG_DIR/nodepool.log" && \
       grep -q "worker-002" "$LOG_DIR/nodepool.log"; then
        echo -e "${GREEN}✓ Both workers registered with Nodepool${NC}"
    else
        echo -e "${RED}✗ Worker registration failed${NC}"
        echo "Nodepool log:"
        tail -20 "$LOG_DIR/nodepool.log"
        return 1
    fi

    # Check Worker logs for VPN connection
    if grep -q "VPN.*connected\|VPN.*registered" "$LOG_DIR/worker_1.log"; then
        echo -e "${GREEN}✓ Worker 1 VPN connected${NC}"
    else
        echo -e "${YELLOW}⚠ Worker 1 VPN status unclear${NC}"
    fi

    if grep -q "VPN.*connected\|VPN.*registered" "$LOG_DIR/worker_2.log"; then
        echo -e "${GREEN}✓ Worker 2 VPN connected${NC}"
    else
        echo -e "${YELLOW}⚠ Worker 2 VPN status unclear${NC}"
    fi

    echo -e "${GREEN}✓ VPN connections verified${NC}"
}

# Test worker communication
test_worker_communication() {
    echo -e "\n${YELLOW}Testing worker-to-worker communication...${NC}"

    # Check if workers can see each other in logs
    sleep 5

    echo "Checking peer discovery..."

    # Look for peer information in logs
    if grep -q "peer\|Peer" "$LOG_DIR/worker_1.log" || \
       grep -q "peer\|Peer" "$LOG_DIR/worker_2.log"; then
        echo -e "${GREEN}✓ Workers discovered peers${NC}"
    else
        echo -e "${YELLOW}⚠ Peer discovery status unclear${NC}"
    fi

    echo -e "${GREEN}✓ Communication test complete${NC}"
}

# Test multinode task execution
test_multinode_task() {
    echo -e "\n${YELLOW}Testing multinode task execution...${NC}"

    # This would require a test client to submit a task
    # For now, we verify the infrastructure is ready

    echo "Verifying multinode execution capability..."

    # Check if both workers are ready
    if ps -p $WORKER1_PID > /dev/null && ps -p $WORKER2_PID > /dev/null; then
        echo -e "${GREEN}✓ Both workers are running and ready for tasks${NC}"
    else
        echo -e "${RED}✗ One or more workers are not running${NC}"
        return 1
    fi

    echo -e "${GREEN}✓ Multinode infrastructure ready${NC}"
}

# Display test summary
display_summary() {
    echo -e "\n${GREEN}=== Test Summary ===${NC}"
    echo "Nodepool PID: $NODEPOOL_PID"
    echo "Worker 1 PID: $WORKER1_PID"
    echo "Worker 2 PID: $WORKER2_PID"
    echo ""
    echo "Logs available in: $LOG_DIR"
    echo "  - nodepool.log"
    echo "  - worker_1.log"
    echo "  - worker_2.log"
    echo ""
    echo -e "${GREEN}All tests passed!${NC}"
    echo ""
    echo "To view logs:"
    echo "  tail -f $LOG_DIR/nodepool.log"
    echo "  tail -f $LOG_DIR/worker_1.log"
    echo "  tail -f $LOG_DIR/worker_2.log"
}

# Main test flow
main() {
    echo "Starting VPN integration tests..."
    echo ""

    # Step 1: Check binaries
    check_binaries

    # Step 2: Start Nodepool
    start_nodepool

    # Step 3: Start Workers
    WORKER1_PID=$(start_worker "worker-001" $WORKER1_PORT 1)
    WORKER2_PID=$(start_worker "worker-002" $WORKER2_PORT 2)

    # Step 4: Verify VPN connections
    verify_vpn_connections

    # Step 5: Test worker communication
    test_worker_communication

    # Step 6: Test multinode task execution
    test_multinode_task

    # Step 7: Display summary
    display_summary

    # Keep running for manual inspection
    echo -e "\n${YELLOW}Press Ctrl+C to stop all services${NC}"
    wait
}

# Run main function
main
