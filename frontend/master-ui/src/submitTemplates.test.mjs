import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { SUBMIT_TEMPLATES } from './submitTemplates.mjs';
import { EXAMPLES } from '../../src/lib/docs/examples.mjs';
import { t } from './locale.mjs';

const normalize = (text) => text.replace(/\r\n/g, '\n').replace(/\s+$/, '');
const repoFile = (base, extension) =>
  normalize(readFileSync(new URL(`../../../templates/hcode/${base}.${extension}`, import.meta.url), 'utf8'));

test('every template is a program the repository already verifies', () => {
  assert.ok(SUBMIT_TEMPLATES.length >= 5);
  for (const template of SUBMIT_TEMPLATES) {
    assert.ok(Boolean(template.docs) !== Boolean(template.repo), `${template.id} names exactly one origin`);
    if (template.docs) {
      const example = EXAMPLES.find((entry) => entry.id === template.docs);
      assert.ok(example, `${template.id}: no documentation example ${template.docs}`);
      assert.equal(normalize(template.source), normalize(example.source), `${template.id} source`);
      assert.equal(normalize(template.input), normalize(example.input), `${template.id} input`);
    } else {
      assert.equal(normalize(template.source), repoFile(template.repo, 'hmf'), `${template.id} source`);
      assert.equal(normalize(template.input), repoFile(template.repo, 'input.json'), `${template.id} input`);
    }
  }
});

test('template ids are unique and every input is valid JSON', () => {
  const ids = SUBMIT_TEMPLATES.map((template) => template.id);
  assert.equal(new Set(ids).size, ids.length);
  for (const template of SUBMIT_TEMPLATES) {
    assert.doesNotThrow(() => JSON.parse(template.input), template.id);
    assert.ok(template.source.trim().length > 0, template.id);
  }
});

test('every template is described in both languages', () => {
  for (const template of SUBMIT_TEMPLATES) {
    for (const field of ['label', 'summary']) {
      assert.ok(template[field].en.trim(), `${template.id}.${field}.en`);
      assert.ok(template[field].zh.trim(), `${template.id}.${field}.zh`);
      assert.equal(t('zh', template[field]), template[field].zh);
      assert.equal(t('en', template[field]), template[field].en);
    }
  }
});

test('templates stay inside the limits a task is held to', () => {
  for (const template of SUBMIT_TEMPLATES) {
    assert.ok(new TextEncoder().encode(template.source).length <= 64 * 1024, template.id);
    assert.ok(new TextEncoder().encode(template.input).length <= 1024 * 1024, template.id);
  }
});
