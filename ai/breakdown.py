import os
import ast
import subprocess
import psutil
import numpy as np
import pickle
import logging
import shutil
import time
import signal
import torch
import random
import string
from pathlib import Path

# 精簡日誌
logging.basicConfig(
    filename='breakdown.log',
    level=logging.INFO,
    format='%(asctime)s - %(message)s'
)
logger = logging.getLogger(__name__)
logger.addHandler(logging.StreamHandler())

CONFIG = {
    'learning_rate': 0.1,
    'discount_factor': 0.95,
    'episodes_per_cycle': 5,
    'max_steps': 5,
    'timeout_seconds': 10,
    'q_table_file': "/mnt/d/hivemind/ai/q_table.pkl",
    'entry_point': "main.py"
}

# GPU 初始化
GPU_AVAILABLE = False
try:
    import pynvml
    pynvml.nvmlInit()
    GPU_COUNT = pynvml.nvmlDeviceGetCount()
    if GPU_COUNT > 0:
        GPU_AVAILABLE = True
        try:
            logger.info(f"檢測到 GPU: {pynvml.nvmlDeviceGetName(0).decode()}")
        except pynvml.NVMLError as e:
            logger.warning(f"無法獲取 GPU 名稱: {e}")
    else:
        logger.warning("未檢測到 GPU")
except ImportError:
    logger.error("未安裝 pynvml")
except pynvml.NVMLError as e:
    logger.error(f"GPU 設置失敗: {e}")

# 信號處理器
stop_training = False
def signal_handler(sig, frame):
    global stop_training
    logger.info("正在停止訓練...")
    stop_training = True
signal.signal(signal.SIGINT, signal_handler)

class CodeAnalyzer(ast.NodeVisitor):
    def __init__(self):
        self.has_input = False
        self.has_gpu_ops = False
        
    def visit_Import(self, node):
        for alias in node.names:
            if alias.name in {'torch', 'cuda', 'triton'}:
                self.has_gpu_ops = True
        self.generic_visit(node)
    
    def visit_ImportFrom(self, node):
        if node.module in {'torch', 'cuda', 'triton'}:
            self.has_gpu_ops = True
        self.generic_visit(node)
    
    def visit_Call(self, node):
        if isinstance(node.func, ast.Name) and node.func.id == "input":
            self.has_input = True
        elif (isinstance(node.func, ast.Attribute) and
              isinstance(node.func.value, ast.Name) and
              node.func.value.id == "sys" and
              node.func.attr in {"read", "readline", "readlines"}):
            self.has_input = True
        elif (isinstance(node.func, ast.Attribute) and
              node.func.attr in {'to', 'cuda', 'device'} and
              isinstance(node.func.value, ast.Name) and
              node.func.value.id == 'torch'):
            self.has_gpu_ops = True
        self.generic_visit(node)

def analyze_code(file_path):
    try:
        with open(file_path, 'r', encoding='utf-8') as f:
            tree = ast.parse(f.read())
        analyzer = CodeAnalyzer()
        analyzer.visit(tree)
        return analyzer.has_input, analyzer.has_gpu_ops
    except Exception as e:
        logger.warning(f"解析錯誤: {file_path}: {e}")
        return True, False

def split_code_into_tasks(project_path):
    project_path = Path(project_path)
    tasks = []
    for file_path in project_path.rglob('*.py'):
        has_input, has_gpu_ops = analyze_code(file_path)
        if not has_input:
            tasks.append({'path': str(file_path), 'requires_gpu': has_gpu_ops})
            logger.info(f"添加任務: {file_path} (GPU: {has_gpu_ops})")
        else:
            logger.info(f"跳過: {file_path} (包含輸入)")
    return tasks

def random_input():
    return ''.join(random.choice(string.ascii_letters + string.digits) for _ in range(10))

def create_task_folder(task_files, task_id):
    task_dir = Path(f"task_{task_id}")
    task_dir.mkdir(exist_ok=True, parents=True)
    
    for task in task_files:
        file_path = Path(task['path'])
        shutil.copy2(file_path, task_dir / file_path.name)
    
    with open(task_dir / "main.py", "w") as f:
        f.write("import os, sys, torch, builtins\n")
        f.write(f"builtins.input = lambda *args, **kwargs: '{random_input()}'\n")
        f.write("os.environ['CUDA_VISIBLE_DEVICES'] = '0'\n")
        f.write("device = torch.device('cuda' if torch.cuda.is_available() else 'cpu')\n")
        f.write("print(f'使用設備: {device}')\n")
        f.write("if torch.cuda.is_available():\n")
        f.write("    x = torch.randn(10000, 10000, device=device)\n")
        f.write("    for _ in range(50): x = torch.matmul(x, x.t())\n")
        f.write("    print(f'GPU 操作完成')\n")
        for task in task_files:
            module_name = Path(task['path']).stem
            f.write(f"try:\n")
            f.write(f"    from {module_name} import *\n")
            f.write(f"except Exception as e:\n")
            f.write(f"    print(f'導入 {module_name} 失敗: {{e}}')\n")
    
    return str(task_dir)

