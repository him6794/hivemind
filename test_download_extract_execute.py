"""
完整測試：任務下載、解壓、執行流程
測試涵蓋：
1. 創建測試任務 ZIP
2. 模擬下載（從本地讀取）
3. 解壓 ZIP 到工作目錄
4. 執行任務腳本
5. 驗證執行結果
"""

import io
import os
import sys
import time
import zipfile
import tempfile
import shutil
from pathlib import Path

# 測試任務腳本
TEST_TASKS = {
    "simple_print": {
        "main.py": """
print("Hello from HiveMind!")
print("Task execution successful")
result = 42
print(f"Result: {result}")
"""
    },
    "prime_calculation": {
        "main.py": """
def is_prime(n):
    if n < 2:
        return False
    if n == 2:
        return True
    if n % 2 == 0:
        return False
    i = 3
    while i * i <= n:
        if n % i == 0:
            return False
        i += 2
    return True

primes = [n for n in range(2, 100) if is_prime(n)]
print(f"Found {len(primes)} primes under 100")
print(f"First 10 primes: {primes[:10]}")
print("Prime calculation completed successfully")
"""
    },
    "file_operations": {
        "main.py": """
# 測試文件讀寫
with open('output.txt', 'w', encoding='utf-8') as f:
    f.write("Task output data\\n")
    f.write("Generated at runtime\\n")
    f.write("Status: Success\\n")

with open('output.txt', 'r', encoding='utf-8') as f:
    content = f.read()
    print("File content:")
    print(content)

print("File operations completed")
""",
        "data.txt": "Sample input data for processing"
    }
}


def create_task_zip(task_name: str, files: dict) -> bytes:
    """創建任務 ZIP 文件"""
    print(f"\n[1] 創建任務 ZIP: {task_name}")
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, 'w', zipfile.ZIP_DEFLATED) as zf:
        for filename, content in files.items():
            zf.writestr(filename, content.strip())
            print(f"    添加文件: {filename} ({len(content)} bytes)")
    
    zip_bytes = buf.getvalue()
    print(f"    ZIP 大小: {len(zip_bytes)} bytes")
    return zip_bytes


def simulate_download(zip_bytes: bytes, save_path: Path) -> bool:
    """模擬下載任務 ZIP（實際是從內存寫入磁盤）"""
    print(f"\n[2] 模擬下載任務到: {save_path}")
    try:
        save_path.parent.mkdir(parents=True, exist_ok=True)
        save_path.write_bytes(zip_bytes)
        print(f"    下載完成: {len(zip_bytes)} bytes")
        return True
    except Exception as e:
        print(f"    下載失敗: {e}")
        return False


def extract_task_zip(zip_path: Path, extract_dir: Path) -> bool:
    """解壓任務 ZIP 到工作目錄"""
    print(f"\n[3] 解壓任務到: {extract_dir}")
    try:
        extract_dir.mkdir(parents=True, exist_ok=True)
        with zipfile.ZipFile(zip_path, 'r') as zf:
            # 安全檢查：防止路徑遍歷攻擊
            for member in zf.infolist():
                member_path = Path(extract_dir) / member.filename
                if not str(member_path.resolve()).startswith(str(extract_dir.resolve())):
                    raise ValueError(f"不安全的 ZIP 路徑: {member.filename}")
            
            zf.extractall(extract_dir)
            files = list(extract_dir.rglob('*'))
            print(f"    解壓完成，共 {len([f for f in files if f.is_file()])} 個文件:")
            for f in files:
                if f.is_file():
                    print(f"      - {f.name} ({f.stat().st_size} bytes)")
        return True
    except Exception as e:
        print(f"    解壓失敗: {e}")
        return False


def execute_task(workspace: Path) -> tuple[bool, str]:
    """執行任務腳本"""
    print(f"\n[4] 執行任務腳本")
    
    # 尋找可執行腳本
    script_candidates = ['main.py', 'run.py', 'app.py', 'start.py']
    script_path = None
    for candidate in script_candidates:
        candidate_path = workspace / candidate
        if candidate_path.exists():
            script_path = candidate_path
            print(f"    找到腳本: {candidate}")
            break
    
    if not script_path:
        print("    錯誤: 找不到可執行腳本")
        return False, "No executable script found"
    
    # 執行腳本
    try:
        import subprocess
        
        start_time = time.time()
        result = subprocess.run(
            [sys.executable, str(script_path)],
            cwd=str(workspace),
            capture_output=True,
            text=True,
            timeout=30,
            encoding='utf-8',
            errors='replace'
        )
        duration = time.time() - start_time
        
        print(f"    執行完成 (耗時: {duration:.2f}s)")
        print(f"    返回碼: {result.returncode}")
        
        if result.stdout:
            print(f"    標準輸出:")
            for line in result.stdout.splitlines():
                print(f"      {line}")
        
        if result.stderr:
            print(f"    標準錯誤:")
            for line in result.stderr.splitlines():
                print(f"      {line}")
        
        success = result.returncode == 0
        output = result.stdout + result.stderr
        
        return success, output
        
    except subprocess.TimeoutExpired:
        print("    錯誤: 執行超時")
        return False, "Execution timeout"
    except Exception as e:
        print(f"    錯誤: {e}")
        return False, str(e)


