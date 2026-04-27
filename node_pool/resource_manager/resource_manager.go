package main

import (
	"context"
	"fmt"
	"log"
	"sync"
	"time"

	"github.com/go-redis/redis/v8"
)

// ResourceManager 資源管理器 - 負責原子性的資源分配與釋放
type ResourceManager struct {
	redisClient *redis.Client
	ctx         context.Context
	mu          sync.RWMutex
}

// NodeResources 節點資源結構
type NodeResources struct {
	TotalCPU        int
	AvailableCPU    int
	TotalMemory     float64
	AvailableMemory float64
	TotalGPU        int
	AvailableGPU    int
	TotalGPUMem     float64
	AvailableGPUMem float64
}

// TaskRequirement 任務需求
type TaskRequirement struct {
	CPUScore    int
	MemoryGB    float64
	GPUScore    int
	GPUMemoryGB float64
}

// NewResourceManager 創建資源管理器
func NewResourceManager(redisAddr string, redisDB int) (*ResourceManager, error) {
	client := redis.NewClient(&redis.Options{
		Addr: redisAddr,
		DB:   redisDB,
	})

	ctx := context.Background()
	if err := client.Ping(ctx).Err(); err != nil {
		return nil, fmt.Errorf("redis connection failed: %w", err)
	}

	log.Println("ResourceManager: Redis 連線成功")
	return &ResourceManager{
		redisClient: client,
		ctx:         ctx,
	}, nil
}

// AllocateResources 原子性分配資源（使用 Lua script）
func (rm *ResourceManager) AllocateResources(nodeID string, taskID string, req TaskRequirement) error {
	// Lua script 保證原子性
	script := `
		local nodeKey = KEYS[1]
		local taskID = ARGV[1]
		local cpuReq = tonumber(ARGV[2])
		local memReq = tonumber(ARGV[3])
		local gpuReq = tonumber(ARGV[4])
		local gpuMemReq = tonumber(ARGV[5])
		
		-- 獲取當前可用資源
		local availCPU = tonumber(redis.call('HGET', nodeKey, 'available_cpu_score') or '0')
		local availMem = tonumber(redis.call('HGET', nodeKey, 'available_memory_gb') or '0')
		local availGPU = tonumber(redis.call('HGET', nodeKey, 'available_gpu_score') or '0')
		local availGPUMem = tonumber(redis.call('HGET', nodeKey, 'available_gpu_memory_gb') or '0')
		
		-- 檢查資源是否足夠
		if availCPU < cpuReq or availMem < memReq or availGPU < gpuReq or availGPUMem < gpuMemReq then
			return {0, availCPU, availMem, availGPU, availGPUMem}
		end
		
		-- 扣除資源
		local newCPU = availCPU - cpuReq
		local newMem = availMem - memReq
		local newGPU = availGPU - gpuReq
		local newGPUMem = availGPUMem - gpuMemReq
		
		-- 更新 Redis
		redis.call('HSET', nodeKey, 'available_cpu_score', tostring(newCPU))
		redis.call('HSET', nodeKey, 'available_memory_gb', tostring(newMem))
		redis.call('HSET', nodeKey, 'available_gpu_score', tostring(newGPU))
		redis.call('HSET', nodeKey, 'available_gpu_memory_gb', tostring(newGPUMem))
		redis.call('HSET', nodeKey, 'updated_at', tostring(ARGV[6]))
		
		-- 添加任務到運行列表
		local runningTasks = redis.call('HGET', nodeKey, 'running_task_ids') or ''
		if runningTasks == '' then
			redis.call('HSET', nodeKey, 'running_task_ids', taskID)
		else
			redis.call('HSET', nodeKey, 'running_task_ids', runningTasks .. ',' .. taskID)
		end
		
		redis.call('HINCRBY', nodeKey, 'current_tasks', 1)
		
		return {1, newCPU, newMem, newGPU, newGPUMem}
	`

	nodeKey := fmt.Sprintf("node:%s", nodeID)
	timestamp := time.Now().Unix()

	result, err := rm.redisClient.Eval(rm.ctx, script, []string{nodeKey},
		taskID, req.CPUScore, req.MemoryGB, req.GPUScore, req.GPUMemoryGB, timestamp).Result()

	if err != nil {
		return fmt.Errorf("lua script execution failed: %w", err)
	}

	resultSlice, ok := result.([]interface{})
	if !ok || len(resultSlice) < 5 {
		return fmt.Errorf("unexpected result format")
	}

	success := resultSlice[0].(int64)
	if success == 0 {
		return fmt.Errorf("insufficient resources: available CPU=%v, MEM=%v, GPU=%v, GPU_MEM=%v",
			resultSlice[1], resultSlice[2], resultSlice[3], resultSlice[4])
	}

	log.Printf("✓ 節點 %s 資源分配成功: 任務 %s (剩餘: CPU=%v, MEM=%v, GPU=%v, GPU_MEM=%v)",
		nodeID, taskID, resultSlice[1], resultSlice[2], resultSlice[3], resultSlice[4])

	return nil
}

