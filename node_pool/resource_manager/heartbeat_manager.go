package main

import (
	"context"
	"fmt"
	"log"
	"time"

	"github.com/go-redis/redis/v8"
)

// HeartbeatManager 心跳管理器
type HeartbeatManager struct {
	redisClient      *redis.Client
	ctx              context.Context
	onlineThreshold  int64 // 秒
	cleanupThreshold int64 // 秒
}

// NewHeartbeatManager 創建心跳管理器
func NewHeartbeatManager(redisAddr string, redisDB int, onlineThreshold, cleanupThreshold int64) (*HeartbeatManager, error) {
	client := redis.NewClient(&redis.Options{
		Addr: redisAddr,
		DB:   redisDB,
	})

	ctx := context.Background()
	if err := client.Ping(ctx).Err(); err != nil {
		return nil, fmt.Errorf("redis connection failed: %w", err)
	}

	log.Println("HeartbeatManager: Redis 連線成功")
	return &HeartbeatManager{
		redisClient:      client,
		ctx:              ctx,
		onlineThreshold:  onlineThreshold,
		cleanupThreshold: cleanupThreshold,
	}, nil
}

// UpdateHeartbeat 更新節點心跳（輕量級操作）
func (hm *HeartbeatManager) UpdateHeartbeat(nodeID string) error {
	nodeKey := fmt.Sprintf("node:%s", nodeID)
	timestamp := time.Now().Unix()

	// 只更新心跳時間和更新時間
	pipe := hm.redisClient.Pipeline()
	pipe.HSet(hm.ctx, nodeKey, "last_heartbeat", fmt.Sprintf("%d", timestamp))
	pipe.HSet(hm.ctx, nodeKey, "updated_at", fmt.Sprintf("%d", timestamp))

	_, err := pipe.Exec(hm.ctx)
	if err != nil {
		return fmt.Errorf("failed to update heartbeat for node %s: %w", nodeID, err)
	}

	log.Printf("✓ 節點 %s 心跳已更新 (時間: %d)", nodeID, timestamp)
	return nil
}

// CheckNodeOnline 檢查節點是否在線
func (hm *HeartbeatManager) CheckNodeOnline(nodeID string) (bool, error) {
	nodeKey := fmt.Sprintf("node:%s", nodeID)

	lastHeartbeat, err := hm.redisClient.HGet(hm.ctx, nodeKey, "last_heartbeat").Result()
	if err == redis.Nil {
		return false, nil // 節點不存在
	}
	if err != nil {
		return false, fmt.Errorf("failed to get heartbeat for node %s: %w", nodeID, err)
	}

	var lastHB int64
	fmt.Sscanf(lastHeartbeat, "%d", &lastHB)

	now := time.Now().Unix()
	isOnline := (now - lastHB) <= hm.onlineThreshold

	return isOnline, nil
}

// CleanupOfflineNodes 清理長時間離線的節點
func (hm *HeartbeatManager) CleanupOfflineNodes() (int, error) {
	now := time.Now().Unix()
	cleaned := 0

	// 掃描所有節點
	keys, err := hm.redisClient.Keys(hm.ctx, "node:*").Result()
	if err != nil {
		return 0, fmt.Errorf("failed to scan node keys: %w", err)
	}

	for _, key := range keys {
		lastHeartbeat, err := hm.redisClient.HGet(hm.ctx, key, "last_heartbeat").Result()
		if err == redis.Nil {
			continue
		}
		if err != nil {
			log.Printf("警告: 無法獲取節點 %s 的心跳時間: %v", key, err)
			continue
		}

		var lastHB int64
		fmt.Sscanf(lastHeartbeat, "%d", &lastHB)

		// 檢查是否超過清理閾值
		if lastHB == 0 || (now-lastHB) > hm.cleanupThreshold {
			nodeID := key[5:] // 移除 "node:" 前綴

			// 刪除節點
			if err := hm.redisClient.Del(hm.ctx, key).Err(); err != nil {
				log.Printf("警告: 刪除離線節點 %s 失敗: %v", nodeID, err)
				continue
			}

			cleaned++
			log.Printf("✓ 已清理離線節點 %s (最後心跳: %d, 已離線 %d 秒)",
				nodeID, lastHB, now-lastHB)
		}
	}

	if cleaned > 0 {
		log.Printf("離線節點清理完成，共清理 %d 個節點", cleaned)
	}

	return cleaned, nil
}

// StartAutoCleanup 啟動自動清理定時任務
func (hm *HeartbeatManager) StartAutoCleanup(interval time.Duration) chan bool {
	stopChan := make(chan bool)

	go func() {
		ticker := time.NewTicker(interval)
		defer ticker.Stop()

		log.Printf("自動清理定時任務已啟動 (間隔: %v)", interval)

		for {
			select {
			case <-ticker.C:
				count, err := hm.CleanupOfflineNodes()
				if err != nil {
					log.Printf("自動清理錯誤: %v", err)
				} else if count > 0 {
					log.Printf("自動清理完成，清理了 %d 個節點", count)
				}
			case <-stopChan:
				log.Println("自動清理定時任務已停止")
				return
			}
		}
	}()

	return stopChan
}

// GetOnlineNodes 獲取所有在線節點列表
func (hm *HeartbeatManager) GetOnlineNodes() ([]string, error) {
	now := time.Now().Unix()
	var onlineNodes []string

	keys, err := hm.redisClient.Keys(hm.ctx, "node:*").Result()
	if err != nil {
		return nil, fmt.Errorf("failed to scan node keys: %w", err)
	}

	for _, key := range keys {
		lastHeartbeat, err := hm.redisClient.HGet(hm.ctx, key, "last_heartbeat").Result()
		if err == redis.Nil {
			continue
		}
		if err != nil {
			continue
		}

		var lastHB int64
		fmt.Sscanf(lastHeartbeat, "%d", &lastHB)

		if (now - lastHB) <= hm.onlineThreshold {
			nodeID := key[5:] // 移除 "node:" 前綴
			onlineNodes = append(onlineNodes, nodeID)
		}
	}

	return onlineNodes, nil
}

// GetNodeStats 獲取節點統計信息
func (hm *HeartbeatManager) GetNodeStats() (map[string]int, error) {
	now := time.Now().Unix()
	stats := map[string]int{
		"total":   0,
		"online":  0,
		"offline": 0,
	}

	keys, err := hm.redisClient.Keys(hm.ctx, "node:*").Result()
	if err != nil {
		return nil, fmt.Errorf("failed to scan node keys: %w", err)
	}

	stats["total"] = len(keys)

	for _, key := range keys {
		lastHeartbeat, err := hm.redisClient.HGet(hm.ctx, key, "last_heartbeat").Result()
		if err == redis.Nil {
			stats["offline"]++
			continue
		}
		if err != nil {
			stats["offline"]++
			continue
		}

		var lastHB int64
		fmt.Sscanf(lastHeartbeat, "%d", &lastHB)

		if (now - lastHB) <= hm.onlineThreshold {
			stats["online"]++
		} else {
			stats["offline"]++
		}
	}

	return stats, nil
}

// Close 關閉 Redis 連接
func (hm *HeartbeatManager) Close() error {
	return hm.redisClient.Close()
}