def run_task(task_dir, requires_gpu):
    def kill_process_tree(pid):
        try:
            process = psutil.Process(pid)
            for child in process.children(recursive=True):
                child.kill()
            process.kill()
        except psutil.NoSuchProcess:
            pass

    cmd = ["python", CONFIG['entry_point']]
    start_time = time.perf_counter()
    result = {
        "success": False,
        "duration": 0,
        "cpu": 0,
        "memory": 0,
        "gpu_util": 0,
        "gpu_memory": 0,
        "error": "",
        "output": ""
    }
    
    try:
        with subprocess.Popen(
            cmd,
            cwd=task_dir,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            start_new_session=True,
            env={**os.environ, 'CUDA_VISIBLE_DEVICES': '0'}
        ) as proc:
            try:
                stdout, stderr = proc.communicate(timeout=CONFIG['timeout_seconds'])
                result["output"] = stdout
                result["error"] = stderr
                result["success"] = proc.returncode == 0
            except subprocess.TimeoutExpired:
                logger.warning(f"任務 {task_dir} 超時")
                kill_process_tree(proc.pid)
                result["error"] = "超時"
    
    except Exception as e:
        logger.error(f"任務 {task_dir} 失敗: {e}")
        result["error"] = str(e)
    
    result["duration"] = time.perf_counter() - start_time
    result["cpu"] = psutil.cpu_percent(interval=0.1)
    result["memory"] = psutil.virtual_memory().percent
    
    if GPU_AVAILABLE and requires_gpu:
        try:
            import pynvml
            handle = pynvml.nvmlDeviceGetHandleByIndex(0)
            result["gpu_util"] = pynvml.nvmlDeviceGetUtilizationRates(handle).gpu
            result["gpu_memory"] = pynvml.nvmlDeviceGetMemoryInfo(handle).used / 1024 / 1024
        except pynvml.NVMLError as e:
            logger.error(f"GPU 監控失敗: {task_dir}: {e}")
            result["gpu_util"] = 0
            result["gpu_memory"] = 0
    
    logger.info(f"任務 {task_dir}: 成功={result['success']}, GPU 利用率={result['gpu_util']}%, GPU 記憶體={result['gpu_memory']}MB")
    return result

class QLearningAgent:
    def __init__(self, num_tasks):
        self.num_tasks = num_tasks
        self.num_actions = min(5, num_tasks)
        if os.path.exists(CONFIG['q_table_file']):
            with open(CONFIG['q_table_file'], 'rb') as f:
                self.q_table = pickle.load(f)
        else:
            self.q_table = np.random.uniform(low=-0.1, high=0.1, size=(num_tasks, self.num_actions))
        logger.info(f"Q 表形狀: {self.q_table.shape}")

    def get_action(self, state, epsilon):
        if np.random.random() < epsilon:
            return np.random.randint(0, self.num_actions)
        return np.argmax(self.q_table[state])

    def update_q_table(self, state, action, reward, next_state):
        if next_state >= self.num_tasks:
            td_target = reward
        else:
            td_target = reward + CONFIG['discount_factor'] * np.max(self.q_table[next_state])
        self.q_table[state, action] += CONFIG['learning_rate'] * (td_target - self.q_table[state, action])

    def save_q_table(self):
        with open(CONFIG['q_table_file'] + ".tmp", 'wb') as f:
            pickle.dump(self.q_table, f)
        os.replace(CONFIG['q_table_file'] + ".tmp", CONFIG['q_table_file'])
        logger.info("Q 表已儲存")

def train_agent(project_path):
    tasks = split_code_into_tasks(project_path)
    if not tasks:
        logger.error("未找到有效任務")
        return

    agent = QLearningAgent(len(tasks))
    epsilon = 1.0
    epsilon_decay = 0.995
    min_epsilon = 0.01
    cycle_count = 0

    logger.info(f"PyTorch: {torch.__version__}, CUDA: {torch.cuda.is_available()}")
    if torch.cuda.is_available():
        logger.info(f"設備: {torch.cuda.get_device_name(0)}, CUDA: {torch.version.cuda}")

    while not stop_training:
        cycle_count += 1
        logger.info(f"週期 {cycle_count}, epsilon={epsilon:.2f}")
        
        for episode in range(CONFIG['episodes_per_cycle']):
            state = 0
            total_reward = 0
            step = 0
            
            while state < len(tasks) and step < CONFIG['max_steps']:
                action = agent.get_action(state, epsilon)
                group_size = min(action + 1, len(tasks) - state)
                task_group = tasks[state:state + group_size]
                
                task_dir = create_task_folder(task_group, f"c{cycle_count}e{episode}s{step}")
                requires_gpu = any(task['requires_gpu'] for task in task_group)
                result = run_task(task_dir, requires_gpu)
                
                reward = 10 if result["success"] else -10
                reward -= result["duration"] / CONFIG['timeout_seconds']
                reward -= (result["cpu"] + result["memory"]) / 200
                if requires_gpu and GPU_AVAILABLE:
                    reward += result["gpu_util"] / 50
                    if result["gpu_util"] < 10:
                        reward -= 10
                
                total_reward += reward
                agent.update_q_table(state, action, reward, state + group_size)
                
                state += group_size
                step += 1
            
            logger.info(f"週期 {cycle_count}, 回合 {episode}, 總獎勵: {total_reward:.2f}")
        
        epsilon = max(min_epsilon, epsilon * epsilon_decay)
        agent.save_q_table()

    agent.save_q_table()
    logger.info("訓練已停止")

if __name__ == "__main__":
    project_path = "/mnt/d/deepseek/DeepSeek-V3/inference/"
    logger.info(f"開始訓練，項目路徑: {project_path}")
    train_agent(project_path)