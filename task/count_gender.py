"""
計算 CSV 檔案中男女數量的工具
使用 Python 內建庫，並輸出運行時間
"""

import csv
import time
import os
from collections import Counter


def format_size(size_bytes: int) -> str:
    """格式化檔案大小顯示"""
    for unit in ['B', 'KB', 'MB', 'GB']:
        if size_bytes < 1024:
            return f"{size_bytes:.2f} {unit}"
        size_bytes /= 1024
    return f"{size_bytes:.2f} TB"


def format_number(n: int) -> str:
    """格式化數字，加入千分位"""
    return f"{n:,}"


def count_gender(file_path: str = "data.csv") -> dict:
    """
    計算 CSV 檔案中的男女數量
    
    Args:
        file_path: CSV 檔案路徑
    
    Returns:
        包含統計結果的字典
    """
    if not os.path.exists(file_path):
        print(f"錯誤：檔案 '{file_path}' 不存在！")
        return {}
    
    file_size = os.path.getsize(file_path)
    print(f"開始計算性別數量...")
    print(f"檔案: {file_path}")
    print(f"檔案大小: {format_size(file_size)}")
    print("-" * 50)
    
    start_time = time.time()
    
    gender_count = Counter()
    total_rows = 0
    
    with open(file_path, 'r', encoding='utf-8') as f:
        reader = csv.reader(f)
        # 跳過標題列
        header = next(reader)
        print(f"欄位: {header}")
        
        for row in reader:
            if len(row) >= 2:
                gender = row[1]
                gender_count[gender] += 1
                total_rows += 1
                
                # 每處理 1000 萬行顯示進度
                if total_rows % 10_000_000 == 0:
                    elapsed = time.time() - start_time
                    speed = total_rows / elapsed
                    print(f"\r已處理: {format_number(total_rows)} 行 | "
                          f"速度: {format_number(int(speed))} 行/秒", end='')
    
    end_time = time.time()
    duration = end_time - start_time
    
    print("\n" + "=" * 50)
    print("統計結果")
    print("=" * 50)
    
    for gender, count in sorted(gender_count.items()):
        percentage = (count / total_rows) * 100 if total_rows > 0 else 0
        print(f"  {gender}: {format_number(count)} ({percentage:.2f}%)")
    
    print("-" * 50)
    print(f"總行數: {format_number(total_rows)}")
    print(f"運行時間: {duration:.2f} 秒")
    print(f"處理速度: {format_number(int(total_rows / duration))} 行/秒")
    print(f"吞吐量: {format_size(file_size / duration)}/秒")
    print("=" * 50)
    
    return {
        'gender_count': dict(gender_count),
        'total_rows': total_rows,
        'duration': duration,
        'file_size': file_size
    }


def count_gender_fast(file_path: str = "data.csv") -> dict:
    """
    使用更快速的方式計算（直接字串處理，不使用 csv 模組）
    
    Args:
        file_path: CSV 檔案路徑
    
    Returns:
        包含統計結果的字典
    """
    if not os.path.exists(file_path):
        print(f"錯誤：檔案 '{file_path}' 不存在！")
        return {}
    
    file_size = os.path.getsize(file_path)
    print(f"開始計算性別數量（快速模式）...")
    print(f"檔案: {file_path}")
    print(f"檔案大小: {format_size(file_size)}")
    print("-" * 50)
    
    start_time = time.time()
    
    male_count = 0
    female_count = 0
    total_rows = 0
    
    with open(file_path, 'r', encoding='utf-8', buffering=8*1024*1024) as f:
        # 跳過標題列
        header = f.readline().strip()
        print(f"欄位: {header}")
        
        for line in f:
            # 直接檢查行尾的性別
            if line.rstrip().endswith('男'):
                male_count += 1
            elif line.rstrip().endswith('女'):
                female_count += 1
            total_rows += 1
            
            # 每處理 1000 萬行顯示進度
            if total_rows % 10_000_000 == 0:
                elapsed = time.time() - start_time
                speed = total_rows / elapsed
                print(f"\r已處理: {format_number(total_rows)} 行 | "
                      f"速度: {format_number(int(speed))} 行/秒", end='')
    
    end_time = time.time()
    duration = end_time - start_time
    
    gender_count = {'男': male_count, '女': female_count}
    
    print("\n" + "=" * 50)
    print("統計結果")
    print("=" * 50)
    
    for gender, count in sorted(gender_count.items()):
        percentage = (count / total_rows) * 100 if total_rows > 0 else 0
        print(f"  {gender}: {format_number(count)} ({percentage:.2f}%)")
    
    print("-" * 50)
    print(f"總行數: {format_number(total_rows)}")
    print(f"運行時間: {duration:.2f} 秒")
    print(f"處理速度: {format_number(int(total_rows / duration))} 行/秒")
    print(f"吞吐量: {format_size(file_size / duration)}/秒")
    print("=" * 50)
    
    return {
        'gender_count': gender_count,
        'total_rows': total_rows,
        'duration': duration,
        'file_size': file_size
    }


if __name__ == "__main__":
    import argparse
    
    parser = argparse.ArgumentParser(description="計算 CSV 檔案中的男女數量")
    parser.add_argument(
        "-i", "--input",
        default="data.csv",
        help="輸入檔案路徑 (預設: data.csv)"
    )
    parser.add_argument(
        "--fast",
        action="store_true",
        help="使用快速模式（直接字串處理）"
    )
    
    args = parser.parse_args()
    
    if args.fast:
        count_gender_fast(args.input)
    else:
        count_gender(args.input)
