package main

import (
	"testing"
	"time"

	"github.com/alicebob/miniredis/v2"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func setupHeartbeatTest(t *testing.T) (*HeartbeatManager, *miniredis.Miniredis) {
	mr, err := miniredis.Run()
	require.NoError(t, err)

	hm, err := NewHeartbeatManager(mr.Addr(), 0, 180, 900)
	require.NoError(t, err)

	return hm, mr
}

func TestUpdateHeartbeat(t *testing.T) {
	hm, mr := setupHeartbeatTest(t)
	defer mr.Close()
	defer hm.Close()

	nodeID := "test_node_hb_1"

	// 初始化節點
	nodeKey := "node:" + nodeID
	hm.redisClient.HSet(hm.ctx, nodeKey, "hostname", "test-host")

	// 更新心跳
	err := hm.UpdateHeartbeat(nodeID)
	assert.NoError(t, err)

	// 驗證心跳時間已更新
	lastHB, err := hm.redisClient.HGet(hm.ctx, nodeKey, "last_heartbeat").Result()
	require.NoError(t, err)
	assert.NotEmpty(t, lastHB)
}

func TestCheckNodeOnline(t *testing.T) {
	hm, mr := setupHeartbeatTest(t)
	defer mr.Close()
	defer hm.Close()

	nodeID := "test_node_hb_2"
	nodeKey := "node:" + nodeID

	// 情況 1: 新鮮的心跳（在線）
	now := time.Now().Unix()
	hm.redisClient.HSet(hm.ctx, nodeKey, "last_heartbeat", now)

	isOnline, err := hm.CheckNodeOnline(nodeID)
	require.NoError(t, err)
	assert.True(t, isOnline)

	// 情況 2: 過期的心跳（離線）
	oldTime := now - 200 // 200 秒前（超過 180 秒閾值）
	hm.redisClient.HSet(hm.ctx, nodeKey, "last_heartbeat", oldTime)

	isOnline, err = hm.CheckNodeOnline(nodeID)
	require.NoError(t, err)
	assert.False(t, isOnline)
}

func TestCleanupOfflineNodes(t *testing.T) {
	hm, mr := setupHeartbeatTest(t)
	defer mr.Close()
	defer hm.Close()

	now := time.Now().Unix()

	// 創建 3 個節點：1 在線、1 離線但未達清理閾值、1 達到清理閾值
	nodes := []struct {
		id          string
		lastHB      int64
		shouldClean bool
	}{
		{"node_online", now, false},
		{"node_offline_short", now - 300, false}, // 離線 5 分鐘
		{"node_offline_long", now - 1000, true},  // 離線 16+ 分鐘（超過 900 秒）
	}

	for _, node := range nodes {
		nodeKey := "node:" + node.id
		hm.redisClient.HSet(hm.ctx, nodeKey, map[string]interface{}{
			"hostname":       "test-host",
			"last_heartbeat": node.lastHB,
		})
	}

	// 執行清理
	cleaned, err := hm.CleanupOfflineNodes()
	require.NoError(t, err)
	assert.Equal(t, 1, cleaned) // 只應清理 1 個節點

	// 驗證結果
	for _, node := range nodes {
		nodeKey := "node:" + node.id
		exists, err := hm.redisClient.Exists(hm.ctx, nodeKey).Result()
		require.NoError(t, err)

		if node.shouldClean {
			assert.Equal(t, int64(0), exists, "節點 %s 應該被清理", node.id)
		} else {
			assert.Equal(t, int64(1), exists, "節點 %s 不應該被清理", node.id)
		}
	}
}

func TestGetOnlineNodes(t *testing.T) {
	hm, mr := setupHeartbeatTest(t)
	defer mr.Close()
	defer hm.Close()

	now := time.Now().Unix()

	// 創建 2 在線、1 離線
	onlineNodes := []string{"node_1", "node_2"}
	offlineNodes := []string{"node_3"}

	for _, nodeID := range onlineNodes {
		nodeKey := "node:" + nodeID
		hm.redisClient.HSet(hm.ctx, nodeKey, "last_heartbeat", now)
	}

	for _, nodeID := range offlineNodes {
		nodeKey := "node:" + nodeID
		hm.redisClient.HSet(hm.ctx, nodeKey, "last_heartbeat", now-300)
	}

	// 獲取在線節點
	result, err := hm.GetOnlineNodes()
	require.NoError(t, err)
	assert.Len(t, result, 2)
	assert.Contains(t, result, "node_1")
	assert.Contains(t, result, "node_2")
}

func TestGetNodeStats(t *testing.T) {
	hm, mr := setupHeartbeatTest(t)
	defer mr.Close()
	defer hm.Close()

	now := time.Now().Unix()

	// 創建混合狀態節點
	hm.redisClient.HSet(hm.ctx, "node:online_1", "last_heartbeat", now)
	hm.redisClient.HSet(hm.ctx, "node:online_2", "last_heartbeat", now-50)
	hm.redisClient.HSet(hm.ctx, "node:offline_1", "last_heartbeat", now-300)

	stats, err := hm.GetNodeStats()
	require.NoError(t, err)

	assert.Equal(t, 3, stats["total"])
	assert.Equal(t, 2, stats["online"])
	assert.Equal(t, 1, stats["offline"])
}

func TestAutoCleanup(t *testing.T) {
	hm, mr := setupHeartbeatTest(t)
	defer mr.Close()
	defer hm.Close()

	now := time.Now().Unix()

	// 創建過期節點
	hm.redisClient.HSet(hm.ctx, "node:old_node", "last_heartbeat", now-1000)

	// 啟動自動清理（每 100ms 一次，測試用）
	stopChan := hm.StartAutoCleanup(100 * time.Millisecond)

	// 等待清理執行
	time.Sleep(200 * time.Millisecond)

	// 停止自動清理
	stopChan <- true

	// 驗證節點已被清理
	exists, err := hm.redisClient.Exists(hm.ctx, "node:old_node").Result()
	require.NoError(t, err)
	assert.Equal(t, int64(0), exists)
}

func TestConcurrentHeartbeatUpdates(t *testing.T) {
	hm, mr := setupHeartbeatTest(t)
	defer mr.Close()
	defer hm.Close()

	nodeID := "concurrent_node"
	nodeKey := "node:" + nodeID
	hm.redisClient.HSet(hm.ctx, nodeKey, "hostname", "test")

	// 並發更新心跳
	concurrency := 100
	done := make(chan bool, concurrency)

	for i := 0; i < concurrency; i++ {
		go func() {
			err := hm.UpdateHeartbeat(nodeID)
			assert.NoError(t, err)
			done <- true
		}()
	}

	// 等待完成
	for i := 0; i < concurrency; i++ {
		<-done
	}

	// 驗證心跳存在且有效
	lastHB, err := hm.redisClient.HGet(hm.ctx, nodeKey, "last_heartbeat").Result()
	require.NoError(t, err)
	assert.NotEmpty(t, lastHB)
}

func BenchmarkUpdateHeartbeat(b *testing.B) {
	mr, _ := miniredis.Run()
	defer mr.Close()

	hm, _ := NewHeartbeatManager(mr.Addr(), 0, 180, 900)
	defer hm.Close()

	nodeID := "bench_node"
	hm.redisClient.HSet(hm.ctx, "node:"+nodeID, "hostname", "test")

	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		hm.UpdateHeartbeat(nodeID)
	}
}

func BenchmarkCheckNodeOnline(b *testing.B) {
	mr, _ := miniredis.Run()
	defer mr.Close()

	hm, _ := NewHeartbeatManager(mr.Addr(), 0, 180, 900)
	defer hm.Close()

	nodeID := "bench_node"
	now := time.Now().Unix()
	hm.redisClient.HSet(hm.ctx, "node:"+nodeID, "last_heartbeat", now)

	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		hm.CheckNodeOnline(nodeID)
	}
}
