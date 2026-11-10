// What a task row shows of its output. The list already carries each task's output,
// so a finished task can show its result without a second request.

export const OUTPUT_PREVIEW_CHARS = 1000;

// The preview of a task's output, or null when it has none. A cut never splits a
// surrogate pair, so the preview is always well-formed text.
export function taskOutputView(task, limit = OUTPUT_PREVIEW_CHARS) {
  const output = typeof task?.output === 'string' ? task.output : '';
  if (!output) return null;
  if (output.length <= limit) {
    return { text: output, truncated: false, totalChars: output.length };
  }
  let end = limit;
  const last = output.charCodeAt(end - 1);
  if (last >= 0xd800 && last <= 0xdbff) end -= 1;
  return { text: output.slice(0, end), truncated: true, totalChars: output.length };
}

export function taskOutputFilename(taskId) {
  return `${taskId}-output.txt`;
}
