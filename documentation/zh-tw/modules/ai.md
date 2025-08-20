# AI Module 模組文檔

## 📋 概述

AI Module 是 HiveMind 分散式計算平台的人工智慧模組，專門用於分散式機器學習模型訓練、推理和模型管理。目前處於開發階段（30% 完成）。

## 🏗️ 系統架構

```
┌─────────────────────┐
│    AI Module        │
├─────────────────────┤
│ • Model Manager     │
│ • Training Engine   │
│ • Inference Server  │
│ • Model Registry    │
│ • Data Pipeline     │
└─────────────────────┘
        │
        ├─ Deep Learning Frameworks
        ├─ Model Storage
        ├─ Training Data
        └─ Distributed Computing
```

## 🔧 核心組件

### 1. Main AI Controller (`main.py`)
- **功能**: AI 模組主控制器
- **狀態**: 部分實現
- **整合**: 與 Node Pool 和任務系統整合

### 2. Model Identification (`Identification.py`)
- **功能**: 模型識別和分類系統
- **狀態**: 基礎實現完成
- **用途**: 自動模型類型檢測

### 3. Breakdown Service (`breakdown.py`)
- **功能**: 模型和任務分解服務
- **狀態**: 開發中
- **目標**: 將大型 AI 任務分解為可分散執行的子任務

### 4. Q-Learning Table (`q_table.pkl`)
- **功能**: 強化學習 Q 表存儲
- **狀態**: 訓練數據已產生
- **用途**: 模型選擇和優化決策

## 🗂️ 文件結構

```
ai/
├── main.py                     # 主要 AI 控制器
├── Identification.py           # 模型識別系統
├── breakdown.py               # 任務分解服務
├── q_table.pkl               # Q-Learning 決策表
├── __pycache__/              # Python 編譯快取
│   └── Identification.cpython-312.pyc
└── DdeepseekDeepSeek-V3inferenceoutput/  # 推理輸出目錄
```

## 🤖 支援的 AI 框架

### 深度學習框架
```python
SUPPORTED_FRAMEWORKS = {
    'tensorflow': {
        'version': '2.x',
        'distributed': 'tf.distribute.Strategy',
        'formats': ['SavedModel', 'HDF5', 'Checkpoint']
    },
    'pytorch': {
        'version': '1.x, 2.x',
        'distributed': 'torch.nn.parallel.DistributedDataParallel',
        'formats': ['pt', 'pth', 'onnx']
    },
    'huggingface': {
        'version': 'transformers 4.x',
        'distributed': 'accelerate',
        'formats': ['transformers', 'onnx', 'flax']
    },
    'scikit-learn': {
        'version': '1.x',
        'distributed': 'joblib',
        'formats': ['pickle', 'joblib']
    }
}
```

### 模型類型支援
```python
MODEL_TYPES = {
    'nlp': {
        'subtypes': ['classification', 'generation', 'translation', 'qa'],
        'architectures': ['transformer', 'rnn', 'cnn'],
        'examples': ['BERT', 'GPT', 'T5', 'LSTM']
    },
    'computer_vision': {
        'subtypes': ['classification', 'detection', 'segmentation', 'generation'],
        'architectures': ['cnn', 'transformer', 'gan'],
        'examples': ['ResNet', 'YOLOv8', 'Stable Diffusion', 'ViT']
    },
    'tabular': {
        'subtypes': ['regression', 'classification', 'clustering'],
        'architectures': ['tree_based', 'neural_network', 'ensemble'],
        'examples': ['XGBoost', 'Random Forest', 'TabNet']
    },
    'reinforcement_learning': {
        'subtypes': ['q_learning', 'policy_gradient', 'actor_critic'],
        'architectures': ['dqn', 'ppo', 'sac'],
        'examples': ['DQN', 'PPO', 'A3C']
    }
}
```

## 📊 模型識別系統

