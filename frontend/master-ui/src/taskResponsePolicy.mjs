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
