#!/bin/bash
set -e
sudo apt update
sudo apt install -y python3 python3-pip python3-venv git curl
sudo apt install -y wireguard wireguard-tools
cd hivemind_master
python3 -m venv venv
source venv/bin/activate
pip install --upgrade pip
pip install --upgrade hivemind_master
sudo apt install wireguard
echo 'start master_node.py...'
python3 -c "from hivemind_master import master_node; master_node.run_master_node()"