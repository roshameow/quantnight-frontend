import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { canGenerate, isGenerating, generationBusyOrUnknown } from '../src/utils/taskGeneration.js';

test('ordinary generation disables click immediately and across refreshed state', () => {
  assert.equal(canGenerate({ status: 'pending' }), true);
  assert.equal(canGenerate({ status: 'pending' }, true), false);
  assert.equal(isGenerating({ status: 'pending' }, true), true);
  for (const status of ['generating', 'generation_unknown', 'running', 'deactive', 'paused']) {
    assert.equal(canGenerate({ status }), false);
  }
  assert.equal(isGenerating({ status: 'generating' }), true);
  assert.equal(generationBusyOrUnknown({ status: 'generation_unknown' }), true);
  assert.equal(canGenerate({ status: 'generation_failed' }), true);
});

test('card and manager bind both in-flight and persisted generation state', () => {
  const card = readFileSync(new URL('../src/components/TaskCards/RegularTaskCard.vue', import.meta.url), 'utf8');
  assert.match(card, /:disabled="!canGenerate\(task, generationBusy\)"/);
  assert.match(card, /:loading="isGenerating\(task, generationBusy\)"/);
  const manager = readFileSync(new URL('../src/views/TaskManager.vue', import.meta.url), 'utf8');
  assert.match(manager, /generationRequests\.value\[key\] = true/);
  assert.match(manager, /listen\('task-registry-changed'/);
  assert.match(manager, /registryUnlisten\?\.\(\)/);
});
