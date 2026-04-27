"""
CSV 拆分與部署工具 - 多進程版
將 CSV 拆成多個檔案，每個資料夾都有獨立的 CSV 和計算程式
"""

import os
import sys
import time
import shutil
import json
import zipfile
import multiprocessing
from concurrent.futures import ProcessPoolExecutor, as_completed


def format_number(n):
    return f"{n:,}"


def format_size(size_bytes):
    for unit in ['B', 'KB', 'MB', 'GB']:
        if size_bytes < 1024:
            return f"{size_bytes:.2f} {unit}"
        size_bytes /= 1024
    return f"{size_bytes:.2f} TB"


def get_worker_code(part_id, task_id, server_url, num_parts):
    code = '''"""
分散式性別計算 - 工作節點
"""

import os
import time
import json
import urllib.request
import urllib.error

CONFIG = {
    "part_id": ''' + str(part_id) + ''',
    "task_id": "''' + task_id + '''",
    "server_url": "''' + server_url + '''",
    "total_parts": ''' + str(num_parts) + ''',
    "csv_file": "data.csv"
}


def format_number(n):
    return f"{n:,}"


def send_result(url, data):
    json_data = json.dumps(data).encode('utf-8')
    req = urllib.request.Request(
        url,
        data=json_data,
        headers={'Content-Type': 'application/json'}
    )
    try:
        with urllib.request.urlopen(req, timeout=60) as response:
            return json.loads(response.read().decode('utf-8'))
    except urllib.error.URLError as e:
        return {'success': False, 'error': str(e)}


def count_gender(file_path):
    if not os.path.exists(file_path):
        return {'error': f"not exist: {file_path}"}
    
    file_size = os.path.getsize(file_path)
    print(f"Start counting...")
    print(f"File: {file_path}")
    print(f"Size: {file_size / (1024**2):.2f} MB")
    print("-" * 50)
    
    start_time = time.time()
    male_count = 0
    female_count = 0
    total_rows = 0
    
    with open(file_path, 'r', encoding='utf-8', buffering=8*1024*1024) as f:
        f.readline()
        
        for line in f:
            if line.rstrip().endswith('男'):
                male_count += 1
            elif line.rstrip().endswith('女'):
                female_count += 1
            total_rows += 1
            
            if total_rows % 5_000_000 == 0:
                elapsed = time.time() - start_time
                speed = total_rows / elapsed if elapsed > 0 else 0
                print(f"Processed: {format_number(total_rows)} | Speed: {format_number(int(speed))} rows/s", end="\\r")
    
    duration = time.time() - start_time
    
    print(f"\\n\\nDone!")
    print(f"Male: {format_number(male_count)}")
    print(f"Female: {format_number(female_count)}")
    print(f"Total: {format_number(total_rows)}")
    print(f"Time: {duration:.2f}s")
    
    return {
        'male_count': male_count,
        'female_count': female_count,
        'row_count': total_rows,
        'duration': duration,
        'file_size': file_size
    }


def main():
    print("=" * 60)
    print(f"Worker Node {CONFIG['part_id']}")
    print("=" * 60)
    
    script_dir = os.path.dirname(os.path.abspath(__file__))
    csv_path = os.path.join(script_dir, CONFIG['csv_file'])
    result = count_gender(csv_path)
    
    if 'error' in result:
        print(f"Error: {result['error']}")
        return
    
    print(f"\\nSubmitting to: {CONFIG['server_url']}")
    
    response = send_result(
        f"{CONFIG['server_url']}/api/task/submit",
        {
            'task_id': CONFIG['task_id'],
            'part_id': CONFIG['part_id'],
            'total_parts': CONFIG['total_parts'],
            'result': result
        }
    )
    
    if response.get('success'):
        print(f"Submitted! Progress: {response.get('completed_parts')}/{response.get('total_parts')}")
        
        if response.get('status') == 'completed':
            print("\\n" + "=" * 60)
            print("All workers done! Summary:")
            agg = response.get('aggregated', {})
            print(f"  Male: {format_number(agg.get('male_count', 0))} ({agg.get('male_percentage', 0):.2f}%)")
            print(f"  Female: {format_number(agg.get('female_count', 0))} ({agg.get('female_percentage', 0):.2f}%)")
            print(f"  Total: {format_number(agg.get('total_rows', 0))}")
            print(f"  Time: {agg.get('total_duration', 0):.2f}s")
            print("=" * 60)
    else:
        print(f"Failed: {response.get('error', 'unknown')}")
        with open('result.json', 'w', encoding='utf-8') as f:
            json.dump(result, f, ensure_ascii=False, indent=2)
        print("Saved to result.json")


if __name__ == "__main__":
    main()
'''
    return code


