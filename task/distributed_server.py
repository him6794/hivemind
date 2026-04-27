"""
分散式性別計算系統 - API 伺服器
使用 Flask 接收各工作節點的計算結果並彙總
支援長期儲存任務結果
"""

import os
import time
import json
import threading
from flask import Flask, jsonify, request
from collections import defaultdict


app = Flask(__name__)

# 儲存檔案路徑
STORAGE_DIR = os.path.dirname(os.path.abspath(__file__))
TASKS_FILE = os.path.join(STORAGE_DIR, "tasks_data.json")

# 全域狀態管理
class TaskManager:
    def __init__(self, storage_file=TASKS_FILE):
        self.lock = threading.Lock()
        self.storage_file = storage_file
        self.tasks = self._load_tasks()  # task_id -> task_info
    
    def _load_tasks(self):
        """從檔案載入任務資料"""
        if os.path.exists(self.storage_file):
            try:
                with open(self.storage_file, 'r', encoding='utf-8') as f:
                    data = json.load(f)
                    print(f"已載入 {len(data)} 個歷史任務")
                    return data
            except Exception as e:
                print(f"載入任務資料失敗: {e}")
        return {}
    
    def _save_tasks(self):
        """儲存任務資料到檔案"""
        try:
            with open(self.storage_file, 'w', encoding='utf-8') as f:
                json.dump(self.tasks, f, ensure_ascii=False, indent=2)
        except Exception as e:
            print(f"儲存任務資料失敗: {e}")
        
    def create_task(self, task_id: str, total_parts: int, file_path: str):
        """建立新任務"""
        with self.lock:
            self.tasks[task_id] = {
                'task_id': task_id,
                'file_path': file_path,
                'total_parts': total_parts,
                'completed_parts': 0,
                'results': {},  # part_id -> result
                'start_time': time.time(),
                'end_time': None,
                'status': 'running',
                'aggregated': None,
                'created_at': time.strftime('%Y-%m-%d %H:%M:%S')
            }
            self._save_tasks()
        return self.tasks[task_id]
    
    def submit_result(self, task_id: str, part_id: int, result: dict, total_parts: int = None):
        """提交部分結果，如果任務不存在且提供了 total_parts 則自動建立"""
        with self.lock:
            # 如果任務不存在，自動建立
            if task_id not in self.tasks:
                if total_parts is not None:
                    self.tasks[task_id] = {
                        'task_id': task_id,
                        'file_path': 'auto-created',
                        'total_parts': total_parts,
                        'completed_parts': 0,
                        'results': {},
                        'start_time': time.time(),
                        'end_time': None,
                        'status': 'running',
                        'aggregated': None,
                        'created_at': time.strftime('%Y-%m-%d %H:%M:%S')
                    }
                    print(f"自動建立任務: {task_id} (total_parts={total_parts})")
                else:
                    return False, "任務不存在"
            
            task = self.tasks[task_id]
            task['results'][str(part_id)] = result  # JSON key 必須是字串
            task['completed_parts'] = len(task['results'])
            
            # 檢查是否所有部分都完成
            if task['completed_parts'] >= task['total_parts']:
                task['status'] = 'completed'
                task['end_time'] = time.time()
                task['completed_at'] = time.strftime('%Y-%m-%d %H:%M:%S')
                task['aggregated'] = self._aggregate_results(task)
            
            self._save_tasks()
            return True, "結果已提交"
    
    def _aggregate_results(self, task: dict) -> dict:
        """彙總所有結果"""
        total_male = 0
        total_female = 0
        total_rows = 0
        total_duration = 0
        
        for part_id, result in task['results'].items():
            total_male += result.get('male_count', 0)
            total_female += result.get('female_count', 0)
            total_rows += result.get('row_count', 0)
            total_duration = max(total_duration, result.get('duration', 0))
        
        total_count = total_male + total_female
        return {
            'male_count': total_male,
            'female_count': total_female,
            'male_percentage': (total_male / total_count * 100) if total_count > 0 else 0,
            'female_percentage': (total_female / total_count * 100) if total_count > 0 else 0,
            'total_rows': total_rows,
            'total_duration': task['end_time'] - task['start_time'],
            'max_worker_duration': total_duration
        }
    
    def get_task(self, task_id: str):
        """取得任務資訊"""
        with self.lock:
            return self.tasks.get(task_id)
    
    def get_all_tasks(self):
        """取得所有任務"""
        with self.lock:
            return dict(self.tasks)
    
    def delete_task(self, task_id: str):
        """刪除任務"""
        with self.lock:
            if task_id in self.tasks:
                del self.tasks[task_id]
                self._save_tasks()
                return True
            return False
    
    def export_results(self, task_id: str = None, include_running: bool = False):
        """匯出任務結果"""
        with self.lock:
            if task_id:
                task = self.tasks.get(task_id)
                if task:
                    if task['status'] == 'completed':
                        return task['aggregated']
                    elif include_running:
                        # 即使未完成也計算目前的結果
                        return self._aggregate_results(task)
                return None
            else:
                results = {}
                for tid, task in self.tasks.items():
                    if task['status'] == 'completed':
                        results[tid] = task['aggregated']
                    elif include_running and task['results']:
                        # 包含進行中的任務（即時計算）
                        results[tid] = self._aggregate_results(task)
                        results[tid]['status'] = 'running'
                        results[tid]['progress'] = f"{task['completed_parts']}/{task['total_parts']}"
                return results


task_manager = TaskManager()


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


# ==================== API 路由 ====================

