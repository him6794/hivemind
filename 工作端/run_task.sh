#!/bin/bash
# filepath: d:\hivemind\工作端\run_task.sh

set -e  # 遇到錯誤立即退出

echo "=== HiveMind Task Execution Script Started ==="
echo "Task ID: ${TASK_ID:-unknown}"
echo "User: $(whoami)"
echo "Working Directory: $(pwd)"
echo "User ID: $(id)"
echo "Date: $(date)"

# 檢查任務 ZIP 文件是否存在
if [ ! -f "/tmp/task_archive.zip" ]; then
    echo "ERROR: Task archive not found at /tmp/task_archive.zip"
    exit 1
fi

# 創建臨時工作目錄，確保用戶有權限
WORK_DIR="/tmp/task_work_${TASK_ID}"
mkdir -p "$WORK_DIR"
cd "$WORK_DIR"

echo "=== Extracting Task Archive to $WORK_DIR ==="
unzip -q /tmp/task_archive.zip || {
    echo "ERROR: Failed to extract task archive"
    exit 1
}

echo "=== Task Files ==="
ls -la

# 檢查是否有 requirements.txt
if [ -f "requirements.txt" ]; then
    echo "=== Installing Python Dependencies ==="
    pip install --user -r requirements.txt || {
        echo "WARNING: Some dependencies failed to install, continuing..."
    }
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
        echo "ERROR: No Python script found in task archive"
        exit 1
    fi
fi

echo "=== Executing Task Script: $SCRIPT_FILE ==="
echo "Environment variables:"
env | grep -E "^(TASK_|PYTHON)" || true

# 設置 Python 路徑和緩衝
export PYTHONUNBUFFERED=1
export PYTHONPATH="$WORK_DIR:$PYTHONPATH"

python "$SCRIPT_FILE" || {
    echo "ERROR: Task script execution failed with exit code $?"
    exit 1
}

echo "=== Creating Results Archive ==="
# 創建結果 ZIP 文件
if [ -d "output" ]; then
    echo "Found output directory, archiving it..."
    cd output
    zip -r ../results.zip . || {
        echo "WARNING: Failed to create results archive from output directory"
        cd ..
        echo "Task completed but no output generated" > result.txt
        zip results.zip result.txt
    }
    cd ..
elif ls *.out >/dev/null 2>&1 || ls result* >/dev/null 2>&1 || ls output* >/dev/null 2>&1; then
    echo "Found result files, archiving them..."
    zip results.zip *.out result* output* 2>/dev/null || {
        echo "Task completed successfully at $(date)" > result.txt
        zip results.zip result.txt
    }
else
    echo "No specific output found, creating completion marker..."
    echo "Task completed successfully at $(date)" > result.txt
    echo "Task ID: ${TASK_ID}" >> result.txt
    echo "Execution completed in directory: $WORK_DIR" >> result.txt
    zip results.zip result.txt
fi

# 將結果移動到期望的位置
if [ -f "results.zip" ]; then
    # 在臨時目錄中創建結果，然後移動到 /app（如果可能）
    if [ -w "/app" ]; then
        cp results.zip /app/results.zip
        echo "Results copied to /app/results.zip"
    else
        # 如果無法寫入 /app，嘗試創建符號鏈接或在當前目錄保留
        ln -sf "$WORK_DIR/results.zip" /tmp/results.zip 2>/dev/null || {
            echo "WARNING: Cannot create symlink, results remain at $WORK_DIR/results.zip"
        }
    fi
    echo "=== Task Execution Completed Successfully ==="
    echo "Results archive: results.zip ($(du -h results.zip | cut -f1))"
else
    echo "ERROR: Failed to create results.zip"
    exit 1
fi