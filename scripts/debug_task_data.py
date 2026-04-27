import redis

# 連接 Redis
r = redis.Redis(host='localhost', port=6379, decode_responses=True)

# 獲取所有任務
task_keys = r.keys("task:*")
print(f"找到 {len(task_keys)} 個任務\n")

if task_keys:
    # 檢查第一個任務
    first_task = task_keys[0]
    print(f"檢查任務: {first_task}")
    print("=" * 60)
    
    task_data = r.hgetall(first_task)
    for key, value in sorted(task_data.items()):
        if key == "task_zip":
            print(f"{key}: <binary data, {len(value)} bytes>")
        else:
            print(f"{key}: {value}")
    
    print("\n" + "=" * 60)
    print("\n關鍵欄位:")
    print(f"  user_id: {task_data.get('user_id')}")
    print(f"  username: {task_data.get('username')}")
    print(f"  status: {task_data.get('status')}")
