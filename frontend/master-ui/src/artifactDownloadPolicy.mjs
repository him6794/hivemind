function fallbackArtifactFilename(taskId) {
  const safeTaskId = String(taskId || '').trim() || 'task';
  return `${safeTaskId}-artifact.bin`;
}

function unquoteDispositionValue(value) {
  const trimmed = String(value || '').trim();
  if (trimmed.startsWith('"') && trimmed.endsWith('"') && trimmed.length >= 2) {
    return trimmed.slice(1, -1).replace(/\\(["\\])/g, '$1');
  }
  return trimmed;
}

function filenameParameter(contentDisposition) {
  const parts = String(contentDisposition || '').split(';').slice(1);
  for (const part of parts) {
    const separator = part.indexOf('=');
    if (separator === -1) continue;
    const name = part.slice(0, separator).trim().toLowerCase();
    if (name === 'filename') {
      return unquoteDispositionValue(part.slice(separator + 1));
    }
  }
  return '';
}

function isSafeDownloadFilename(filename) {
  const value = String(filename || '').trim();
  if (!value || value === '.' || value === '..') return false;
  if (value.includes('/') || value.includes('\\')) return false;
  if (/^[a-zA-Z]:/.test(value)) return false;
  if (/[\x00-\x1f\x7f]/.test(value)) return false;
  if (/^\.+$/.test(value)) return false;
  return true;
}

export function artifactFilenameFromContentDisposition(contentDisposition, taskId) {
  const filename = filenameParameter(contentDisposition);
  return isSafeDownloadFilename(filename) ? filename.trim() : fallbackArtifactFilename(taskId);
}
