// Download locations for the Windows applications.
//
// The source keeps these entries empty. A release build supplies the published values
// through NEXT_PUBLIC_RELEASE_DOWNLOADS, a JSON object with one entry per application:
//   url     https address of the installer
//   version release version shown to visitors
//   sha256  SHA-256 of the same installer, copied from its .sha256 file
// While url is empty, the download page says that no release is available and
// shows no link.
export const releaseDownloads = {
  master: { version: '', url: '', sha256: '' },
  worker: { version: '', url: '', sha256: '' },
};

function buildTimeDownloads() {
  try {
    return JSON.parse(process.env.NEXT_PUBLIC_RELEASE_DOWNLOADS || '{}');
  } catch {
    return {};
  }
}

export function downloadFor(id) {
  return buildTimeDownloads()[id] ?? releaseDownloads[id];
}

export function isPublishedDownload(entry) {
  return Boolean(entry && typeof entry.url === 'string' && entry.url.startsWith('https://'));
}
