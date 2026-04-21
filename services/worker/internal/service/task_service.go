package service

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"sync"
)

type TaskStatus string

const (
	TaskStatusRunning   TaskStatus = "RUNNING"
	TaskStatusCompleted TaskStatus = "COMPLETED"
	TaskStatusStopped   TaskStatus = "STOPPED"
)

type Task struct {
	TaskID         string
	Torrent        string
	Output         string
	ResultTorrent  string
	Status         TaskStatus
	CPUUsage       float32
	MemoryUsage    float32
	GPUUsage       float32
	GPUMemoryUsage float32
}

type TaskService struct {
	mu          sync.RWMutex
	tasks       map[string]*Task
	persistPath string
}

func NewTaskService() *TaskService {
	s := &TaskService{tasks: make(map[string]*Task), persistPath: os.Getenv("WORKER_TASK_PERSIST_PATH")}
	s.loadFromDisk()
	return s
}

func (s *TaskService) loadFromDisk() {
	if s.persistPath == "" {
		return
	}
	b, err := os.ReadFile(s.persistPath)
	if err != nil {
		return
	}
	m := make(map[string]*Task)
	if err := json.Unmarshal(b, &m); err != nil {
		return
	}
	s.tasks = m
}

func (s *TaskService) persistLocked() {
	if s.persistPath == "" {
		return
	}
	b, err := json.MarshalIndent(s.tasks, "", "  ")
	if err != nil {
		return
	}
	_ = os.WriteFile(s.persistPath, b, 0o644)
}

func (s *TaskService) ExecuteTask(ctx context.Context, taskID, torrent string, cpuUsage, gpuUsage float32, memoryGB, gpuMemoryGB int32) error {
	_ = ctx
	if taskID == "" {
		return fmt.Errorf("task_id is required")
	}
	if torrent == "" {
		return fmt.Errorf("torrent is required")
	}

	s.mu.Lock()
	defer s.mu.Unlock()

	s.tasks[taskID] = &Task{
		TaskID:         taskID,
		Torrent:        torrent,
		Status:         TaskStatusRunning,
		CPUUsage:       cpuUsage,
		GPUUsage:       gpuUsage,
		MemoryUsage:    float32(memoryGB),
		GPUMemoryUsage: float32(gpuMemoryGB),
	}
	s.persistLocked()
	return nil
}

func (s *TaskService) UploadOutput(ctx context.Context, taskID, output string) error {
	_ = ctx
	s.mu.Lock()
	defer s.mu.Unlock()

	t, ok := s.tasks[taskID]
	if !ok {
		return fmt.Errorf("task not found")
	}
	t.Output = output
	s.persistLocked()
	return nil
}

func (s *TaskService) UploadResult(ctx context.Context, taskID, resultTorrent string) error {
	_ = ctx
	s.mu.Lock()
	defer s.mu.Unlock()

	t, ok := s.tasks[taskID]
	if !ok {
		return fmt.Errorf("task not found")
	}
	t.ResultTorrent = resultTorrent
	t.Status = TaskStatusCompleted
	s.persistLocked()
	return nil
}

func (s *TaskService) GetOutput(ctx context.Context, taskID string) (string, error) {
	_ = ctx
	s.mu.RLock()
	defer s.mu.RUnlock()

	t, ok := s.tasks[taskID]
	if !ok {
		return "", fmt.Errorf("task not found")
	}
	return t.Output, nil
}

func (s *TaskService) StopTask(ctx context.Context, taskID string) error {
	_ = ctx
	s.mu.Lock()
	defer s.mu.Unlock()

	t, ok := s.tasks[taskID]
	if !ok {
		return fmt.Errorf("task not found")
	}
	t.Status = TaskStatusStopped
	s.persistLocked()
	return nil
}

func (s *TaskService) UpdateUsage(ctx context.Context, taskID string, cpu, memory, gpu, gpuMemory float32) error {
	_ = ctx
	s.mu.Lock()
	defer s.mu.Unlock()

	t, ok := s.tasks[taskID]
	if !ok {
		return fmt.Errorf("task not found")
	}
	t.CPUUsage = cpu
	t.MemoryUsage = memory
	t.GPUUsage = gpu
	t.GPUMemoryUsage = gpuMemory
	s.persistLocked()
	return nil
}
