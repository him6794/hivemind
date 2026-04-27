package storage

// storage.go: abstraction for persisting workers, tasks, and metadata

// Init initializes connections (redis/postgres)
func Init() error {
	// TODO: open DB/Redis connections based on config
	return nil
}

// SaveWorker saves worker info
func SaveWorker(w interface{}) error {
	return nil
}

// SaveTask saves task info
func SaveTask(t interface{}) error {
	return nil
}
