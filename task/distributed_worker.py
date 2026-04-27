"""
分散式性別計算系統 - 工作節點
負責處理分配到的 CSV 部分並回報結果
"""

import os
import time
import json
import urllib.request
import urllib.error
from typing import Optional


def format_number(n: int) -> str:
    """格式化數字"""
    return f"{n:,}"


def format_size(size_bytes: float) -> str:
    """格式化檔案大小"""
    for unit in ['B', 'KB', 'MB', 'GB']:
        if size_bytes < 1024:
            return f"{size_bytes:.2f} {unit}"
        size_bytes /= 1024
    return f"{size_bytes:.2f} TB"


def send_request(url: str, data: dict) -> dict:
    """發送 HTTP POST 請求（使用內建庫）"""
    json_data = json.dumps(data).encode('utf-8')
    
    req = urllib.request.Request(
        url,
        data=json_data,
        headers={'Content-Type': 'application/json'}
    )
    
    try:
        with urllib.request.urlopen(req, timeout=30) as response:
            return json.loads(response.read().decode('utf-8'))
    except urllib.error.URLError as e:
        return {'success': False, 'error': str(e)}


def get_file_line_count(file_path: str) -> int:
    """快速計算檔案行數"""
    count = 0
    with open(file_path, 'rb') as f:
        for _ in f:
            count += 1
    return count - 1  # 減去標題列


def count_gender_part(
    file_path: str,
    part_id: int,
    total_parts: int,
    total_lines: Optional[int] = None
) -> dict:
    """
    計算 CSV 檔案中指定部分的男女數量
    
    Args:
        file_path: CSV 檔案路徑
        part_id: 部分編號 (0-based)
        total_parts: 總共分成幾份
        total_lines: 總行數（可選，不提供則自動計算）
    
    Returns:
        計算結果
    """
    if not os.path.exists(file_path):
        return {'error': f"檔案 '{file_path}' 不存在"}
    
    # 計算總行數（如果未提供）
    if total_lines is None:
        print(f"[Part {part_id}] 計算檔案行數...")
        total_lines = get_file_line_count(file_path)
    
    # 計算這個部分負責的行範圍
    lines_per_part = total_lines // total_parts
    start_line = part_id * lines_per_part
    
    # 最後一個部分負責剩餘的所有行
    if part_id == total_parts - 1:
        end_line = total_lines
    else:
        end_line = (part_id + 1) * lines_per_part
    
    lines_to_process = end_line - start_line
    
    print(f"[Part {part_id}] 處理範圍: 行 {start_line + 1} - {end_line}")
    print(f"[Part {part_id}] 預計處理: {format_number(lines_to_process)} 行")
    
    start_time = time.time()
    
    male_count = 0
    female_count = 0
    processed = 0
    current_line = 0
    
    with open(file_path, 'r', encoding='utf-8', buffering=8*1024*1024) as f:
        # 跳過標題列
        f.readline()
        
        for line in f:
            # 跳過不屬於這個部分的行
            if current_line < start_line:
                current_line += 1
                continue
            
            # 超過範圍則停止
            if current_line >= end_line:
                break
            
            # 處理這一行
            if line.rstrip().endswith('男'):
                male_count += 1
            elif line.rstrip().endswith('女'):
                female_count += 1
            
            processed += 1
            current_line += 1
            
            # 顯示進度
            if processed % 5_000_000 == 0:
                elapsed = time.time() - start_time
                speed = processed / elapsed if elapsed > 0 else 0
                progress = processed / lines_to_process * 100
                print(f"\r[Part {part_id}] 進度: {progress:.1f}% | "
                      f"已處理: {format_number(processed)} | "
                      f"速度: {format_number(int(speed))} 行/秒", end='')
    
    end_time = time.time()
    duration = end_time - start_time
    
    print(f"\n[Part {part_id}] 完成！耗時: {duration:.2f} 秒")
    
    return {
        'part_id': part_id,
        'male_count': male_count,
        'female_count': female_count,
        'row_count': processed,
        'duration': duration,
        'start_line': start_line,
        'end_line': end_line
    }


def run_worker(
    file_path: str,
    part_id: int,
    total_parts: int,
    task_id: str,
    server_url: str = "http://localhost:5000",
    total_lines: Optional[int] = None
):
    """
    執行工作節點
    
    Args:
        file_path: CSV 檔案路徑
        part_id: 部分編號 (0-based)
        total_parts: 總共分成幾份
        task_id: 任務 ID
        server_url: API 伺服器 URL
        total_lines: 總行數（可選）
    """
    print("=" * 60)
    print(f"分散式性別計算 - 工作節點 (Part {part_id})")
    print("=" * 60)
    print(f"檔案: {file_path}")
    print(f"任務 ID: {task_id}")
    print(f"分割: {part_id + 1}/{total_parts}")
    print(f"伺服器: {server_url}")
    print("-" * 60)
    
    # 執行計算
    result = count_gender_part(file_path, part_id, total_parts, total_lines)
    
    if 'error' in result:
        print(f"錯誤: {result['error']}")
        return
    
    # 回報結果
    print(f"\n[Part {part_id}] 回報結果到伺服器...")
    
    response = send_request(
        f"{server_url}/api/task/submit",
        {
            'task_id': task_id,
            'part_id': part_id,
            'result': {
                'male_count': result['male_count'],
                'female_count': result['female_count'],
                'row_count': result['row_count'],
                'duration': result['duration']
            }
        }
    )
    
    if response.get('success'):
        print(f"[Part {part_id}] 結果已提交！")
        print(f"[Part {part_id}] 任務進度: {response.get('completed_parts')}/{response.get('total_parts')}")
        
        if response.get('status') == 'completed':
            print("\n" + "=" * 60)
            print("所有部分已完成！彙總結果:")
            print("=" * 60)
            agg = response.get('aggregated', {})
            print(f"  男: {format_number(agg.get('male_count', 0))} ({agg.get('male_percentage', 0):.2f}%)")
            print(f"  女: {format_number(agg.get('female_count', 0))} ({agg.get('female_percentage', 0):.2f}%)")
            print(f"  總行數: {format_number(agg.get('total_rows', 0))}")
            print(f"  總耗時: {agg.get('total_duration', 0):.2f} 秒")
            print("=" * 60)
    else:
        print(f"[Part {part_id}] 提交失敗: {response.get('error', '未知錯誤')}")


if __name__ == "__main__":
    import argparse
    
    parser = argparse.ArgumentParser(description="分散式性別計算 - 工作節點")
    parser.add_argument(
        "-i", "--input",
        default="data.csv",
        help="輸入檔案路徑 (預設: data.csv)"
    )
    parser.add_argument(
        "-p", "--part",
        type=int,
        required=True,
        help="部分編號 (0-based)"
    )
    parser.add_argument(
        "-t", "--total",
        type=int,
        required=True,
        help="總共分成幾份"
    )
    parser.add_argument(
        "--task-id",
        default="task_001",
        help="任務 ID (預設: task_001)"
    )
    parser.add_argument(
        "-s", "--server",
        default="http://localhost:5000",
        help="API 伺服器 URL (預設: http://localhost:5000)"
    )
    parser.add_argument(
        "-l", "--lines",
        type=int,
        default=None,
        help="總行數（可選，不提供則自動計算）"
    )
    
    args = parser.parse_args()
    
    run_worker(
        file_path=args.input,
        part_id=args.part,
        total_parts=args.total,
        task_id=args.task_id,
        server_url=args.server,
        total_lines=args.lines
    )
