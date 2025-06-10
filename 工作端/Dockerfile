FROM python:3.9-slim

# 安裝系統依賴
RUN apt-get update && apt-get install -y \
    curl \
    unzip \
    zip \
    && rm -rf /var/lib/apt/lists/*

# 創建非 root 用戶
RUN useradd -m -s /bin/bash appuser

# 設置工作目錄並設置正確的權限
WORKDIR /app
RUN chown -R appuser:appuser /app

# 複製任務執行腳本並設置權限
COPY run_task.sh /usr/local/bin/run_task.sh
RUN chmod +x /usr/local/bin/run_task.sh

# 安裝常用 Python 包
RUN pip install --no-cache-dir \
    numpy \
    pandas \
    requests \
    pillow \
    matplotlib

# 切換到非 root 用戶
USER appuser

# 默認命令
CMD ["/usr/local/bin/run_task.sh"]