### 自動模型檢測
```python
# Identification.py 實現範例
import torch
import tensorflow as tf
from transformers import AutoModel
import pickle
import joblib

class ModelIdentifier:
    def __init__(self):
        self.supported_formats = {
            '.pt': self._identify_pytorch,
            '.pth': self._identify_pytorch,
            '.h5': self._identify_tensorflow,
            '.pb': self._identify_tensorflow,
            '.pkl': self._identify_sklearn,
            '.joblib': self._identify_sklearn,
            '.onnx': self._identify_onnx
        }
    
    def identify_model(self, model_path):
        """識別模型類型和架構"""
        file_extension = self._get_file_extension(model_path)
        
        if file_extension in self.supported_formats:
            return self.supported_formats[file_extension](model_path)
        else:
            return self._identify_by_directory(model_path)
    
    def _identify_pytorch(self, model_path):
        """識別 PyTorch 模型"""
        try:
            model = torch.load(model_path, map_location='cpu')
            
            model_info = {
                'framework': 'pytorch',
                'type': self._analyze_pytorch_architecture(model),
                'parameters': self._count_parameters(model),
                'input_shape': self._infer_input_shape(model),
                'model_size': self._get_file_size(model_path)
            }
            
            return model_info
        except Exception as e:
            return {'error': str(e)}
    
    def _identify_tensorflow(self, model_path):
        """識別 TensorFlow 模型"""
        try:
            if model_path.endswith('.h5'):
                model = tf.keras.models.load_model(model_path)
            else:
                model = tf.saved_model.load(model_path)
            
            model_info = {
                'framework': 'tensorflow',
                'type': self._analyze_tensorflow_architecture(model),
                'parameters': model.count_params() if hasattr(model, 'count_params') else None,
                'input_shape': self._get_tensorflow_input_shape(model),
                'model_size': self._get_file_size(model_path)
            }
            
            return model_info
        except Exception as e:
            return {'error': str(e)}
    
    def _analyze_pytorch_architecture(self, model):
        """分析 PyTorch 模型架構"""
        if hasattr(model, 'named_modules'):
            modules = list(model.named_modules())
            
            # 檢測常見架構模式
            if any('transformer' in name.lower() for name, _ in modules):
                return 'transformer'
            elif any('resnet' in name.lower() for name, _ in modules):
                return 'resnet'
            elif any('lstm' in name.lower() or 'gru' in name.lower() for name, _ in modules):
                return 'rnn'
            elif any('conv' in name.lower() for name, _ in modules):
                return 'cnn'
            else:
                return 'neural_network'
        
        return 'unknown'
```

### 智能模型分析
```python
class ModelAnalyzer:
    def __init__(self):
        self.complexity_thresholds = {
            'small': 1e6,      # < 1M parameters
            'medium': 1e8,     # 1M - 100M parameters
            'large': 1e9,      # 100M - 1B parameters
            'xlarge': float('inf')  # > 1B parameters
        }
    
    def analyze_computational_requirements(self, model_info):
        """分析模型計算需求"""
        parameter_count = model_info.get('parameters', 0)
        model_type = model_info.get('type', 'unknown')
        
        # 根據參數數量估算複雜度
        complexity = self._estimate_complexity(parameter_count)
        
        # 根據模型類型調整需求
        requirements = self._get_base_requirements(complexity)
        
        if model_type == 'transformer':
            requirements['memory_gb'] *= 1.5  # Transformer 需要更多記憶體
            requirements['gpu_memory_gb'] = max(requirements.get('gpu_memory_gb', 0), 8)
        elif model_type == 'cnn':
            requirements['gpu_memory_gb'] = max(requirements.get('gpu_memory_gb', 0), 4)
        
        return requirements
    
    def _estimate_complexity(self, parameter_count):
        """估算模型複雜度級別"""
        for level, threshold in self.complexity_thresholds.items():
            if parameter_count < threshold:
                return level
        return 'xlarge'
    
    def _get_base_requirements(self, complexity):
        """獲取基礎資源需求"""
        requirements_map = {
            'small': {
                'cpu_cores': 2,
                'memory_gb': 4,
                'gpu_memory_gb': 2,
                'estimated_time_hours': 0.5
            },
            'medium': {
                'cpu_cores': 4,
                'memory_gb': 16,
                'gpu_memory_gb': 8,
                'estimated_time_hours': 2
            },
            'large': {
                'cpu_cores': 8,
                'memory_gb': 32,
                'gpu_memory_gb': 16,
                'estimated_time_hours': 8
            },
            'xlarge': {
                'cpu_cores': 16,
                'memory_gb': 64,
                'gpu_memory_gb': 32,
                'estimated_time_hours': 24
            }
        }
        
        return requirements_map.get(complexity, requirements_map['small'])
```

