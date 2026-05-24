package main

import "testing"

func TestWorkerRegisterPayloadUsesDefaults(t *testing.T) {
	payload := workerRegisterPayload(workerInfo{
		ID:   "worker1",
		Addr: "127.0.0.1:51053",
		Meta: map[string]string{},
	})

	if payload.Username != "worker1" || payload.IP != "127.0.0.1:51053" {
		t.Fatalf("unexpected identity: %+v", payload)
	}
	if payload.CPUCores != 8 || payload.MemoryGB != 16 || payload.CPUScore != 100 || payload.GPUScore != 80 || payload.GPUMemoryGB != 8 {
		t.Fatalf("defaults not applied: %+v", payload)
	}
	if payload.Location != "Local" {
		t.Fatalf("location=%q, want Local", payload.Location)
	}
}

func TestWorkerRegisterPayloadUsesMetadata(t *testing.T) {
	payload := workerRegisterPayload(workerInfo{
		ID:   "worker2",
		Addr: "127.0.0.1:51054",
		Meta: map[string]string{
			"cpu_cores":  "12",
			"memory_gb":  "32",
			"cpu_score":  "240",
			"gpu_score":  "192",
			"gpu_memory": "15",
			"location":   "Taipei",
		},
	})

	if payload.CPUCores != 12 || payload.MemoryGB != 32 || payload.CPUScore != 240 || payload.GPUScore != 192 || payload.GPUMemoryGB != 15 {
		t.Fatalf("metadata not applied: %+v", payload)
	}
	if payload.Location != "Taipei" {
		t.Fatalf("location=%q, want Taipei", payload.Location)
	}
}

func TestFindTaskMatchesBothJSONShapes(t *testing.T) {
	tasks := []map[string]any{
		{"task_id": "snake", "Status": "RUNNING"},
		{"TaskID": "camel", "Status": "COMPLETED"},
	}

	if got := findTask(tasks, "snake"); got == nil || got["Status"] != "RUNNING" {
		t.Fatalf("snake task not found: %+v", got)
	}
	if got := findTask(tasks, "camel"); got == nil || got["Status"] != "COMPLETED" {
		t.Fatalf("camel task not found: %+v", got)
	}
	if got := findTask(tasks, "missing"); got != nil {
		t.Fatalf("unexpected task found: %+v", got)
	}
}
