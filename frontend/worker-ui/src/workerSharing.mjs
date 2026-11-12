export function buildSharingRequest(base, token, settings) {
  if (!String(token || '').trim()) throw new Error('A signed-in session is required');
  const options = { method: 'GET', headers: { Authorization: `Bearer ${token.trim()}` } };
  if (settings) {
    const { mode } = settings;
    const idle_seconds = Number(settings.idle_seconds);
    const cpu_percent_threshold = Number(settings.cpu_percent_threshold);
    if (!['always', 'idle'].includes(mode) || !Number.isInteger(idle_seconds) || idle_seconds < 1 || idle_seconds > 86400 || !Number.isFinite(cpu_percent_threshold) || cpu_percent_threshold < 0 || cpu_percent_threshold > 100) {
      throw new Error('Invalid sharing settings');
    }
    options.method = 'POST';
    options.headers['Content-Type'] = 'application/json';
    options.body = JSON.stringify({ mode, idle_seconds, cpu_percent_threshold });
  }
  return { url: `${String(base || '').replace(/\/+$/, '')}/api/sharing`, options };
}

export function sharingView(value) {
  return {
    status: !value ? 'Sharing status is unavailable.' : value.accepts_tasks === true ? 'Accepting new tasks' : 'Waiting for this computer to become idle',
    explanation: value?.source === 'last_input'
      ? 'On Windows, idle means no keyboard or mouse input in this session.'
      : value?.source === 'cpu_usage'
        ? 'On Linux, idle means CPU usage stays below the threshold for the idle period.'
        : 'Idle detection is unavailable. Idle mode will not accept new tasks.',
  };
}
