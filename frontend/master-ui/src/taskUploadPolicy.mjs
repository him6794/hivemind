export const MAX_TASK_UPLOAD_SIZE_BYTES = 500 * 1024 * 1024;

export function validateTaskUploadFile(file) {
  if (!file) return 'No file selected';
  const name = String(file.name || '').toLowerCase();
  if (!name.endsWith('.torrent') && !name.endsWith('.zip')) {
    return 'Only .torrent or .zip task files are accepted';
  }
  if (file.size > MAX_TASK_UPLOAD_SIZE_BYTES) {
    return 'File exceeds 500 MB limit. Use a smaller task file.';
  }
  return null;
}

export function clearTaskUploadInput(input) {
  if (input) {
    input.value = '';
  }
}