@app.route('/api/task/create', methods=['POST'])
def create_task():
    """
    建立新的計算任務
    
    Request Body:
    {
        "task_id": "task_001",
        "total_parts": 4,
        "file_path": "data.csv"
    }
    """
    data = request.get_json()
    task_id = data.get('task_id')
    total_parts = data.get('total_parts', 1)
    file_path = data.get('file_path', 'data.csv')
    
    if not task_id:
        return jsonify({'success': False, 'error': '缺少 task_id'}), 400
    
    task = task_manager.create_task(task_id, total_parts, file_path)
    
    return jsonify({
        'success': True,
        'message': '任務已建立',
        'task': {
            'task_id': task['task_id'],
            'total_parts': task['total_parts'],
            'status': task['status']
        }
    })


@app.route('/api/task/submit', methods=['POST'])
def submit_result():
    """
    提交部分計算結果
    
    Request Body:
    {
        "task_id": "task_001",
        "part_id": 0,
        "result": {
            "male_count": 12345,
            "female_count": 12346,
            "row_count": 24691,
            "duration": 1.5
        }
    }
    """
    data = request.get_json()
    task_id = data.get('task_id')
    part_id = data.get('part_id')
    result = data.get('result', {})
    total_parts = data.get('total_parts')  # 可選，用於自動建立任務
    
    if not task_id or part_id is None:
        return jsonify({'success': False, 'error': '缺少必要參數'}), 400
    
    success, message = task_manager.submit_result(task_id, part_id, result, total_parts)
    
    task = task_manager.get_task(task_id)
    response = {
        'success': success,
        'message': message,
        'completed_parts': task['completed_parts'] if task else 0,
        'total_parts': task['total_parts'] if task else 0,
        'status': task['status'] if task else 'unknown'
    }
    
    # 如果任務完成，附上彙總結果
    if task and task['status'] == 'completed':
        response['aggregated'] = task['aggregated']
    
    return jsonify(response)


@app.route('/api/task/<task_id>', methods=['GET'])
def get_task(task_id: str):
    """取得任務狀態和結果"""
    task = task_manager.get_task(task_id)
    
    if not task:
        return jsonify({'success': False, 'error': '任務不存在'}), 404
    
    response = {
        'success': True,
        'task': {
            'task_id': task['task_id'],
            'file_path': task['file_path'],
            'total_parts': task['total_parts'],
            'completed_parts': task['completed_parts'],
            'status': task['status'],
            'start_time': task['start_time'],
            'end_time': task['end_time']
        }
    }
    
    if task['status'] == 'completed' and task['aggregated']:
        response['result'] = task['aggregated']
    
    return jsonify(response)


@app.route('/api/tasks', methods=['GET'])
def get_all_tasks():
    """取得所有任務列表"""
    tasks = task_manager.get_all_tasks()
    
    task_list = []
    for task_id, task in tasks.items():
        task_list.append({
            'task_id': task['task_id'],
            'status': task['status'],
            'progress': f"{task['completed_parts']}/{task['total_parts']}"
        })
    
    return jsonify({
        'success': True,
        'tasks': task_list
    })


@app.route('/api/health', methods=['GET'])
def health_check():
    """健康檢查"""
    return jsonify({
        'success': True,
        'status': 'healthy',
        'timestamp': time.time()
    })


@app.route('/api/task/<task_id>', methods=['DELETE'])
def delete_task(task_id: str):
    """刪除任務"""
    success = task_manager.delete_task(task_id)
    if success:
        return jsonify({'success': True, 'message': f'任務 {task_id} 已刪除'})
    else:
        return jsonify({'success': False, 'error': '任務不存在'}), 404


@app.route('/api/results', methods=['GET'])
def get_all_results():
    """取得所有任務的結果（包含進行中的）"""
    include_running = request.args.get('all', 'false').lower() == 'true'
    results = task_manager.export_results(include_running=include_running)
    return jsonify({
        'success': True,
        'count': len(results),
        'results': results
    })


@app.route('/api/results/<task_id>', methods=['GET'])
def get_task_result(task_id: str):
    """取得特定任務的結果"""
    include_running = request.args.get('all', 'false').lower() == 'true'
    result = task_manager.export_results(task_id, include_running=include_running)
    if result:
        return jsonify({
            'success': True,
            'task_id': task_id,
            'result': result
        })
    else:
        return jsonify({'success': False, 'error': '任務不存在或尚未有結果'}), 404


# ==================== 主程式 ====================

if __name__ == '__main__':
    import argparse
    
    parser = argparse.ArgumentParser(description="分散式性別計算 API 伺服器")
    parser.add_argument(
        "-p", "--port",
        type=int,
        default=5000,
        help="伺服器埠號 (預設: 5000)"
    )
    parser.add_argument(
        "--host",
        default="0.0.0.0",
        help="伺服器主機 (預設: 0.0.0.0)"
    )
    parser.add_argument(
        "--debug",
        action="store_true",
        help="啟用除錯模式"
    )
    
    args = parser.parse_args()
    
    print("=" * 60)
    print("分散式性別計算系統 - API 伺服器")
    print("=" * 60)
    print(f"伺服器位址: http://{args.host}:{args.port}")
    print(f"資料儲存: {TASKS_FILE}")
    print()
    print("API 端點:")
    print("  POST   /api/task/create  - 建立新任務")
    print("  POST   /api/task/submit  - 提交計算結果")
    print("  GET    /api/task/<id>    - 取得任務狀態")
    print("  DELETE /api/task/<id>    - 刪除任務")
    print("  GET    /api/tasks        - 取得所有任務")
    print("  GET    /api/results      - 取得所有結果")
    print("  GET    /api/results/<id> - 取得特定結果")
    print("  GET    /api/health       - 健康檢查")
    print("=" * 60)
    
    app.run(host=args.host, port=args.port, debug=args.debug, threaded=True)
