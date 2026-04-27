package handlers

// task.go: handle UploadTask / GetTaskResult / StopTask

func UploadTask(req interface{}) (interface{}, error) {
	// validate token, create Task record in storage, enqueue to scheduler
	return nil, nil
}

func GetTaskResult(req interface{}) (interface{}, error) {
	// return torrent/metadata for result if available
	return nil, nil
}

func StopTask(req interface{}) (interface{}, error) {
	// instruct scheduler to cancel and notify worker(s)
	return nil, nil
}