// ReleaseResources 原子性釋放資源
func (rm *ResourceManager) ReleaseResources(nodeID string, taskID string, req TaskRequirement) error {
	script := `
		local nodeKey = KEYS[1]
		local taskID = ARGV[1]
		local cpuReq = tonumber(ARGV[2])
		local memReq = tonumber(ARGV[3])
		local gpuReq = tonumber(ARGV[4])
		local gpuMemReq = tonumber(ARGV[5])
		
		-- 獲取總資源與當前可用資源
		local totalCPU = tonumber(redis.call('HGET', nodeKey, 'total_cpu_score') or '0')
		local totalMem = tonumber(redis.call('HGET', nodeKey, 'total_memory_gb') or '0')
		local totalGPU = tonumber(redis.call('HGET', nodeKey, 'total_gpu_score') or '0')
		local totalGPUMem = tonumber(redis.call('HGET', nodeKey, 'total_gpu_memory_gb') or '0')
		
		local availCPU = tonumber(redis.call('HGET', nodeKey, 'available_cpu_score') or '0')
		local availMem = tonumber(redis.call('HGET', nodeKey, 'available_memory_gb') or '0')
		local availGPU = tonumber(redis.call('HGET', nodeKey, 'available_gpu_score') or '0')
		local availGPUMem = tonumber(redis.call('HGET', nodeKey, 'available_gpu_memory_gb') or '0')
		
		-- 釋放資源（夾限不超過總量）
		local newCPU = math.min(availCPU + cpuReq, totalCPU)
		local newMem = math.min(availMem + memReq, totalMem)
		local newGPU = math.min(availGPU + gpuReq, totalGPU)
		local newGPUMem = math.min(availGPUMem + gpuMemReq, totalGPUMem)
		
		-- 更新 Redis
		redis.call('HSET', nodeKey, 'available_cpu_score', tostring(newCPU))
		redis.call('HSET', nodeKey, 'available_memory_gb', tostring(newMem))
		redis.call('HSET', nodeKey, 'available_gpu_score', tostring(newGPU))
		redis.call('HSET', nodeKey, 'available_gpu_memory_gb', tostring(newGPUMem))
		redis.call('HSET', nodeKey, 'updated_at', tostring(ARGV[6]))
		
		-- 從運行列表移除任務
		local runningTasks = redis.call('HGET', nodeKey, 'running_task_ids') or ''
		local newTasks = ''
		for task in string.gmatch(runningTasks, '([^,]+)') do
			if task ~= taskID then
				if newTasks == '' then
					newTasks = task
				else
					newTasks = newTasks .. ',' .. task
				end
			end
		end
		redis.call('HSET', nodeKey, 'running_task_ids', newTasks)
		
		-- 減少任務計數（確保不為負）
		local currentTasks = tonumber(redis.call('HGET', nodeKey, 'current_tasks') or '0')
		if currentTasks > 0 then
			redis.call('HSET', nodeKey, 'current_tasks', tostring(currentTasks - 1))
		end
		
		return {1, newCPU, newMem, newGPU, newGPUMem}
	`

	nodeKey := fmt.Sprintf("node:%s", nodeID)
	timestamp := time.Now().Unix()

	result, err := rm.redisClient.Eval(rm.ctx, script, []string{nodeKey},
		taskID, req.CPUScore, req.MemoryGB, req.GPUScore, req.GPUMemoryGB, timestamp).Result()

	if err != nil {
		return fmt.Errorf("lua script execution failed: %w", err)
	}

	resultSlice, ok := result.([]interface{})
	if !ok || len(resultSlice) < 5 {
		return fmt.Errorf("unexpected result format")
	}

	log.Printf("✓ 節點 %s 資源釋放成功: 任務 %s (新可用: CPU=%v, MEM=%v, GPU=%v, GPU_MEM=%v)",
		nodeID, taskID, resultSlice[1], resultSlice[2], resultSlice[3], resultSlice[4])

	return nil
}

// GetNodeResources 獲取節點資源狀態
func (rm *ResourceManager) GetNodeResources(nodeID string) (*NodeResources, error) {
	nodeKey := fmt.Sprintf("node:%s", nodeID)

	fields := []string{
		"total_cpu_score", "available_cpu_score",
		"total_memory_gb", "available_memory_gb",
		"total_gpu_score", "available_gpu_score",
		"total_gpu_memory_gb", "available_gpu_memory_gb",
	}

	values, err := rm.redisClient.HMGet(rm.ctx, nodeKey, fields...).Result()
	if err != nil {
		return nil, fmt.Errorf("failed to get node resources: %w", err)
	}

	parseFloat := func(val interface{}) float64 {
		if val == nil {
			return 0
		}
		str, ok := val.(string)
		if !ok {
			return 0
		}
		var f float64
		fmt.Sscanf(str, "%f", &f)
		return f
	}

	parseInt := func(val interface{}) int {
		return int(parseFloat(val))
	}

	return &NodeResources{
		TotalCPU:        parseInt(values[0]),
		AvailableCPU:    parseInt(values[1]),
		TotalMemory:     parseFloat(values[2]),
		AvailableMemory: parseFloat(values[3]),
		TotalGPU:        parseInt(values[4]),
		AvailableGPU:    parseInt(values[5]),
		TotalGPUMem:     parseFloat(values[6]),
		AvailableGPUMem: parseFloat(values[7]),
	}, nil
}

// Close 關閉 Redis 連接
func (rm *ResourceManager) Close() error {
	return rm.redisClient.Close()
}
