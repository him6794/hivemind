package main

import (
	"testing"

	"github.com/alicebob/miniredis/v2"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func setupTestRedis(t *testing.T) (*ResourceManager, *miniredis.Miniredis) {
	// 使用 miniredis 建立測試用 Redis
	mr, err := miniredis.Run()
	require.NoError(t, err)

	rm, err := NewResourceManager(mr.Addr(), 0)
	require.NoError(t, err)

	return rm, mr
}

func initializeTestNode(t *testing.T, rm *ResourceManager, nodeID string) {
	// 初始化測試節點
	nodeKey := "node:" + nodeID
	rm.redisClient.HSet(rm.ctx, nodeKey, map[string]interface{}{
		"total_cpu_score":         "100",
		"available_cpu_score":     "100",
		"total_memory_gb":         "16",
		"available_memory_gb":     "16",
		"total_gpu_score":         "50",
		"available_gpu_score":     "50",
		"total_gpu_memory_gb":     "8",
		"available_gpu_memory_gb": "8",
		"current_tasks":           "0",
		"running_task_ids":        "",
	})
}

func TestAllocateResources_Success(t *testing.T) {
	rm, mr := setupTestRedis(t)
	defer mr.Close()
	defer rm.Close()

	nodeID := "test_node_1"
	initializeTestNode(t, rm, nodeID)

	req := TaskRequirement{
		CPUScore:    20,
		MemoryGB:    4.0,
		GPUScore:    10,
		GPUMemoryGB: 2.0,
	}

	err := rm.AllocateResources(nodeID, "task_001", req)
	assert.NoError(t, err)

	// 驗證資源已正確扣除
	resources, err := rm.GetNodeResources(nodeID)
	require.NoError(t, err)

	assert.Equal(t, 80, resources.AvailableCPU)
	assert.Equal(t, 12.0, resources.AvailableMemory)
	assert.Equal(t, 40, resources.AvailableGPU)
	assert.Equal(t, 6.0, resources.AvailableGPUMem)
}

func TestAllocateResources_InsufficientResources(t *testing.T) {
	rm, mr := setupTestRedis(t)
	defer mr.Close()
	defer rm.Close()

	nodeID := "test_node_2"
	initializeTestNode(t, rm, nodeID)

	// 嘗試分配超過可用資源
	req := TaskRequirement{
		CPUScore:    150, // 超過 100
		MemoryGB:    4.0,
		GPUScore:    10,
		GPUMemoryGB: 2.0,
	}

	err := rm.AllocateResources(nodeID, "task_002", req)
	assert.Error(t, err)
	assert.Contains(t, err.Error(), "insufficient resources")
}

func TestReleaseResources_Success(t *testing.T) {
	rm, mr := setupTestRedis(t)
	defer mr.Close()
	defer rm.Close()

	nodeID := "test_node_3"
	initializeTestNode(t, rm, nodeID)

	req := TaskRequirement{
		CPUScore:    20,
		MemoryGB:    4.0,
		GPUScore:    10,
		GPUMemoryGB: 2.0,
	}

	// 先分配
	err := rm.AllocateResources(nodeID, "task_003", req)
	require.NoError(t, err)

	// 再釋放
	err = rm.ReleaseResources(nodeID, "task_003", req)
	assert.NoError(t, err)

	// 驗證資源已恢復
	resources, err := rm.GetNodeResources(nodeID)
	require.NoError(t, err)

	assert.Equal(t, 100, resources.AvailableCPU)
	assert.Equal(t, 16.0, resources.AvailableMemory)
	assert.Equal(t, 50, resources.AvailableGPU)
	assert.Equal(t, 8.0, resources.AvailableGPUMem)
}

func TestConcurrentAllocations(t *testing.T) {
	rm, mr := setupTestRedis(t)
	defer mr.Close()
	defer rm.Close()

	nodeID := "test_node_4"
	initializeTestNode(t, rm, nodeID)

	// 並發分配測試
	concurrency := 10
	successCount := 0
	errorCount := 0

	done := make(chan bool, concurrency)

	for i := 0; i < concurrency; i++ {
		go func(index int) {
			req := TaskRequirement{
				CPUScore:    15, // 每個任務 15 CPU
				MemoryGB:    2.0,
				GPUScore:    5,
				GPUMemoryGB: 1.0,
			}

			taskID := "concurrent_task_" + string(rune('A'+index))
			err := rm.AllocateResources(nodeID, taskID, req)

			if err == nil {
				successCount++
			} else {
				errorCount++
			}

			done <- true
		}(i)
	}

	// 等待所有 goroutine 完成
	for i := 0; i < concurrency; i++ {
		<-done
	}

	// 100 CPU 只能容納 6 個任務（6*15=90），所以應該有 6 成功、4 失敗
	t.Logf("Success: %d, Errors: %d", successCount, errorCount)

	// 驗證最終資源狀態
	resources, err := rm.GetNodeResources(nodeID)
	require.NoError(t, err)

	// 可用 CPU 應該介於 0-100 之間
	assert.GreaterOrEqual(t, resources.AvailableCPU, 0)
	assert.LessOrEqual(t, resources.AvailableCPU, 100)

	// 可用資源不應超過總資源
	assert.LessOrEqual(t, resources.AvailableCPU, resources.TotalCPU)
	assert.LessOrEqual(t, resources.AvailableMemory, resources.TotalMemory)
}

func TestReleaseResources_DoesNotExceedTotal(t *testing.T) {
	rm, mr := setupTestRedis(t)
	defer mr.Close()
	defer rm.Close()

	nodeID := "test_node_5"
	initializeTestNode(t, rm, nodeID)

	req := TaskRequirement{
		CPUScore:    20,
		MemoryGB:    4.0,
		GPUScore:    10,
		GPUMemoryGB: 2.0,
	}

	// 釋放從未分配的資源（模擬重複釋放）
	err := rm.ReleaseResources(nodeID, "task_999", req)
	assert.NoError(t, err)

	// 驗證可用資源不會超過總資源
	resources, err := rm.GetNodeResources(nodeID)
	require.NoError(t, err)

	assert.LessOrEqual(t, resources.AvailableCPU, resources.TotalCPU)
	assert.LessOrEqual(t, resources.AvailableMemory, resources.TotalMemory)
	assert.LessOrEqual(t, resources.AvailableGPU, resources.TotalGPU)
	assert.LessOrEqual(t, resources.AvailableGPUMem, resources.TotalGPUMem)
}

func TestTaskTracking(t *testing.T) {
	rm, mr := setupTestRedis(t)
	defer mr.Close()
	defer rm.Close()

	nodeID := "test_node_6"
	initializeTestNode(t, rm, nodeID)

	req := TaskRequirement{
		CPUScore:    10,
		MemoryGB:    2.0,
		GPUScore:    5,
		GPUMemoryGB: 1.0,
	}

	// 分配多個任務
	err := rm.AllocateResources(nodeID, "task_A", req)
	require.NoError(t, err)

	err = rm.AllocateResources(nodeID, "task_B", req)
	require.NoError(t, err)

	// 檢查任務計數
	nodeKey := "node:" + nodeID
	currentTasks, err := rm.redisClient.HGet(rm.ctx, nodeKey, "current_tasks").Result()
	require.NoError(t, err)
	assert.Equal(t, "2", currentTasks)

	// 釋放一個任務
	err = rm.ReleaseResources(nodeID, "task_A", req)
	require.NoError(t, err)

	currentTasks, err = rm.redisClient.HGet(rm.ctx, nodeKey, "current_tasks").Result()
	require.NoError(t, err)
	assert.Equal(t, "1", currentTasks)
}

func BenchmarkAllocateResources(b *testing.B) {
	mr, _ := miniredis.Run()
	defer mr.Close()

	rm, _ := NewResourceManager(mr.Addr(), 0)
	defer rm.Close()

	nodeID := "bench_node"
	initializeTestNode(&testing.T{}, rm, nodeID)

	req := TaskRequirement{
		CPUScore:    5,
		MemoryGB:    1.0,
		GPUScore:    2,
		GPUMemoryGB: 0.5,
	}

	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		taskID := "bench_task_" + string(rune(i%26+'A'))
		rm.AllocateResources(nodeID, taskID, req)
	}
}
