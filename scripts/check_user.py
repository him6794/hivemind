import sqlite3

# 連接到用戶數據庫
conn = sqlite3.connect('/mnt/myusb/hivemind/nodepool.db')
cursor = conn.cursor()

# 查詢 justin 用戶
cursor.execute("SELECT id, username, password FROM users WHERE username = 'justin'")
result = cursor.fetchone()

if result:
    print(f"用戶 ID: {result[0]}")
    print(f"用戶名: {result[1]}")
    print(f"密碼 hash: {result[2][:50]}...")
else:
    print("找不到用戶 justin")

conn.close()
