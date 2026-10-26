import test from 'node:test';
import assert from 'node:assert/strict';
import { taskStartBlockReason } from '../src/utils/taskDiagnostics.js';

const task = { _id: 'original', name: 'same-task', taskType: 'super', isRemote: false };

test('v8 start admission uses task identity/source and view-owned busy state, not control/PID/proof history', () => {
  for (const status of ['ready', 'running', 'launch_unknown', 'paused', 'completed', 'completed_with_errors',
    'generation_unknown', 'generating', 'syncing', 'stop_unconfirmed', 'unknown']) {
    assert.equal(taskStartBlockReason({ ...task, status }), '', status);
    assert.notEqual(taskStartBlockReason({ ...task, status }, { busy: true }), '');
    assert.notEqual(taskStartBlockReason({ ...task, status }, { stopping: true }), '');
  }
  assert.notEqual(taskStartBlockReason(null), '');
  assert.notEqual(taskStartBlockReason({ ...task, _id: null }), '');
  assert.notEqual(taskStartBlockReason({ ...task, status: 'deactive' }), '');
  assert.notEqual(taskStartBlockReason({ ...task, controlSource: 'remote_only' }), '');
  for (const controlSource of [null, undefined, 'local', 'old-unknown']) {
    assert.equal(taskStartBlockReason({ ...task, controlSource, isRemote: true }), '');
  }
});
