"""測試 NodePool store_task 函數的參數"""
import sys
import os
sys.path.insert(0, os.path.join(os.path.dirname(__file__), '..', '..', 'node_pool'))

from master_node_service import TaskManager
from database_manager import DatabaseManager
import inspect

# 檢查 store_task 函數簽名
sig = inspect.signature(TaskManager.store_task)
print("=" * 60)
print("TaskManager.store_task 函數簽名：")
print("=" * 60)
print(f"參數: {list(sig.parameters.keys())}")
print()

# 顯示完整簽名
print(f"完整簽名: {sig}")
print()

# 檢查參數詳情
print("參數詳情：")
for param_name, param in sig.parameters.items():
    print(f"  - {param_name}: {param.annotation if param.annotation != inspect.Parameter.empty else 'Any'}")
    if param.default != inspect.Parameter.empty:
        print(f"    預設值: {param.default}")
print()

print("=" * 60)
print("✅ 檢查完成")
print("=" * 60)
print()
print("預期的參數名稱應該包含 'user_identifier'")
print("如果看到 'user_identifier' 則修復成功")
