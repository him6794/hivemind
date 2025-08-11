#!/bin/bash
set -e
sudo apt update
sudo apt install -y python3 python3-pip python3-venv git curl
sudo apt install -y wireguard wireguard-tools
cd hivemind_worker
echo '🐍 建立虛擬環境...'
python3 -m venv venv
source venv/bin/activate
echo '📦 安裝依賴...'
pip install --upgrade pip
pip install hivemind_worker
sudo apt install wireguard
echo '🐳 檢查是否已安裝 Docker...'
if ! command -v docker &> /dev/null; then
    echo '📦 Docker 未安裝，正在安裝...'
    curl -fsSL https://get.docker.com -o get-docker.sh
    sudo sh get-docker.sh
    sudo systemctl start docker
    sudo systemctl enable docker
else
    echo '✅ Docker 已安裝，略過安裝步驟。'
fi
echo '🚀 啟動 worker_node.py...'
python3 -c "from hivemind_worker import worker_node; worker_node.run_worker_node()"