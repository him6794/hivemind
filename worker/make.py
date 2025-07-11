import base64
from pathlib import Path

FILES = {
    "worker_node.py": "worker_node.py",
    "requirements.txt": "requirements.txt",
    "templates/login.html": "templates/login.html",
    "templates/monitor.html": "templates/monitor.html",
    'nodepool.proto': 'nodepool.proto',
    'static/js/monitor.js': 'static/js/monitor.js',
}

install_lines = [
    "#!/bin/bash",
    "set -e",
    "echo '📁 建立資料夾 hivemind_worker/templates...'",
    "mkdir -p hivemind_worker/templates",
    "cd hivemind_worker",
]

for out_path, file_path in FILES.items():
    file_data = Path(file_path).read_bytes()
    b64_data = base64.b64encode(file_data).decode()
    target_path = f"./{out_path}"
    install_lines += [
        f"echo '🔧 建立 {target_path} ...'",
        f"base64 -d > \"{target_path}\" << 'EOF_{out_path}'",
        b64_data,
        f"EOF_{out_path}"
    ]

install_lines += [
    "echo '🐍 建立虛擬環境...'",
    "python3 -m venv venv",
    "source venv/bin/activate",
    "echo '📦 安裝依賴...'",
    "pip install --upgrade pip",
    "pip install -r requirements.txt",
    "python3 -m grpc_tools.protoc --proto_path=. --python_out=. --grpc_python_out=. nodepool.proto",
    "echo '🚀 啟動 worker_node.py...'",
    "python3 worker_node.py"
]

Path("install.sh").write_text("\n".join(install_lines), encoding="utf-8")

print("✅ install.sh 已產生")