## 🔄 分散式訓練系統

### 任務分解策略
```python
# breakdown.py 實現規劃
class AITaskBreakdown:
    def __init__(self):
        self.breakdown_strategies = {
            'data_parallel': self._data_parallel_breakdown,
            'model_parallel': self._model_parallel_breakdown,
            'pipeline_parallel': self._pipeline_parallel_breakdown,
            'federated': self._federated_breakdown
        }
    
    def breakdown_training_task(self, task):
        """分解 AI 訓練任務"""
        model_info = task['model_info']
        dataset_info = task['dataset_info']
        training_config = task['training_config']
        
        # 選擇分解策略
        strategy = self._select_breakdown_strategy(model_info, dataset_info)
        
        # 執行分解
        subtasks = self.breakdown_strategies[strategy](task)
        
        return {
            'strategy': strategy,
            'subtasks': subtasks,
            'coordination_plan': self._create_coordination_plan(subtasks)
        }
    
    def _data_parallel_breakdown(self, task):
        """數據並行分解"""
        dataset_size = task['dataset_info']['size']
        available_nodes = task['available_nodes']
        
        # 計算每個節點的數據分片
        data_per_node = dataset_size // len(available_nodes)
        
        subtasks = []
        for i, node in enumerate(available_nodes):
            subtask = {
                'type': 'data_parallel_training',
                'node_id': node['id'],
                'data_range': {
                    'start': i * data_per_node,
                    'end': (i + 1) * data_per_node if i < len(available_nodes) - 1 else dataset_size
                },
                'model_config': task['model_info'],
                'training_config': task['training_config'],
                'synchronization': 'gradient_averaging'
            }
            subtasks.append(subtask)
        
        return subtasks
    
    def _model_parallel_breakdown(self, task):
        """模型並行分解"""
        model_layers = task['model_info']['layers']
        available_nodes = task['available_nodes']
        
        # 將模型層分配給不同節點
        layers_per_node = len(model_layers) // len(available_nodes)
        
        subtasks = []
        for i, node in enumerate(available_nodes):
            layer_start = i * layers_per_node
            layer_end = (i + 1) * layers_per_node if i < len(available_nodes) - 1 else len(model_layers)
            
            subtask = {
                'type': 'model_parallel_training',
                'node_id': node['id'],
                'layer_range': {
                    'start': layer_start,
                    'end': layer_end,
                    'layers': model_layers[layer_start:layer_end]
                },
                'input_shape': self._calculate_input_shape(layer_start, model_layers),
                'output_shape': self._calculate_output_shape(layer_end - 1, model_layers),
                'pipeline_stage': i
            }
            subtasks.append(subtask)
        
        return subtasks
```

