FROM python:3.12-slim

ENV PYTHONUNBUFFERED=1

# 安裝必要工具
RUN apt-get update && \
    apt-get install -y --no-install-recommends \
    tini \
    unzip \
    && rm -rf /var/lib/apt/lists/*

# 建立非 root 使用者和工作目錄
RUN useradd --no-log-init -r -m appuser && \
    mkdir -p /app && \
    chown appuser:appuser /app

WORKDIR /app
USER appuser

# 複製執行腳本
COPY --chown=appuser:appuser run_task.sh /usr/local/bin/
RUN chmod +x /usr/local/bin/run_task.sh

ENTRYPOINT ["/usr/bin/tini", "--"]
CMD ["sleep", "infinity"]