def verify_results(workspace: Path, expected_files: list = None) -> bool:
    """驗證執行結果"""
    print(f"\n[5] 驗證執行結果")
    
    # 檢查工作目錄中的文件
    files = list(workspace.rglob('*'))
    result_files = [f for f in files if f.is_file()]
    
    print(f"    工作目錄文件 ({len(result_files)} 個):")
    for f in result_files:
        print(f"      - {f.name} ({f.stat().st_size} bytes)")
        
        # 顯示文本文件內容
        if f.suffix in ['.txt', '.log', '.json', '.csv']:
            try:
                content = f.read_text(encoding='utf-8', errors='replace')
                if len(content) < 500:
                    print(f"        內容預覽:")
                    for line in content.splitlines()[:10]:
                        print(f"          {line}")
            except Exception:
                pass
    
    # 檢查預期文件
    if expected_files:
        print(f"    檢查預期文件:")
        all_found = True
        for expected in expected_files:
            exists = (workspace / expected).exists()
            status = "[OK]" if exists else "[X]"
            print(f"      {status} {expected}")
            if not exists:
                all_found = False
        return all_found
    
    return True


def package_results(workspace: Path, output_zip: Path) -> bool:
    """打包執行結果"""
    print(f"\n[6] 打包執行結果")
    try:
        with zipfile.ZipFile(output_zip, 'w', zipfile.ZIP_DEFLATED) as zf:
            # 添加執行日誌
            log_content = f"""Task Execution Log
==================
Timestamp: {time.strftime('%Y-%m-%d %H:%M:%S')}
Workspace: {workspace}
Status: Completed
"""
            zf.writestr('execution_log.txt', log_content)
            
            # 添加工作目錄所有文件
            for file_path in workspace.rglob('*'):
                if file_path.is_file():
                    arcname = file_path.relative_to(workspace)
                    zf.write(file_path, arcname)
                    print(f"    添加: {arcname}")
        
        result_size = output_zip.stat().st_size
        print(f"    結果 ZIP 大小: {result_size} bytes")
        return True
    except Exception as e:
        print(f"    打包失敗: {e}")
        return False


def run_test(task_name: str, task_files: dict):
    """運行單個測試"""
    print(f"\n{'='*70}")
    print(f"測試任務: {task_name}")
    print(f"{'='*70}")
    
    # 創建臨時目錄
    test_dir = Path(tempfile.mkdtemp(prefix=f"hivemind_test_{task_name}_"))
    print(f"測試目錄: {test_dir}")
    
    try:
        # 1. 創建任務 ZIP
        zip_bytes = create_task_zip(task_name, task_files)
        
        # 2. 模擬下載
        zip_path = test_dir / f"{task_name}.zip"
        if not simulate_download(zip_bytes, zip_path):
            print(f"\n[FAIL] 測試失敗: 下載階段")
            return False
        
        # 3. 解壓
        workspace = test_dir / "workspace"
        if not extract_task_zip(zip_path, workspace):
            print(f"\n[FAIL] 測試失敗: 解壓階段")
            return False
        
        # 4. 執行
        success, output = execute_task(workspace)
        if not success:
            print(f"\n[FAIL] 測試失敗: 執行階段")
            return False
        
        # 5. 驗證
        expected_files = ['main.py']
        if 'output.txt' in output or 'file_operations' in task_name:
            expected_files.append('output.txt')
        
        if not verify_results(workspace, expected_files):
            print(f"\n[WARN] 警告: 部分預期文件未找到")
        
        # 6. 打包結果
        result_zip = test_dir / f"{task_name}_result.zip"
        if not package_results(workspace, result_zip):
            print(f"\n[WARN] 警告: 結果打包失敗")
        
        print(f"\n[PASS] 測試成功: {task_name}")
        return True
        
    except Exception as e:
        print(f"\n[ERROR] 測試異常: {e}")
        import traceback
        traceback.print_exc()
        return False
    
    finally:
        # 清理臨時目錄
        try:
            shutil.rmtree(test_dir)
            print(f"\n清理測試目錄: {test_dir}")
        except Exception:
            pass


def main():
    """主測試流程"""
    print("="*70)
    print("HiveMind 任務下載/解壓/執行 完整測試")
    print("="*70)
    
    results = {}
    
    # 運行所有測試
    for task_name, task_files in TEST_TASKS.items():
        success = run_test(task_name, task_files)
        results[task_name] = success
    
    # 總結
    print(f"\n{'='*70}")
    print("測試總結")
    print(f"{'='*70}")
    
    total = len(results)
    passed = sum(1 for v in results.values() if v)
    failed = total - passed
    
    for task_name, success in results.items():
        status = "[PASS]" if success else "[FAIL]"
        print(f"  {status}  {task_name}")
    
    print(f"\n總計: {total} 個測試")
    print(f"通過: {passed} 個")
    print(f"失敗: {failed} 個")
    print(f"成功率: {passed/total*100:.1f}%")
    
    if failed == 0:
        print(f"\n*** 所有測試通過！***")
        return 0
    else:
        print(f"\n*** 有 {failed} 個測試失敗 ***")
        return 1


if __name__ == "__main__":
    sys.exit(main())