### 聯邦學習支援
```python
class FederatedLearningManager:
    def __init__(self):
        self.aggregation_methods = {
            'fedavg': self._federated_averaging,
            'fedprox': self._federated_proximal,
            'scaffold': self._scaffold_aggregation
        }
    
    def coordinate_federated_training(self, participants, global_model, rounds=10):
        """協調聯邦學習訓練"""
        global_weights = global_model.get_weights()
        
        for round_num in range(rounds):
            print(f"聯邦學習輪次 {round_num + 1}/{rounds}")
            
            # 選擇參與者
            selected_participants = self._select_participants(participants)
            
            # 分發全局模型
            local_updates = []
            for participant in selected_participants:
                local_weights = self._train_local_model(
                    participant, global_weights
                )
                local_updates.append({
                    'participant_id': participant['id'],
                    'weights': local_weights,
                    'sample_count': participant['sample_count']
                })
            
            # 聚合更新
            global_weights = self._aggregate_updates(local_updates)
            global_model.set_weights(global_weights)
            
            # 評估全局模型
            metrics = self._evaluate_global_model(global_model)
            print(f"輪次 {round_num + 1} 指標: {metrics}")
        
        return global_model
    
    def _federated_averaging(self, local_updates):
        """聯邦平均聚合"""
        total_samples = sum(update['sample_count'] for update in local_updates)
        
        # 加權平均
        aggregated_weights = []
        for layer_idx in range(len(local_updates[0]['weights'])):
            layer_weights = []
            
            for update in local_updates:
                weight = update['weights'][layer_idx]
                sample_ratio = update['sample_count'] / total_samples
                layer_weights.append(weight * sample_ratio)
            
            aggregated_layer = sum(layer_weights)
            aggregated_weights.append(aggregated_layer)
        
        return aggregated_weights
```

## 🧠 強化學習系統

### Q-Learning 決策系統
```python
# Q-Table 管理和使用
import pickle
import numpy as np

class QLearningDecisionMaker:
    def __init__(self, q_table_path='q_table.pkl'):
        self.q_table_path = q_table_path
        self.q_table = self._load_q_table()
        
        # 狀態和動作定義
        self.state_features = [
            'model_complexity',    # 0: small, 1: medium, 2: large, 3: xlarge
            'dataset_size',       # 0: small, 1: medium, 2: large
            'available_nodes',    # 0: 1-2, 1: 3-5, 2: 6-10, 3: 10+
            'network_latency'     # 0: low, 1: medium, 2: high
        ]
        
        self.actions = [
            'data_parallel',      # 數據並行
            'model_parallel',     # 模型並行
            'pipeline_parallel',  # 流水線並行
            'federated',         # 聯邦學習
            'single_node'        # 單節點執行
        ]
    
    def _load_q_table(self):
        """載入 Q 表"""
        try:
            with open(self.q_table_path, 'rb') as f:
                return pickle.load(f)
        except FileNotFoundError:
            # 初始化新的 Q 表
            return np.zeros((4, 3, 4, 3, 5))  # state_space x action_space
    
    def select_training_strategy(self, model_info, dataset_info, node_info):
        """選擇最佳訓練策略"""
        state = self._encode_state(model_info, dataset_info, node_info)
        
        # ε-greedy 策略選擇
        if np.random.random() < 0.1:  # 10% 探索
            action_idx = np.random.choice(len(self.actions))
        else:  # 90% 利用
            action_idx = np.argmax(self.q_table[state])
        
        return self.actions[action_idx]
    
    def update_q_table(self, state, action, reward, next_state):
        """更新 Q 表"""
        alpha = 0.1  # 學習率
        gamma = 0.9  # 折扣因子
        
        action_idx = self.actions.index(action)
        
        # Q-learning 更新公式
        current_q = self.q_table[state][action_idx]
        max_next_q = np.max(self.q_table[next_state])
        
        new_q = current_q + alpha * (reward + gamma * max_next_q - current_q)
        self.q_table[state][action_idx] = new_q
        
        # 保存更新的 Q 表
        self._save_q_table()
    
    def _encode_state(self, model_info, dataset_info, node_info):
        """編碼狀態為 Q 表索引"""
        # 模型複雜度
        param_count = model_info.get('parameters', 0)
        if param_count < 1e6:
            complexity = 0
        elif param_count < 1e8:
            complexity = 1
        elif param_count < 1e9:
            complexity = 2
        else:
            complexity = 3
        
        # 數據集大小
        dataset_size = dataset_info.get('size', 0)
        if dataset_size < 10000:
            size = 0
        elif dataset_size < 100000:
            size = 1
        else:
            size = 2
        
        # 可用節點數
        node_count = len(node_info.get('available_nodes', []))
        if node_count <= 2:
            nodes = 0
        elif node_count <= 5:
            nodes = 1
        elif node_count <= 10:
            nodes = 2
        else:
            nodes = 3
        
        # 網路延遲（簡化）
        latency = 1  # 預設中等延遲
        
        return (complexity, size, nodes, latency)
    
    def _save_q_table(self):
        """保存 Q 表"""
        with open(self.q_table_path, 'wb') as f:
            pickle.dump(self.q_table, f)
```

