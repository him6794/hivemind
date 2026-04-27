package models

// models.go: shared types used by worker (if not using generated proto types)

type WorkerStatus struct {
	TaskID      string
	CPUUsage    float32
	MemoryUsage float32
	GPUUsage    float32
}

type TaskSpec struct {
	TaskID   string
	Torrent  string
	MemoryGB int
	CPUScore int
	GPUScore int
}
