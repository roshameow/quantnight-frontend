import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { canGenerate, isGenerating, generationFailure } from '../src/utils/taskGeneration.js';

test('generation admission ignores legacy projections but blocks a current local request or archived task', () => {
  for (const status of ['pending', 'waiting', 'ready', 'generating', 'generation_unknown', 'generation_failed', 'running', 'paused']) {
    assert.equal(canGenerate({ status }), true);
    assert.equal(canGenerate({ status }, true), false);
  }
  assert.equal(canGenerate({ status: 'deactive' }), false);
  assert.equal(canGenerate(undefined), false);
  assert.equal(isGenerating({ status: 'generating' }), true); // diagnostic only
});

test('cards gate controls only on local in-flight work and mutation identity, not cached generation status', () => {
  const card = readFileSync(new URL('../src/components/TaskCards/RegularTaskCard.vue', import.meta.url), 'utf8');
  assert.match(card, /:disabled="!!taskMutationIssue\(task\) \|\| !canGenerate\(task, generationBusy \|\| operationBusy\)"/);
  assert.match(card, /:loading="generationBusy"/);
  assert.match(card, /:disabled="!!taskMutationIssue\(task\) \|\| generationBusy" @click="\$emit\('pause'\)"/);
  assert.match(card, /:disabled="!!taskMutationIssue\(task\) \|\| operationBusy \|\| generationBusy" @click="\$emit\('delete'\)"/);
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