## 🔬 模型推理服務

### 分散式推理引擎
```python
class DistributedInferenceEngine:
    def __init__(self):
        self.model_registry = {}
        self.inference_nodes = []
        
    def register_model(self, model_id, model_info, model_path):
        """註冊模型到推理服務"""
        self.model_registry[model_id] = {
            'info': model_info,
            'path': model_path,
            'loaded_nodes': [],
            'request_count': 0,
            'avg_latency': 0
        }
    
    def distribute_model(self, model_id, target_nodes):
        """將模型分發到推理節點"""
        model_info = self.model_registry[model_id]
        
        for node in target_nodes:
            success = self._deploy_model_to_node(model_id, model_info, node)
            if success:
                model_info['loaded_nodes'].append(node['id'])
    
    def inference_request(self, model_id, input_data):
        """處理推理請求"""
        model_info = self.model_registry.get(model_id)
        if not model_info:
            raise ValueError(f"模型 {model_id} 未註冊")
        
        # 選擇最佳推理節點
        best_node = self._select_inference_node(model_info['loaded_nodes'])
        
        # 發送推理請求
        result = self._send_inference_request(best_node, model_id, input_data)
        
        # 更新統計
        self._update_inference_stats(model_id, result['latency'])
        
        return result
    
    def _select_inference_node(self, available_nodes):
        """選擇最佳推理節點"""
        # 簡單的負載均衡：選擇負載最低的節點
        node_loads = {}
        for node_id in available_nodes:
            node_loads[node_id] = self._get_node_load(node_id)
        
        return min(node_loads, key=node_loads.get)
```

## 📊 監控和指標

### AI 模組指標收集
```python
class AIMetricsCollector:
    def __init__(self):
        self.metrics = {
            'training_metrics': {},
            'inference_metrics': {},
            'resource_metrics': {},
            'model_metrics': {}
        }
    
    def collect_training_metrics(self, task_id, metrics):
        """收集訓練指標"""
        self.metrics['training_metrics'][task_id] = {
            'accuracy': metrics.get('accuracy'),
            'loss': metrics.get('loss'),
            'epoch': metrics.get('epoch'),
            'training_time': metrics.get('training_time'),
            'convergence_rate': metrics.get('convergence_rate'),
            'timestamp': time.time()
        }
    
    def collect_inference_metrics(self, model_id, request_metrics):
        """收集推理指標"""
        if model_id not in self.metrics['inference_metrics']:
            self.metrics['inference_metrics'][model_id] = []
        
        self.metrics['inference_metrics'][model_id].append({
            'latency': request_metrics['latency'],
            'throughput': request_metrics['throughput'],
            'accuracy': request_metrics.get('accuracy'),
            'input_size': request_metrics['input_size'],
            'timestamp': time.time()
        })
    
    def get_model_performance_summary(self, model_id):
        """獲取模型性能摘要"""
        inference_data = self.metrics['inference_metrics'].get(model_id, [])
        
        if not inference_data:
            return None
        
        latencies = [record['latency'] for record in inference_data]
        throughputs = [record['throughput'] for record in inference_data]
        
        return {
            'avg_latency': np.mean(latencies),
            'p95_latency': np.percentile(latencies, 95),
            'avg_throughput': np.mean(throughputs),
            'total_requests': len(inference_data),
            'success_rate': 100.0  # 簡化
        }
```