def write_part(args):
    input_file, output_dir, part_id, start_line, end_line, header, task_id, server_url, num_parts = args
    
    part_dir = os.path.join(output_dir, f"part_{part_id}")
    os.makedirs(part_dir, exist_ok=True)
    
    csv_path = os.path.join(part_dir, "data.csv")
    lines_written = 0
    
    try:
        with open(input_file, 'r', encoding='utf-8', buffering=16*1024*1024) as f_in:
            f_in.readline()
            
            for _ in range(start_line):
                f_in.readline()
            
            with open(csv_path, 'w', encoding='utf-8', buffering=8*1024*1024) as f_out:
                f_out.write(header)
                
                lines_to_write = end_line - start_line
                for _ in range(lines_to_write):
                    line = f_in.readline()
                    if not line:
                        break
                    f_out.write(line)
                    lines_written += 1
        
        worker_path = os.path.join(part_dir, "main.py")
        worker_code = get_worker_code(part_id, task_id, server_url, num_parts)
        with open(worker_path, 'w', encoding='utf-8') as f_code:
            f_code.write(worker_code)
        
        config_path = os.path.join(part_dir, "config.json")
        with open(config_path, 'w', encoding='utf-8') as f_config:
            json.dump({
                'part_id': part_id,
                'task_id': task_id,
                'server_url': server_url,
                'total_parts': num_parts,
                'lines': lines_written
            }, f_config, ensure_ascii=False, indent=2)
        
        part_size = os.path.getsize(csv_path)
        
        return {
            'part_id': part_id,
            'lines_written': lines_written,
            'size': part_size,
            'success': True
        }
        
    except Exception as e:
        return {
            'part_id': part_id,
            'error': str(e),
            'success': False
        }


def count_lines_fast(file_path):
    count = 0
    with open(file_path, 'rb') as f:
        buf_size = 64 * 1024 * 1024
        buf = f.raw.read(buf_size)
        while buf:
            count += buf.count(b'\n')
            buf = f.raw.read(buf_size)
    return count - 1


