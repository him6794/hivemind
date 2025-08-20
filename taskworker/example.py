import asyncio
from taskworker import TaskWorker

# 創建 TaskWorker 實例
worker = TaskWorker("worker_001")

# 使用裝飾器註冊函數
@worker.function("a")
def function_a():
    return "123"

@worker.function("b") 
def function_b():
    return "456"

# 或者手動註冊函數
def custom_function(x, y):
    return x + y

worker.register_function("add", custom_function)

async def main():
    # 啟動服務器
    await worker.start_server(port=50052)

if __name__ == "__main__":
    asyncio.run(main())
