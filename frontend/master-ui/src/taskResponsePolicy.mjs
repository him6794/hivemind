const FAILURE_MESSAGE_FIELDS = ['message', 'status_message', 'log'];

function usableText(value) {
  return typeof value === 'string' ? value.trim() : '';
}

export function taskResponseFailureMessage(response, fallbackMessage, transportOk = true) {
  if (transportOk && response?.success === true) {
    return '';
  }

  for (const field of FAILURE_MESSAGE_FIELDS) {
    const message = usableText(response?.[field]);
    if (message) {
      return message;
    }
  }

  return usableText(fallbackMessage) || 'Task request failed';
}

export function taskRequestFailureText(operation, error, fallbackMessage) {
  const label = usableText(operation) || 'Task request';
  const detail = usableText(error?.message) || usableText(fallbackMessage) || 'Task request failed';
  return `${label} failed: ${detail}`;
}

/// Managed tasks deliberately persist output as a task log instead of the
/// legacy result torrent; the result endpoint answers success=false with
/// stable guidance. Callers should show the task log inline instead of
/// treating that contract response as an error.
export function isManagedLogGuidanceResult(response) {
  return (
    response?.success === false &&
    !usableText(response?.result_torrent) &&
    /task log/i.test(usableText(response?.status_message))
  );
}

/// Managed GPU-v1 results are typed JSON returned by Nodepool. They are not
/// legacy result torrents and must be shown even when the typed status failed.
export function isManagedGpuResult(response) {
  const result = response?.managed_gpu_result;
  return (
    result !== null &&
    typeof result === 'object' &&
    !Array.isArray(result) &&
    result.runtime_version === 'managed-function-gpu-v1'
  );
}
