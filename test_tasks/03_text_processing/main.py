print("=" * 60)
print("HiveMind 測試任務 - 文字處理")
print("=" * 60)
print()

# 文字分析
text = """
HiveMind 是一個分散式運算平台。
它可以將計算任務分配到多個 Worker 節點執行。
支援 Python 任務，具有資源監控和限制功能。
使用 Monty 執行器，啟動速度極快。
"""

print("文字內容:")
print(text)
print()

# 統計分析
lines = text.strip().split('\n')
words = text.split()
chars = len(text)

print("文字統計:")
print(f"  行數: {len(lines)}")
print(f"  字數: {len(words)}")
print(f"  字元數: {chars}")
print()

# 字詞頻率
word_freq = {}
for word in words:
    word_freq[word] = word_freq.get(word, 0) + 1

print("字詞頻率 (前 5 個):")
sorted_words = sorted(word_freq.items(), key=lambda x: x[1], reverse=True)
for word, freq in sorted_words[:5]:
    print(f"  '{word}': {freq} 次")
print()

# 字串操作
sample = "HiveMind"
print("字串操作:")
print(f"  原始: {sample}")
print(f"  大寫: {sample.upper()}")
print(f"  小寫: {sample.lower()}")
print(f"  反轉: {sample[::-1]}")
print()

# 文字格式化
data = [
    ("任務 ID", "task-001"),
    ("狀態", "COMPLETED"),
    ("Worker", "worker-001"),
    ("執行時間", "2.5 秒"),
]

print("格式化輸出:")
for key, value in data:
    print(f"  {key:12s}: {value}")
print()

print("=" * 60)
print("文字處理完成！")
print("=" * 60)
