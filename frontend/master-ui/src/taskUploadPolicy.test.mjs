import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

import { validateTaskUploadFile } from './taskUploadPolicy.mjs';

function file(name, size = 1024) {
  return { name, size };
}

describe('task upload policy', () => {
  it('accepts torrent task references and zip packages', () => {
    assert.equal(validateTaskUploadFile(file('seeded-task.torrent')), null);
    assert.equal(validateTaskUploadFile(file('legacy-task.zip')), null);
  });

  it('rejects missing, oversized, and unsupported task upload files', () => {
    assert.equal(validateTaskUploadFile(null), 'No file selected');
    assert.equal(
      validateTaskUploadFile(file('task.txt')),
      'Only .torrent or .zip task files are accepted',
    );
    assert.equal(
      validateTaskUploadFile(file('task.torrent', 501 * 1024 * 1024)),
      'File exceeds 500 MB limit. Use a smaller task file.',
    );
  });
});
