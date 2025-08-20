# AI Module - Distributed Model Training

> **Language / 語言選擇**
> 
> - **English**: [README.en.md](README.en.md) (This document)
> - **繁體中文**: [README.md](README.md)

[![AI Status](https://img.shields.io/badge/status-development-orange.svg)](https://github.com/him6794/hivemind)
[![Python](https://img.shields.io/badge/python-3.8%2B-blue.svg)](https://www.python.org/)
[![PyTorch](https://img.shields.io/badge/pytorch-supported-red.svg)](https://pytorch.org/)

The AI module provides distributed artificial intelligence model training and inference capabilities, allowing large-scale machine learning tasks to be split and executed across multiple worker nodes in the HiveMind network.

## Overview

The HiveMind AI module implements distributed model training through:

- **🧠 Model Segmentation**: Automatically splits large models into executable subtasks
- **Distributed Training**: Coordinates training across multiple worker nodes
- **Gradient Aggregation**: Efficiently combines gradients from distributed workers
- **🔄 Model Synchronization**: Maintains model consistency across the network
- **Performance Optimization**: Intelligent task scheduling based on node capabilities

## Key Features

### Core Capabilities

#### 1. **Distributed Model Training**
- **Federated Learning**: Privacy-preserving distributed training
- **Data Parallel Training**: Split datasets across multiple nodes
- **Model Parallel Training**: Split large models across multiple GPUs/nodes
- **Gradient Compression**: Reduce communication overhead
- **Fault Tolerance**: Handle node failures gracefully

#### 2. **Model Splitting Engine**
```python
from hivemind.ai import ModelSplitter

# Example: Split a large transformer model
splitter = ModelSplitter()
model_parts = splitter.split_model(
    model=large_transformer,
    num_parts=4,
    strategy='layer_wise'
)

# Distribute parts to worker nodes
for i, part in enumerate(model_parts):
    assign_to_worker(part, worker_nodes[i])
```

#### 3. **Intelligent Task Scheduling**
- **GPU Affinity**: Match tasks to appropriate GPU types
- **Memory Optimization**: Optimize memory usage across nodes
- **Latency Minimization**: Reduce communication latency
- **Load Balancing**: Distribute workload evenly

#### 4. **Model Management**
- **Version Control**: Track model versions and updates
- **Checkpoint Management**: Save and restore training progress
- **Model Registry**: Centralized model storage and retrieval
- **Performance Metrics**: Track training accuracy and loss

## Installation and Setup

### Prerequisites

**System Requirements:**
- **Python**: 3.8 or higher
- **PyTorch**: 1.12.0 or higher
- **CUDA**: 11.3+ (for GPU acceleration)
- **Memory**: 8GB+ RAM recommended
- **Storage**: 20GB+ free space for model caching

**Dependencies:**
```bash
# Core ML libraries
pip install torch torchvision torchaudio
pip install transformers datasets
pip install accelerate deepspeed

# Distributed computing
pip install ray[tune]
pip install horovod

# Monitoring and visualization
pip install tensorboard wandb
pip install matplotlib seaborn
```

### Installation Steps

#### 1. **Install AI Module**
```bash
cd hivemind/ai
pip install -r requirements.txt

# Install additional ML frameworks (optional)
pip install tensorflow keras
pip install jax jaxlib
pip install onnx onnxruntime
```

#### 2. **Configure GPU Support**
```bash
# Check CUDA availability
python -c "import torch; print(torch.cuda.is_available())"

# Install NVIDIA drivers (Linux)
sudo apt install nvidia-driver-470
sudo apt install nvidia-cuda-toolkit

# Verify GPU setup
nvidia-smi
```

#### 3. **Setup Model Repository**
```bash
# Create model storage directory
mkdir -p models/checkpoints
mkdir -p models/datasets
mkdir -p models/artifacts

# Configure HuggingFace cache
export HF_HOME="./models/huggingface"
export TRANSFORMERS_CACHE="./models/transformers"
```

## Usage Examples

### Distributed Training Example

#### 1. **Simple Federated Learning**
```python
import torch
import torch.nn as nn
from hivemind.ai import FederatedTrainer

# Define your model
class SimpleNN(nn.Module):
    def __init__(self):
        super().__init__()
        self.layers = nn.Sequential(
            nn.Linear(784, 128),
            nn.ReLU(),
            nn.Linear(128, 10)
        )
    
    def forward(self, x):
        return self.layers(x)

# Setup federated training
trainer = FederatedTrainer(
    model=SimpleNN(),
    num_workers=4,
    rounds=100,
    local_epochs=5
)

# Start distributed training
trainer.train(
    train_data=mnist_dataset,
    validation_data=mnist_test
)
```

#### 2. **Large Language Model Training**
```python
from transformers import AutoTokenizer, AutoModelForCausalLM
from hivemind.ai import DistributedTrainer

# Load pre-trained model
model_name = "gpt2-medium"
tokenizer = AutoTokenizer.from_pretrained(model_name)
model = AutoModelForCausalLM.from_pretrained(model_name)

# Configure distributed training
trainer = DistributedTrainer(
    model=model,
    tokenizer=tokenizer,
    world_size=8,  # Number of worker nodes
    gradient_accumulation_steps=4,
    mixed_precision=True
)

# Start training
trainer.train(
    dataset="openwebtext",
    batch_size=16,
    learning_rate=5e-5,
    num_epochs=3
)
```

#### 3. **Custom Model Splitting**
```python
from hivemind.ai import ModelSplitter, DistributedOptimizer

# Split a large model
splitter = ModelSplitter()
model_parts = splitter.split_transformer(
    model=large_bert_model,
    num_layers_per_part=4,
    split_attention=True
)

# Distribute across nodes
for i, part in enumerate(model_parts):
    node_id = f"worker-{i:03d}"
    deploy_model_part(part, node_id)

# Setup distributed optimizer
optimizer = DistributedOptimizer(
    model_parts=model_parts,
    lr=1e-4,
    weight_decay=0.01,
    gradient_clipping=1.0
)
```

### Model Inference

#### 1. **Distributed Inference**
```python
from hivemind.ai import DistributedInference

# Setup distributed inference
inference = DistributedInference(
    model_name="hivemind/bert-large-distributed",
    num_workers=4,
    batch_size=32
)

# Run inference on large dataset
results = inference.predict(
    texts=["Hello world", "How are you?"] * 1000,
    return_embeddings=True
)

print(f"Processed {len(results)} samples")
```

#### 2. **Real-time Model Serving**
```python
from hivemind.ai import ModelServer

# Start model server
server = ModelServer(
    model_path="models/checkpoints/my_model.pt",
    port=8080,
    workers=4,
    max_batch_size=16
)

# Start serving
server.start()

# Client usage
import requests
response = requests.post(
    "http://localhost:8080/predict",
    json={"text": "Classify this sentence"}
)
```

## Technical Implementation

### Model Splitting Algorithm

The model splitting engine uses advanced techniques to partition neural networks:

```python
class ModelSplitter:
    """Advanced model splitting for distributed training"""
    
    def split_model(self, model, num_parts, strategy='balanced'):
        """
        Split model into parts for distributed execution
        
        Args:
            model: PyTorch model to split
            num_parts: Number of parts to create
            strategy: Splitting strategy ('balanced', 'layer_wise', 'memory_aware')
        """
        if strategy == 'balanced':
            return self._split_balanced(model, num_parts)
        elif strategy == 'layer_wise':
            return self._split_by_layers(model, num_parts)
        elif strategy == 'memory_aware':
            return self._split_by_memory(model, num_parts)
    
    def _split_balanced(self, model, num_parts):
        """Split model to balance computation across parts"""
        layers = list(model.children())
        params_per_layer = [sum(p.numel() for p in layer.parameters()) 
                           for layer in layers]
        
        # Use dynamic programming to find optimal split points
        split_points = self._find_optimal_splits(params_per_layer, num_parts)
        
        parts = []
        for i, (start, end) in enumerate(split_points):
            part = nn.Sequential(*layers[start:end])
            parts.append(part)
        
        return parts
```

### Gradient Aggregation

Efficient gradient aggregation across distributed workers:

```python
class GradientAggregator:
    """Handles gradient aggregation for distributed training"""
    
    def __init__(self, compression_ratio=0.1):
        self.compression_ratio = compression_ratio
        self.gradient_buffer = {}
    
    def aggregate_gradients(self, worker_gradients):
        """
        Aggregate gradients from multiple workers
        
        Args:
            worker_gradients: List of gradient dictionaries from workers
        """
        # Apply gradient compression
        compressed_gradients = [
            self._compress_gradients(grad) 
            for grad in worker_gradients
        ]
        
        # Aggregate using weighted average
        aggregated = self._weighted_average(compressed_gradients)
        
        # Apply gradient clipping
        self._clip_gradients(aggregated, max_norm=1.0)
        
        return aggregated
    
    def _compress_gradients(self, gradients):
        """Compress gradients to reduce communication overhead"""
        compressed = {}
        for name, grad in gradients.items():
            # Top-k sparsification
            k = int(grad.numel() * self.compression_ratio)
            if k > 0:
                values, indices = torch.topk(grad.abs().flatten(), k)
                compressed[name] = (values, indices, grad.shape)
            else:
                compressed[name] = (torch.tensor([]), torch.tensor([]), grad.shape)
        return compressed
```

### 🔄 Model Synchronization

Efficient model synchronization across the network:

```python
class ModelSynchronizer:
    """Synchronizes model parameters across distributed workers"""
    
    def __init__(self, model, sync_frequency=10):
        self.model = model
        self.sync_frequency = sync_frequency
        self.step_count = 0
    
    def sync_step(self, worker_updates):
        """Perform one synchronization step"""
        self.step_count += 1
        
        if self.step_count % self.sync_frequency == 0:
            # Aggregate updates from all workers
            aggregated_update = self._aggregate_updates(worker_updates)
            
            # Apply update to global model
            self._apply_update(aggregated_update)
            
            # Broadcast updated model to all workers
            return self._get_model_state()
        
        return None
    
    def _aggregate_updates(self, worker_updates):
        """Aggregate parameter updates from workers"""
        aggregated = {}
        
        for param_name in worker_updates[0].keys():
            updates = [update[param_name] for update in worker_updates]
            aggregated[param_name] = torch.mean(torch.stack(updates), dim=0)
        
        return aggregated
```

## Performance Monitoring

### Training Metrics

Monitor distributed training progress:

```python
from hivemind.ai import TrainingMonitor

# Setup monitoring
monitor = TrainingMonitor(
    log_dir="logs/distributed_training",
    metrics=['loss', 'accuracy', 'throughput'],
    visualization_backend='tensorboard'
)

# During training
for epoch in range(num_epochs):
    for batch in dataloader:
        # Training step
        loss = train_step(batch)
        
        # Log metrics
        monitor.log_metric('loss', loss, step=global_step)
        monitor.log_metric('throughput', samples_per_second, step=global_step)
        
        # Log system metrics
        monitor.log_system_metrics(
            gpu_utilization=get_gpu_usage(),
            memory_usage=get_memory_usage(),
            network_bandwidth=get_network_stats()
        )
```

### Model Analysis

Analyze model performance and resource usage:

```python
from hivemind.ai import ModelAnalyzer

# Analyze model complexity
analyzer = ModelAnalyzer()
analysis = analyzer.analyze_model(
    model=my_model,
    input_shape=(1, 3, 224, 224),
    device='cuda'
)

print(f"Parameters: {analysis['total_params']:,}")
print(f"FLOPs: {analysis['flops']:,}")
print(f"Memory usage: {analysis['memory_mb']:.1f} MB")
print(f"Inference time: {analysis['inference_time']:.2f} ms")

# Analyze distributed performance
dist_analysis = analyzer.analyze_distributed_performance(
    model_parts=distributed_model_parts,
    communication_overhead=comm_overhead
)

print(f"Communication ratio: {dist_analysis['comm_ratio']:.2%}")
print(f"Parallel efficiency: {dist_analysis['efficiency']:.2%}")
```

## Supported Models and Frameworks

### 🤖 Supported Model Types

- **🔤 Natural Language Processing**:
  - BERT, GPT, T5, RoBERTa
  - Custom transformer architectures
  - Sequence-to-sequence models

- **Computer Vision**:
  - ResNet, VGG, DenseNet
  - Vision Transformers (ViT)
  - Object detection models (YOLO, R-CNN)

- **Audio Processing**:
  - Wav2Vec, Whisper
  - Custom audio classification models
  - Speech synthesis models

- **🧬 Scientific Computing**:
  - Molecular dynamics models
  - Climate simulation models
  - Physics simulation networks

### Framework Integration

- **PyTorch**: Full native support
- **TensorFlow**: Limited support via ONNX conversion
- **JAX**: Experimental support
- **Hugging Face**: Native integration for pretrained models

## Troubleshooting

### ❓ Common Issues

#### 1. **Memory Issues**
```bash
# Monitor GPU memory
nvidia-smi --query-gpu=memory.used,memory.total --format=csv

# Optimize model for memory
export PYTORCH_CUDA_ALLOC_CONF=max_split_size_mb:128

# Use gradient checkpointing
model.gradient_checkpointing_enable()
```

#### 2. **Communication Bottlenecks**
```bash
# Monitor network bandwidth
iftop -i eth0

# Optimize gradient compression
export HIVEMIND_COMPRESSION_RATIO=0.05

# Use efficient serialization
export HIVEMIND_SERIALIZATION=msgpack
```

#### 3. **Model Convergence Issues**
```python
# Adjust learning rate for distributed training
base_lr = 1e-4
distributed_lr = base_lr * world_size

# Use learning rate scaling
scheduler = torch.optim.lr_scheduler.LinearLR(
    optimizer, 
    start_factor=1.0/world_size, 
    total_iters=warmup_steps
)
```

## License

This AI module is part of the HiveMind project and is licensed under the **GNU General Public License v3.0** - see the [LICENSE](../LICENSE.txt) file for details.

## Contact & Support

### Contributing
We welcome contributions to improve the AI module:
- **Bug Reports**: [GitHub Issues](https://github.com/him6794/hivemind/issues)
- **Feature Requests**: [GitHub Discussions](https://github.com/him6794/hivemind/discussions)
- **📧 Technical Support**: [ai-support@hivemind.justin0711.com](mailto:ai-support@hivemind.justin0711.com)

### Additional Resources
- **API Documentation**: [AI Module API Reference](../docs/API.md#ai-module)
- **🎓 Tutorials**: [AI Training Tutorials](../docs/tutorials/ai/)
- **Benchmarks**: [Performance Benchmarks](../docs/benchmarks/ai/)

---

<div align="center">

**🧠 Democratizing AI with Distributed Computing 🧠**

*Build, train, and deploy AI models at scale with HiveMind*

[![GitHub Stars](https://img.shields.io/github/stars/him6794/hivemind?style=social)](https://github.com/him6794/hivemind)
[![Discord](https://img.shields.io/discord/123456789?style=social&logo=discord)](https://discord.gg/hivemind)

</div>
