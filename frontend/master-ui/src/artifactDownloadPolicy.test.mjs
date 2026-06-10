import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

import { artifactFilenameFromContentDisposition } from './artifactDownloadPolicy.mjs';

describe('artifact download filename policy', () => {
  it('falls back for unsafe content-disposition filenames', () => {
    assert.equal(
      artifactFilenameFromContentDisposition('attachment; filename="../secrets.txt"', 'task-123'),
      'task-123-artifact.bin',
    );

    assert.equal(
      artifactFilenameFromContentDisposition('attachment; filename="..\\secrets.txt"', 'task-123'),
      'task-123-artifact.bin',
    );

    assert.equal(
      artifactFilenameFromContentDisposition('attachment; filename="C:\\Users\\user\\secrets.txt"', 'task-123'),
      'task-123-artifact.bin',
    );

    assert.equal(
      artifactFilenameFromContentDisposition('attachment; filename="."', 'task-123'),
      'task-123-artifact.bin',
    );
  });

  it('uses the real filename parameter instead of lookalike parameters', () => {
    assert.equal(
      artifactFilenameFromContentDisposition(
        'attachment; x-filename="../secrets.txt"; filename="result.zip"',
        'task-123',
      ),
      'result.zip',
    );
  });
});
