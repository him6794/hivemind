# HiveMind AI 模組

> **Language / 語言選擇**
> 
> - **English**: [README.en.md](README.en.md)
> - **繁體中文**: [README.md](README.md) (本文檔)

## 概述

AI 模組負責將大型人工智慧模型分割為可在分布式環境中執行的小任務。這個模組目前正在開發中，旨在實現 AI 模型的智能拆分和分布式推理。

## 功能特色

### 🤖 模型分割 (Model Breakdown)
- **自動分析**：分析 AI 模型結構和計算需求
- **智能拆分**：將模型分割為獨立的計算單元
- **依賴管理**：處理模型層間的數據依賴關係

### 模型識別 (Model Identification)
- **格式檢測**：支援多種 AI 模型格式 (PyTorch, TensorFlow, ONNX)
- **資源估算**：預估模型運行所需的計算資源
- **兼容性檢查**：驗證模型與目標節點的兼容性

### 智能調度
- **性能預測**：基於模型特徵預測執行時間
- **節點匹配**：為不同模型任務選擇最適合的工作節點
- **負載平衡**：在多個節點間均衡分配 AI 推理任務

## 文件結構

```
ai/
├── breakdown.py        # 模型分割核心邏輯
├── identification.py   # 模型識別和分析
├── main.py            # 主程序入口
├── q_table.pkl        # Q-learning 訓練數據
└── __pycache__/       # Python 編譯緩存
```

## 核心模組

### breakdown.py
負責將大型 AI 模型拆分為可分布式執行的小任務：

```python
class ModelBreakdown:
    """模型分割器"""
    
    def analyze_model(self, model_path: str) -> ModelInfo:
        """分析模型結構和計算需求"""
        pass
    
    def split_model(self, model: Model, max_chunk_size: int) -> List[ModelChunk]:
        """將模型分割為多個塊"""
        pass
    
    def optimize_splitting(self, model: Model, available_nodes: List[Node]) -> SplitPlan:
        """優化分割策略以提高執行效率"""
        pass
```

### identification.py
處理模型識別和兼容性檢查：

```python
class ModelIdentifier:
    """模型識別器"""
    
    def detect_format(self, model_path: str) -> str:
        """檢測模型格式 (pytorch, tensorflow, onnx, etc.)"""
        pass
    
    def estimate_resources(self, model: Model) -> ResourceRequirement:
        """估算模型運行所需資源"""
        pass
    
    def check_compatibility(self, model: Model, node: Node) -> bool:
        """檢查模型與節點的兼容性"""
        pass
```

## 使用方法

### 基本示例

```python
from ai.breakdown import ModelBreakdown
from ai.identification import ModelIdentifier

# 1. 識別模型
identifier = ModelIdentifier()
model_info = identifier.analyze_model("/path/to/model.pth")

print(f"模型格式: {model_info.format}")
print(f"預估記憶體需求: {model_info.memory_mb}MB")
print(f"預估 GPU 需求: {model_info.gpu_memory_mb}MB")

# 2. 分割模型
breakdown = ModelBreakdown()
chunks = breakdown.split_model(model_info, max_chunk_size=500)  # 500MB 每塊

print(f"模型已分割為 {len(chunks)} 個塊")

# 3. 生成分布式執行計畫
execution_plan = breakdown.create_execution_plan(chunks, available_nodes)
```

### 高級用法

```python
# 基於 Q-learning 的智能調度
from ai.scheduler import AIScheduler

scheduler = AIScheduler()
scheduler.load_q_table("q_table.pkl")  # 載入預訓練的 Q 表

# 為特定 AI 任務選擇最佳節點
best_nodes = scheduler.select_optimal_nodes(
    model_requirements=model_info.requirements,
    available_nodes=node_pool.get_available_nodes(),
    performance_target="minimize_latency"  # 或 "maximize_throughput"
)
```

## 支援的模型格式

