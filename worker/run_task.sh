#!/bin/bash
# filepath: d:\hivemind\工作端\run_task.sh

set -e  # 遇到錯誤立即退出

echo "=== HiveMind Task Execution Script Started ==="
echo "Task ID: ${TASK_ID:-unknown}"
echo "User: $(whoami)"
echo "Working Directory: $(pwd)"
echo "Date: $(date)"

echo "=== Current Directory Contents ==="
ls -la

# 檢查是否有 requirements.txt
if [ -f "requirements.txt" ]; then
    echo "=== Installing Python Dependencies ==="
    echo "Found requirements.txt, installing dependencies..."
    cat requirements.txt
    
    # 安裝依賴，如果失敗則警告但繼續執行
    if pip install --user -r requirements.txt; then
        echo "Dependencies installed successfully"
    else
        echo "WARNING: Some dependencies failed to install, continuing anyway..."
        # 嘗試逐行安裝
        while IFS= read -r package; do
            if [[ ! -z "$package" && ! "$package" =~ ^# ]]; then
                echo "Trying to install: $package"
                pip install --user "$package" || echo "Failed to install $package, skipping..."
            fi
        done < requirements.txt
    fi
else
    echo "No requirements.txt found, skipping dependency installation"
fi

# 尋找 Python 腳本文件（按優先級）
SCRIPT_FILE=""
for script in "main.py" "task_script.py" "run.py" "app.py" "script.py"; do
    if [ -f "$script" ]; then
        SCRIPT_FILE="$script"
        echo "Found main script: $SCRIPT_FILE"
        break
    fi
done

if [ -z "$SCRIPT_FILE" ]; then
    # 如果沒找到常見名稱，找第一個 .py 文件
    SCRIPT_FILE=$(find . -name "*.py" -type f | head -n 1)
    if [ -n "$SCRIPT_FILE" ]; then
        echo "Using first Python file found: $SCRIPT_FILE"
    else
        echo "ERROR: No Python script found in task directory"
        echo "Available files:"
        ls -la
        exit 1
    fi
fi

echo "=== Executing Task Script: $SCRIPT_FILE ==="
echo "Environment variables:"
env | grep -E "^(TASK_|PYTHON)" || true

# 設置 Python 路徑和緩衝
export PYTHONUNBUFFERED=1
export PYTHONPATH="$(pwd):$PYTHONPATH"

# 執行 Python 腳本
echo "Starting script execution..."
if python "$SCRIPT_FILE"; then
    echo "=== Task Script Execution Completed Successfully ==="
    echo "Task execution finished at $(date)"
    exit 0
else
    exit_code=$?
    echo "ERROR: Task script execution failed with exit code $exit_code"
    echo "Task execution failed at $(date)"
    exit $exit_code
fi