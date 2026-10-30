// Download locations for the Windows applications.
//
// The release operator fills these in after a signed package is published:
//   url     https address of the package archive
//   version release version shown to visitors
//   sha256  SHA-256 of the same archive, copied from SHA256SUMS
// While url is empty, the download page says that no release is available and
// shows no link.
export const releaseDownloads = {
  master: { version: '', url: '', sha256: '' },
  worker: { version: '', url: '', sha256: '' },
};

export function isPublishedDownload(entry) {
  return Boolean(entry && typeof entry.url === 'string' && entry.url.startsWith('https://'));
}