| 格式 | 支援狀態 | 描述 |
|------|---------|------|
| PyTorch (.pth, .pt) | 完整支援 | PyTorch 模型檔案 |
| TensorFlow (.pb, .h5) | 🚧 開發中 | TensorFlow 模型格式 |
| ONNX (.onnx) | 🚧 開發中 | 開放神經網路交換格式 |
| Hugging Face | 計畫中 | Transformers 模型 |
| Custom Formats | 計畫中 | 自定義模型格式 |

## 配置選項

創建 `ai_config.json` 配置文件：

```json
{
  "model_breakdown": {
    "max_chunk_size_mb": 500,
    "min_chunk_size_mb": 50,
    "overlap_size_mb": 10,
    "compression_enabled": true
  },
  "scheduling": {
    "strategy": "q_learning",
    "learning_rate": 0.1,
    "exploration_rate": 0.1,
    "reward_weights": {
      "execution_time": 0.6,
      "resource_efficiency": 0.3,
      "network_cost": 0.1
    }
  },
  "compatibility": {
    "min_python_version": "3.8",
    "required_packages": ["torch", "numpy", "scipy"],
    "gpu_required": false
  }
}
```

## 開發狀態

### 已完成
- [x] 基礎模型分析框架
- [x] PyTorch 模型格式支援
- [x] 簡單的分割算法
- [x] Q-learning 調度基礎

### 🚧 開發中
- [ ] TensorFlow 模型支援
- [ ] ONNX 格式支援
- [ ] 動態分割優化
- [ ] 多 GPU 並行處理
- [ ] 模型壓縮和量化

### 計畫中
- [ ] Hugging Face Transformers 整合
- [ ] 聯邦學習支援
- [ ] 模型快取機制
- [ ] 實時性能監控
- [ ] 自適應調度算法

## 性能考慮

### 分割策略
- **固定大小分割**：簡單但可能不是最優
- **基於層分割**：保持模型結構完整性
- **動態分割**：根據節點能力調整塊大小

### 通信優化
- **數據壓縮**：減少網路傳輸開銷
- **管道並行**：重疊計算和通信
- **結果快取**：避免重複計算

## 故障排除

### 常見問題

1. **模型載入失敗**
   ```python
   # 檢查模型文件格式
   import torch
   try:
       model = torch.load("model.pth", map_location='cpu')
       print("模型載入成功")
   except Exception as e:
       print(f"載入失敗: {e}")
   ```

2. **記憶體不足**
   ```python
   # 調整分割參數
   breakdown = ModelBreakdown(max_chunk_size=200)  # 減小塊大小
   ```

3. **兼容性問題**
   ```bash
   # 檢查 Python 套件版本
   pip list | grep torch
   pip list | grep numpy
   ```

## 測試

```bash
# 運行 AI 模組
cd ai/
python main.py

**測試狀態**: 當前 AI 模組尚未建立測試框架，這是待開發的重要功能。
```

## 貢獻指南

歡迎為 AI 模組開發做出貢獻！請參考以下指南：

1. **開發環境設置**
   ```bash
   pip install torch torchvision
   pip install tensorflow  # 可選
   pip install onnx onnxruntime  # 可選
   ```

2. **提交代碼前檢查**
   - 確保所有測試通過
   - 添加必要的文檔註釋
   - 遵循 PEP 8 編碼規範

3. **新功能開發**
   - 先創建 issue 討論需求
   - 在 feature 分支開發
   - 提交 Pull Request

## 路線圖

### 短期目標 (3個月)
- 完成 TensorFlow 模型支援
- 實現基於層的智能分割
- 添加模型性能基準測試

### 中期目標 (6個月)
- 整合 Hugging Face Transformers
- 實現聯邦學習基礎框架
- 開發 Web UI 模型管理界面

### 長期目標 (12個月)
- 支援自定義模型格式
- 實現自適應調度算法
- 提供完整的 AI 模型生命週期管理

## 許可證

本模組採用與主項目相同的 GPL v3.0 許可證。

## 聯繫方式

- **項目維護者**: HiveMind AI Team
- **GitHub Issues**: https://github.com/him6794/hivemind/issues
- **技術討論**: ai-dev@hivemind.com