def create_launch_scripts(output_dir, num_parts, task_id, server_url):
    server_bat = os.path.join(output_dir, "run_server.bat")
    with open(server_bat, 'w', encoding='utf-8') as f:
        f.write('@echo off\n')
        f.write('echo Starting API Server...\n')
        f.write('python server.py -p 5000\n')
        f.write('pause\n')
    
    workers_bat = os.path.join(output_dir, "run_all_workers.bat")
    with open(workers_bat, 'w', encoding='utf-8') as f:
        f.write('@echo off\n')
        f.write('echo Starting all workers...\n')
        f.write('\n')
        f.write(f'echo Creating task: {task_id}\n')
        f.write(f'curl -X POST {server_url}/api/task/create ')
        f.write(f'-H "Content-Type: application/json" ')
        f.write(f'-d "{{\\"task_id\\": \\"{task_id}\\", \\"total_parts\\": {num_parts}}}"\n')
        f.write('echo.\n')
        f.write('timeout /t 2\n')
        f.write('\n')
        for i in range(num_parts):
            f.write(f'echo Starting worker {i}...\n')
            f.write(f'start "Worker {i}" cmd /k "cd part_{i} && python calculate.py"\n')
        f.write('\n')
        f.write('echo All workers started!\n')
        f.write('pause\n')
    
    ps_script = os.path.join(output_dir, "run_all.ps1")
    with open(ps_script, 'w', encoding='utf-8') as f:
        f.write('# Distributed calculation launcher\n\n')
        f.write(f'$task_id = "{task_id}"\n')
        f.write(f'$server_url = "{server_url}"\n')
        f.write(f'$num_parts = {num_parts}\n\n')
        f.write('Write-Host "Starting server..."\n')
        f.write('Start-Process -FilePath "python" -ArgumentList "server.py" -WorkingDirectory $PSScriptRoot\n')
        f.write('Start-Sleep -Seconds 3\n\n')
        f.write('Write-Host "Creating task..."\n')
        f.write('$body = @{ task_id = $task_id; total_parts = $num_parts } | ConvertTo-Json\n')
        f.write('Invoke-RestMethod -Uri "$server_url/api/task/create" -Method Post -Body $body -ContentType "application/json"\n\n')
        f.write('for ($i = 0; $i -lt $num_parts; $i++) {\n')
        f.write('    Write-Host "Starting worker $i..."\n')
        f.write('    Start-Process -FilePath "python" -ArgumentList "calculate.py" -WorkingDirectory "$PSScriptRoot\\part_$i"\n')
        f.write('}\n\n')
        f.write('Write-Host "All workers started!"\n')


