package models

// models.go: shared struct types if not using generated proto messages

type WorkerInfo struct {
	Username    string
	IP          string
	CPUCores    int
	MemoryGB    int
	CPUScore    int
	GPUScore    int
	GPUMemoryGB int
	Location    string
}

type TaskRecord struct {
	TaskID      string
	Torrent     string
	MemoryGB    int
	CPUScore    int
	GPUScore    int
	GPUMemoryGB int
	Location    string
	HostCount   int
	Token       string
}
