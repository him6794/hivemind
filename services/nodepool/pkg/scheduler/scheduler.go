package scheduler

// scheduler.go: simple scheduler stub

// EnqueueTask accepts a task record and picks workers
func EnqueueTask(task interface{}) error {
	// TODO: implement matching logic based on cpu/gpu/memory and location
	return nil
}

// CancelTask cancels a scheduled or running task
func CancelTask(taskID string) error {
	return nil
}
