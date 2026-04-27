"""
大量 CSV 資料生成工具
生成包含 id 和性別欄位的 CSV 檔案
"""

import csv
import random
import os
import time
from pathlib import Path


def get_file_size_gb(file_path: str) -> float:
    """取得檔案大小（GB）"""
    if os.path.exists(file_path):
        return os.path.getsize(file_path) / (1024 ** 3)
    return 0


def format_size(size_bytes: int) -> str:
    """格式化檔案大小顯示"""
    for unit in ['B', 'KB', 'MB', 'GB']:
        if size_bytes < 1024:
            return f"{size_bytes:.2f} {unit}"
        size_bytes /= 1024
    return f"{size_bytes:.2f} TB"


def generate_csv(
    output_file: str = "data.csv",
    target_size_gb: float = 20.0,
    batch_size: int = 1_000_000,
    show_progress: bool = True
):
    """
    生成大量 CSV 資料
    
    Args:
        output_file: 輸出檔案路徑
        target_size_gb: 目標檔案大小（GB）
        batch_size: 每批次寫入的行數
        show_progress: 是否顯示進度
    """
    target_size_bytes = target_size_gb * (1024 ** 3)
    genders = ['男', '女']
    
    print(f"開始生成 CSV 檔案...")
    print(f"目標大小: {target_size_gb} GB")
    print(f"輸出檔案: {output_file}")
    print(f"批次大小: {batch_size:,} 行")
    print("-" * 50)
    
    start_time = time.time()
    current_id = 1
    total_rows = 0
    
    with open(output_file, 'w', newline='', encoding='utf-8') as f:
        writer = csv.writer(f)
        # 寫入標題列
        writer.writerow(['id', '性別'])
        
        while True:
            # 檢查目前檔案大小
            current_size = os.path.getsize(output_file)
            
            if current_size >= target_size_bytes:
                break
            
            # 生成一批資料
            batch_data = []
            for _ in range(batch_size):
                gender = random.choice(genders)
                batch_data.append([current_id, gender])
                current_id += 1
            
            # 批次寫入
            writer.writerows(batch_data)
            total_rows += batch_size
            
            # 顯示進度
            if show_progress:
                current_size = os.path.getsize(output_file)
                progress = (current_size / target_size_bytes) * 100
                elapsed = time.time() - start_time
                speed = current_size / elapsed if elapsed > 0 else 0
                
                # 預估剩餘時間
                if speed > 0:
                    remaining_bytes = target_size_bytes - current_size
                    eta_seconds = remaining_bytes / speed
                    eta_str = time.strftime("%H:%M:%S", time.gmtime(eta_seconds))
                else:
                    eta_str = "計算中..."
                
                print(f"\r進度: {progress:.2f}% | "
                      f"大小: {format_size(current_size)} | "
                      f"行數: {total_rows:,} | "
                      f"速度: {format_size(speed)}/s | "
                      f"剩餘時間: {eta_str}    ", end='')
    
    # 完成統計
    end_time = time.time()
    final_size = os.path.getsize(output_file)
    duration = end_time - start_time
    
    print("\n" + "=" * 50)
    print("生成完成！")
    print(f"總行數: {total_rows:,}")
    print(f"檔案大小: {format_size(final_size)}")
    print(f"耗時: {time.strftime('%H:%M:%S', time.gmtime(duration))}")
    print(f"平均速度: {format_size(final_size / duration)}/s")
    print("=" * 50)


def generate_csv_fast(
    output_file: str = "data.csv",
    target_size_gb: float = 20.0,
    show_progress: bool = True
):
    """
    使用更快速的方式生成 CSV（直接字串操作）
    
    Args:
        output_file: 輸出檔案路徑
        target_size_gb: 目標檔案大小（GB）
        show_progress: 是否顯示進度
    """
    target_size_bytes = target_size_gb * (1024 ** 3)
    genders = ['男', '女']
    batch_size = 500_000  # 每批次行數
    
    print(f"開始生成 CSV 檔案（快速模式）...")
    print(f"目標大小: {target_size_gb} GB")
    print(f"輸出檔案: {output_file}")
    print("-" * 50)
    
    start_time = time.time()
    current_id = 1
    total_rows = 0
    
    with open(output_file, 'w', encoding='utf-8', buffering=8*1024*1024) as f:
        # 寫入標題列
        f.write('id,性別\n')
        
        while True:
            # 檢查目前檔案大小
            f.flush()
            current_size = os.path.getsize(output_file)
            
            if current_size >= target_size_bytes:
                break
            
            # 生成一批資料（使用字串拼接）
            lines = []
            for _ in range(batch_size):
                gender = random.choice(genders)
                lines.append(f"{current_id},{gender}")
                current_id += 1
            
            # 批次寫入
            f.write('\n'.join(lines) + '\n')
            total_rows += batch_size
            
            # 顯示進度
            if show_progress:
                f.flush()
                current_size = os.path.getsize(output_file)
                progress = (current_size / target_size_bytes) * 100
                elapsed = time.time() - start_time
                speed = current_size / elapsed if elapsed > 0 else 0
                
                if speed > 0:
                    remaining_bytes = target_size_bytes - current_size
                    eta_seconds = remaining_bytes / speed
                    eta_str = time.strftime("%H:%M:%S", time.gmtime(eta_seconds))
                else:
                    eta_str = "計算中..."
                
                print(f"\r進度: {progress:.2f}% | "
                      f"大小: {format_size(current_size)} | "
                      f"行數: {total_rows:,} | "
                      f"速度: {format_size(speed)}/s | "
                      f"剩餘時間: {eta_str}    ", end='')
    
    # 完成統計
    end_time = time.time()
    final_size = os.path.getsize(output_file)
    duration = end_time - start_time
    
    print("\n" + "=" * 50)
    print("生成完成！")
    print(f"總行數: {total_rows:,}")
    print(f"檔案大小: {format_size(final_size)}")
    print(f"耗時: {time.strftime('%H:%M:%S', time.gmtime(duration))}")
    print(f"平均速度: {format_size(final_size / duration)}/s")
    print("=" * 50)


if __name__ == "__main__":
    import argparse
    
    parser = argparse.ArgumentParser(description="生成大量 CSV 資料")
    parser.add_argument(
        "-o", "--output",
        default="data.csv",
        help="輸出檔案路徑 (預設: data.csv)"
    )
    parser.add_argument(
        "-s", "--size",
        type=float,
        default=20.0,
        help="目標檔案大小（GB）(預設: 20)"
    )
    parser.add_argument(
        "--fast",
        action="store_true",
        help="使用快速模式（直接字串操作）"
    )
    parser.add_argument(
        "-q", "--quiet",
        action="store_true",
        help="安靜模式，不顯示進度"
    )
    
    args = parser.parse_args()
    
    if args.fast:
        generate_csv_fast(
            output_file=args.output,
            target_size_gb=args.size,
            show_progress=not args.quiet
        )
    else:
        generate_csv(
            output_file=args.output,
            target_size_gb=args.size,
            show_progress=not args.quiet
        )
