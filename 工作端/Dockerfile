FROM python:3.11-slim

# 安裝系統依賴
RUN apt-get update && apt-get install -y bash git && rm -rf /var/lib/apt/lists/*

# 創建用戶
RUN useradd -m -u 1000 appuser

# 設置工作目錄
WORKDIR /app

# 複製執行腳本
COPY run_task.sh /app/run_task.sh
RUN chmod +x /app/run_task.sh

# 預安裝常用包
RUN pip install --no-cache-dir \
    requests numpy pandas matplotlib \
    Pillow scikit-learn

# 切換用戶
USER appuser

CMD ["bash", "/app/run_task.sh"]