def split_csv(input_file, output_dir, num_parts, task_id="task_001", 
              server_url="http://172.16.100.148:5000", num_workers=None):
    if not os.path.exists(input_file):
        print(f"Error: File '{input_file}' not found!")
        return
    
    if num_workers is None:
        num_workers = min(num_parts, multiprocessing.cpu_count())
    
    file_size = os.path.getsize(input_file)
    
    print("=" * 70)
    print("CSV Split Tool (Multi-process)")
    print("=" * 70)
    print(f"Input: {input_file}")
    print(f"Size: {format_size(file_size)}")
    print(f"Parts: {num_parts}")
    print(f"Workers: {num_workers}")
    print(f"Output: {output_dir}")
    print(f"Task ID: {task_id}")
    print(f"Server: {server_url}")
    print("=" * 70)
    
    if os.path.exists(output_dir):
        print(f"\nWarning: '{output_dir}' exists, will overwrite!")
        response = input("Continue? (y/n): ")
        if response.lower() != 'y':
            print("Cancelled.")
            return
        shutil.rmtree(output_dir)
    
    os.makedirs(output_dir)
    
    print("\nCounting lines...")
    start_time = time.time()
    total_lines = count_lines_fast(input_file)
    print(f"Total lines: {format_number(total_lines)} ({time.time() - start_time:.2f}s)")
    
    lines_per_part = total_lines // num_parts
    print(f"Lines per part: ~{format_number(lines_per_part)}")
    print(f"Size per part: ~{format_size(file_size / num_parts)}")
    
    with open(input_file, 'r', encoding='utf-8') as f:
        header = f.readline()
    
    part_args = []
    for part_id in range(num_parts):
        start_line = part_id * lines_per_part
        if part_id == num_parts - 1:
            end_line = total_lines
        else:
            end_line = (part_id + 1) * lines_per_part
        
        part_args.append((
            input_file, output_dir, part_id, start_line, end_line,
            header, task_id, server_url, num_parts
        ))
    
    print(f"\nSplitting ({num_workers} workers)...")
    print("-" * 70)
    
    split_start = time.time()
    results = []
    
    with ProcessPoolExecutor(max_workers=num_workers) as executor:
        futures = {executor.submit(write_part, args): args[2] for args in part_args}
        
        for future in as_completed(futures):
            part_id = futures[future]
            try:
                result = future.result()
                results.append(result)
                
                if result['success']:
                    progress = len(results) / num_parts * 100
                    print(f"  Part {result['part_id']}: {format_number(result['lines_written'])} lines, "
                          f"{format_size(result['size'])} [{progress:.0f}%]")
                else:
                    print(f"  Part {result['part_id']}: Failed - {result.get('error')}")
                    
            except Exception as e:
                print(f"  Part {part_id}: Exception - {e}")
    
    split_duration = time.time() - split_start
    
    server_script = os.path.join(os.path.dirname(os.path.abspath(__file__)), 
                                  "distributed_server.py")
    if os.path.exists(server_script):
        shutil.copy(server_script, os.path.join(output_dir, "server.py"))
        print(f"\nCopied server.py to {output_dir}/")
    
    create_launch_scripts(output_dir, num_parts, task_id, server_url)
    
    total_written = sum(r.get('lines_written', 0) for r in results if r.get('success'))
    total_size_written = sum(r.get('size', 0) for r in results if r.get('success'))
    
    print("\n" + "=" * 70)
    print("Split Complete!")
    print("=" * 70)
    print(f"Time: {split_duration:.2f}s")
    print(f"Speed: {format_size(total_size_written / split_duration)}/s")
    print(f"Total lines: {format_number(total_written)}")
    print(f"Total size: {format_size(total_size_written)}")
    print(f"Output: {output_dir}")
    print("")
    print("Structure:")
    print(f"  {output_dir}/")
    print(f"  ├── server.py")
    print(f"  ├── run_server.bat")
    print(f"  ├── run_all_workers.bat")
    for i in range(min(num_parts, 3)):
        print(f"  ├── part_{i}/")
        print(f"  │   ├── data.csv")
        print(f"  │   ├── calculate.py")
        print(f"  │   └── config.json")
    if num_parts > 3:
        print(f"  └── ... ({num_parts} part folders total)")
    
    # 打包成 zip
    zip_filename = f"{output_dir}.zip"
    print(f"\nCreating zip: {zip_filename}...")
    zip_start = time.time()
    
    with zipfile.ZipFile(zip_filename, 'w', zipfile.ZIP_DEFLATED) as zipf:
        for root, dirs, files in os.walk(output_dir):
            for file in files:
                file_path = os.path.join(root, file)
                arcname = os.path.relpath(file_path, os.path.dirname(output_dir))
                zipf.write(file_path, arcname)
    
    zip_size = os.path.getsize(zip_filename)
    zip_duration = time.time() - zip_start
    print(f"Zip created: {zip_filename}")
    print(f"Zip size: {format_size(zip_size)}")
    print(f"Zip time: {zip_duration:.2f}s")
    
    print("\n" + "=" * 70)
    print("Usage:")
    print("=" * 70)
    print(f"1. cd {output_dir} && python server.py")
    print(f"2. Run run_all_workers.bat")
    print(f"\nOr distribute: {zip_filename}")
    print("=" * 70)


if __name__ == "__main__":
    import argparse
    
    parser = argparse.ArgumentParser(description="CSV Split Tool")
    parser.add_argument("-i", "--input", default="data.csv", help="Input CSV file")
    parser.add_argument("-o", "--output", default="distributed_task", help="Output directory")
    parser.add_argument("-n", "--num-parts", type=int, default=4, help="Number of parts")
    parser.add_argument("-w", "--workers", type=int, default=None, help="Number of workers")
    parser.add_argument("--task-id", default="task_001", help="Task ID")
    parser.add_argument("-s", "--server", default="http://172.16.100.148:5000", help="API server URL")
    
    args = parser.parse_args()
    
    split_csv(
        input_file=args.input,
        output_dir=args.output,
        num_parts=args.num_parts,
        task_id=args.task_id,
        server_url=args.server,
        num_workers=args.workers
    )