## 🔧 開發狀態和路線圖

### 當前實現狀態 (30%)
```python
IMPLEMENTATION_STATUS = {
    'model_identification': {
        'status': 'completed',
        'completion': 90,
        'features': ['pytorch_support', 'tensorflow_support', 'basic_analysis']
    },
    'task_breakdown': {
        'status': 'in_progress',
        'completion': 40,
        'features': ['strategy_selection', 'basic_decomposition']
    },
    'q_learning': {
        'status': 'in_progress',
        'completion': 60,
        'features': ['q_table_management', 'strategy_selection']
    },
    'distributed_training': {
        'status': 'planned',
        'completion': 10,
        'features': ['architecture_design']
    },
    'inference_engine': {
        'status': 'planned',
        'completion': 5,
        'features': ['basic_design']
    },
    'federated_learning': {
        'status': 'planned',
        'completion': 0,
        'features': []
    }
}
```

### 開發路線圖
```python
ROADMAP = {
    'phase_1': {
        'target_completion': 50,
        'timeline': '1-2 months',
        'goals': [
            '完成任務分解系統',
            '實現基礎分散式訓練',
            '整合 Q-Learning 決策系統',
            '建立模型註冊表'
        ]
    },
    'phase_2': {
        'target_completion': 75,
        'timeline': '2-3 months',
        'goals': [
            '實現聯邦學習框架',
            '建立推理服務引擎',
            '添加模型版本管理',
            '實現自動超參數調優'
        ]
    },
    'phase_3': {
        'target_completion': 100,
        'timeline': '3-4 months',
        'goals': [
            '完善監控和指標系統',
            '添加模型安全和隱私保護',
            '實現自動模型優化',
            '建立完整的 AI 工作流程'
        ]
    }
}
```

## 🔧 常見問題排除

### 1. 模型載入失敗
**問題**: 無法載入或識別模型文件
**解決**:
```python
# 添加異常處理和格式檢測
def safe_model_load(model_path):
    try:
        # 檢查文件是否存在
        if not os.path.exists(model_path):
            raise FileNotFoundError(f"模型文件不存在: {model_path}")
        
        # 檢查文件大小
        file_size = os.path.getsize(model_path)
        if file_size == 0:
            raise ValueError("模型文件為空")
        
        # 根據副檔名載入
        return load_model_by_format(model_path)
        
    except Exception as e:
        logger.error(f"模型載入失敗: {e}")
        return None
```

### 2. 記憶體不足
**問題**: 大型模型導致記憶體溢出
**解決**:
```python
# 實現模型分片和記憶體管理
def load_model_with_memory_check(model_path, max_memory_gb=8):
    model_size = estimate_model_memory_usage(model_path)
    
    if model_size > max_memory_gb * 1024**3:  # 轉換為 bytes
        # 使用模型分片
        return load_model_with_sharding(model_path)
    else:
        return standard_model_load(model_path)
```

### 3. 分散式同步問題
**問題**: 多節點訓練同步失敗
**解決**:
```python
# 實現重試機制和同步檢查點
def synchronize_training_nodes(nodes, max_retries=3):
    for attempt in range(max_retries):
        try:
            sync_results = []
            for node in nodes:
                result = node.synchronize()
                sync_results.append(result)
            
            if all(result['success'] for result in sync_results):
                return True
                
        except Exception as e:
            logger.warning(f"同步嘗試 {attempt + 1} 失敗: {e}")
            time.sleep(2 ** attempt)  # 指數退避
    
    return False
```

---

**相關文檔**:
- [TaskWorker 模組](taskworker.md)
- [Node Pool 模組](node-pool.md)
- [API 文檔](../api.md)
- [開發指南](../developer.md)
