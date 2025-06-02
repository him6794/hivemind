#!/bin/sh
set -eu

echo "Starting task execution..."

# 解壓任務檔案
unzip -q /tmp/task_archive.zip -d /app

# 安裝依賴（如果存在）
[ -f requirements.txt ] && pip install -r requirements.txt --no-cache-dir

# 執行主程式
python main.py