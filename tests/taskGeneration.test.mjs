import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { canGenerate, isGenerating, generationBusyOrUnknown, generationFailure } from '../src/utils/taskGeneration.js';

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
  assert.match(card, /:disabled="!!taskMutationIssue\(task\) \|\| !canGenerate\(task, generationBusy\)"/);
  assert.match(card, /:loading="isGenerating\(task, generationBusy\)"/);
  const manager = readFileSync(new URL('../src/views/TaskManager.vue', import.meta.url), 'utf8');
  assert.match(manager, /generationRequests\.value\[key\] = true/);
  assert.match(manager, /listen\('task-registry-changed'/);
  assert.match(manager, /registryUnlisten\?\.\(\)/);
});


test('generation failure is an operation result, not pending control or arbitrary lastError', () => {
  const failed = { status: 'pending', generation: { id: 'attempt-1', phase: 'generation_failed' }, lastError: 'original writer error' };
  const before = JSON.stringify(failed);
  assert.deepEqual(generationFailure(failed), { status: 'generation_failed', error: 'original writer error' });
  assert.equal(canGenerate(failed), true);
  assert.equal(generationBusyOrUnknown(failed), false);
  assert.equal(JSON.stringify(failed), before);
  assert.deepEqual(generationFailure({ status: 'generation_failed', lastError: 'legacy error' }),
    { status: 'generation_failed', error: 'legacy error' });
  assert.deepEqual(generationFailure({ status: 'generation_failed' }), { status: 'generation_failed', error: '' });
  assert.equal(generationFailure(failed, true), null);
  for (const status of ['generating', 'generation_unknown']) assert.equal(generationFailure({ ...failed, status }), null);
  for (const phase of ['writing', 'preparing', 'prepared', 'complete', 'generation_unknown']) {
    assert.equal(generationFailure({ ...failed, generation: { id: 'attempt-2', phase } }), null);
    assert.equal(generationFailure({ ...failed, status: 'generation_failed', generation: { phase } }), null);
  }
  for (const status of ['pending', 'ready', 'running']) {
    assert.equal(generationFailure({ status, lastError: 'unrelated or stale error' }), null);
  }
  assert.equal(generationFailure(null), null);
});
