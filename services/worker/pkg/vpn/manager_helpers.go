package vpn

// GetTsnetClient returns the tsnet client instance
func (m *Manager) GetTsnetClient() *TsnetClient {
	m.mu.RLock()
	defer m.mu.RUnlock()
	return m.tsnetClient
}

// SetTaskID sets the current task ID
func (m *Manager) SetTaskID(taskID string) {
	m.mu.Lock()
	defer m.mu.Unlock()
	m.taskID = taskID
}

// GetTaskID returns the current task ID
func (m *Manager) GetTaskID() string {
	m.mu.RLock()
	defer m.mu.RUnlock()
	return m.taskID
}

// IsRegistered returns whether the VPN is registered
func (m *Manager) IsRegistered() bool {
	m.mu.RLock()
	defer m.mu.RUnlock()
	return m.registered
}
