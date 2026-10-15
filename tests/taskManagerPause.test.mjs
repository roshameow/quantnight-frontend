import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { pathToFileURL } from 'node:url';
import { parse, compileScript } from '@vue/compiler-sfc';
import { createPinia, setActivePinia } from 'pinia';
import { useTaskStore, taskStopDisplayStatus, validStopReceipt, stopProofMillis } from '../src/stores/taskStore.js';

// Execute the actual TaskManager setup, with real Pinia/Vue/coordinator code.
// Only component imports and native/router/UI boundaries are replaced. No app,
// DB, network, SSH or real timers are required to test the pause command path.
const source = await readFile(new URL('../src/views/TaskManager.vue', import.meta.url), 'utf8');
const { descriptor } = parse(source);
const require = createRequire(import.meta.url);
const vueModule = pathToFileURL(require.resolve('vue/dist/vue.runtime.esm-bundler.js')).href;
const piniaModule = import.meta.resolve('pinia');
let instance = 0;
const deferred = () => {
  let resolve, reject;
  const promise = new Promise((res, rej) => { resolve = res; reject = rej; });
  return { promise, resolve, reject };
};
const tick = async () => { for (let i = 0; i < 8; i++) await Promise.resolve(); };
const task = (extra = {}) => ({ _id: { $oid: 'id-a' }, name: 'priority-a', taskType: 'priority',
  isRemote: true, status: 'running', mission_config: { rounds: { enabled: true } }, ...extra });

async function harness(initial = task()) {
  setActivePinia(createPinia());
  const store = useTaskStore();
  store.replaceTasks([initial]);
  const compiled = compileScript(descriptor, { id: `pause-${instance++}` }).content;
  const code = compiled
    .replace('createAutoPauseScheduler,', 'createAutoPauseScheduler as realAutoPauseScheduler,')
    .replace('ref, onMounted, onUnmounted, watch, computed', 'ref, watch, computed')
    .replaceAll('from "vue"', `from ${JSON.stringify(vueModule)}`)
    .replaceAll('from "pinia"', `from ${JSON.stringify(piniaModule)}`)
    .replace('"../stores/taskStore"', JSON.stringify(new URL('../src/stores/taskStore.js', import.meta.url).href))
    .replace('"../utils/taskProgress.js"', JSON.stringify(new URL('../src/utils/taskProgress.js', import.meta.url).href))
    .replace(/import \{ invoke \} from "@tauri-apps\/api\/core";/, 'const invoke = (...args) => hooks.invoke(...args);')
    .replace(/import \{ listen \} from "@tauri-apps\/api\/event";/, 'const listen = async () => () => {};')
    .replace(/import \{ useRoute, useRouter \} from "vue-router";/,
      'const useRoute = () => ({ fullPath: "/tasks" }); const useRouter = () => ({ push() {} });')
    .replace(/import \{ useMessage, useDialog \} from "naive-ui";/,
      'const useMessage = () => ({ error: (...args) => hooks.errors.push(args), success() {} }); const useDialog = () => ({});')
    .replace(/import (\w+) from "[^"\n]+\.vue";/g, 'const $1 = {};');
  const hooksCode = `export const hooks = { errors: [], invoke: null, unmount: null, timers: new Map(), nextTimer: 0 };
    const setTimeout = (callback, delay) => { const id = ++hooks.nextTimer; hooks.timers.set(id, { callback, delay }); return id; };
    const clearTimeout = (id) => { hooks.timers.delete(id); };
    const createAutoPauseScheduler = (options) => realAutoPauseScheduler({ ...options, setTimeoutFn: setTimeout, clearTimeoutFn: clearTimeout });
    const onMounted = () => {};
    const onUnmounted = (fn) => { hooks.unmount = fn; };
  `;
  const module = await import(`data:text/javascript;base64,${Buffer.from(hooksCode + code + `\n// ${instance}`).toString('base64')}`);
  const calls = [], lists = [], command = deferred();
  let backgroundError = null;
  module.hooks.invoke = (name, args) => {
    calls.push({ name, args, status: store.tasks[0]?.status });
    if (name === 'pause_task') return command.promise;
    if (name === 'get_all_tasks') { const request = deferred(); lists.push(request); return request.promise; }
    if (name === 'refresh_task_progress') return backgroundError ? Promise.reject(backgroundError) : Promise.resolve();
    assert.fail(`unexpected native command: ${name}`);
  };
  const state = module.default.setup({}, { expose() {} });
  return { store, state, hooks: module.hooks, command, lists, calls, stop: () => module.hooks.unmount(),
    failBackground: () => { backgroundError = new Error('background progress timeout'); } };
}

function seedCounts(store) {
  const counts = { success: 7, error: 2, total: 60, priority_success: 7, priority_error: 2, priority_total: 60 };
  const metadata = { collection: 'priority-a', task_id: 'id-a', is_remote: true, updated_at: 100,
    pool_id: 'pool-a', mode: 'per_root', status: 'active', stats_confirmed: true,
    unresolved: 51, root_count: 3, stage_distribution: { '0': 1, '1': 2, '2': 0, '3': 0 },
    source_tasks: ['regular-a'], source_summaries: [{ source_task: 'regular-a', source_remote: false,
      root_count: 3, stage_distribution: { '0': 1, '1': 2, '2': 0, '3': 0 } }] };
  assert.equal(store.updateProgress({ ...counts, collection: 'priority-a', task_id: 'id-a', is_remote: true,
    updated_at: 100, priority_campaign: metadata }), true);
  return store.taskProgressMap['priority-a'];
}

const proofAt = Date.now();
const receipt = (extra = {}) => ({ confirmed: true, taskId: 'id-a', taskName: 'priority-a',
  isRemote: true, stopConfirmedAt: proofAt, ...extra });
const stopStatus = h => taskStopDisplayStatus(h.store.tasks[0], h.store.taskStopOperationMap['priority-a']);

test('real handler shows stopping until a matching proof receipt, commits before refresh; Mongo timeout keeps proof/counts', async () => {
  const h = await harness();
  try {
    const oldProgress = seedCounts(h.store);
    const saved = JSON.parse(JSON.stringify(oldProgress));
    const pausing = h.state.pauseTask(h.store.tasks[0]);
    assert.equal(h.store.tasks[0].status, 'running');
    assert.equal(stopStatus(h), 'stopping');
    assert.deepEqual(h.calls[0], { name: 'pause_task', args: { taskName: 'priority-a', taskId: 'id-a', isRemote: true }, status: 'running' });
    assert.equal(h.lists.length, 0);
    h.command.resolve(receipt());
    await tick();
    assert.equal(h.store.tasks[0].status, 'paused');
    assert.equal(stopStatus(h), 'paused');
    assert.equal(h.store.tasks[0].stopConfirmedAt, proofAt);
    assert.equal(h.calls[1].name, 'get_all_tasks');
    assert.equal(h.calls[1].status, 'paused');
    assert.equal(h.store.taskProgressMap['priority-a'], oldProgress);
    assert.equal(h.hooks.timers.size, 0);
    h.lists[0].reject(new Error('local Mongo server selection timeout'));
    await pausing;
    assert.equal(stopStatus(h), 'paused');
    assert.equal(h.hooks.errors.length, 1);
    assert.deepEqual(JSON.parse(JSON.stringify(h.store.taskProgressMap['priority-a'])), saved);
  } finally { h.stop(); }
});

test('native failure or proof-verified/DB-writeback failure remains unconfirmed, never paused and never retried automatically', async () => {
  for (const error of ['pause_graceful failed', '执行退出已核验，但控制记录回写超时；停止状态未完成确认']) {
    const h = await harness();
    try {
      const progress = seedCounts(h.store);
      const pausing = h.state.pauseTask(h.store.tasks[0]);
      assert.equal(stopStatus(h), 'stopping');
      h.command.reject(new Error(error));
      await assert.rejects(pausing, new RegExp(error));
      assert.equal(h.store.tasks[0].status, 'running');
      assert.equal(stopStatus(h), '停止状态未确认');
      assert.equal(h.store.taskStopOperationMap['priority-a'].status, 'stop_unconfirmed');
      assert.equal(h.store.taskProgressMap['priority-a'], progress);
      assert.equal(h.lists.length, 0);
      await h.state.autoPause.reconcile();
      assert.equal(h.calls.length, 1);
      assert.equal(h.hooks.timers.size, 0);
    } finally { h.stop(); }
  }
});

test('resolved void, stdout claims and malformed/mismatched receipts fail closed without a paused commit', async () => {
  for (const response of [undefined, null, true, '停止成功', { confirmed: true },
    receipt({ confirmed: false }), receipt({ confirmed: 'true' }), receipt({ taskId: { $oid: 'id-a' } }), receipt({ taskId: 'other-id' }),
    receipt({ taskName: 'other' }), receipt({ isRemote: false }), receipt({ isRemote: 'true' }),
    receipt({ stopConfirmedAt: null }), receipt({ stopConfirmedAt: -1 }), receipt({ stopConfirmedAt: '123' }),
    receipt({ stopConfirmedAt: NaN }), receipt({ stopConfirmedAt: Infinity })]) {
    const h = await harness();
    try {
      const pausing = h.state.pauseTask(h.store.tasks[0]);
      h.command.resolve(response);
      await assert.rejects(pausing, /停止回执未确认/);
      assert.equal(h.store.tasks[0].status, 'running');
      assert.equal(stopStatus(h), '停止状态未确认');
      assert.equal(h.lists.length, 0);
      assert.equal(h.hooks.timers.size, 0);
    } finally { h.stop(); }
  }
});

test('bounded client confirmation timeout stays unconfirmed even if native resolves with proof later', async () => {
  const h = await harness();
  try {
    const pausing = h.state.pauseTask(h.store.tasks[0]);
    const [timer] = h.hooks.timers.values();
    assert.equal(timer.delay, 120000);
    timer.callback();
    await assert.rejects(pausing, /停止确认超时/);
    assert.equal(stopStatus(h), '停止状态未确认');
    h.command.resolve(receipt());
    await tick();
    assert.equal(h.store.tasks[0].status, 'running');
    assert.equal(stopStatus(h), '停止状态未确认');
    assert.equal(h.lists.length, 0);
  } finally { h.stop(); }
});

test('pre/during-command reads cannot overwrite proof; later active list/root snapshots cannot override operation display', async () => {
  const h = await harness();
  try {
    seedCounts(h.store);
    const older = h.state.fetchTasks();
    const pausing = h.state.pauseTask(h.store.tasks[0]);
    h.lists[0].resolve([task({ status: 'active' })]);
    assert.equal(await older, false);
    assert.equal(stopStatus(h), 'stopping');
    const during = h.state.fetchTasks();
    h.command.resolve(receipt());
    await tick();
    assert.equal(stopStatus(h), 'paused');
    assert.equal(h.lists.length, 3);
    h.lists[2].resolve([task({ status: 'active' })]); // late root/control snapshot lacks proof
    await pausing;
    assert.equal(stopStatus(h), 'paused'); // current-action receipt survives identical ID/host list reload
    h.lists[1].resolve([task({ status: 'active' })]);
    assert.equal(await during, false);
    h.store.updateProgressError({ collection: 'priority-a', task_id: 'id-a', is_remote: true,
      updated_at: 101, error: 'metadata timeout' });
    h.store.markProgressStale({ scope: 'all', updated_at: 102, error: 'discovery timeout' });
    assert.equal(stopStatus(h), 'paused');
    assert.equal(h.store.taskProgressMap['priority-a'].success, 7);
    const refresh = h.state.fetchTasks();
    h.failBackground();
    h.lists[3].resolve([task({ status: 'paused' })]);
    await refresh;
    await tick();
    assert.equal(stopStatus(h), 'paused');
  } finally { h.stop(); }
});

test('same-name replacement ID/host prunes pending operation; old receipt cannot mark replacement paused', async () => {
  for (const replacement of [task({ _id: 'new-id' }), task({ isRemote: false })]) {
    const h = await harness();
    try {
      const pausing = h.state.pauseTask(h.store.tasks[0]);
      h.store.replaceTasks([replacement]);
      assert.equal(h.store.taskStopOperationMap['priority-a'], undefined);
      h.command.resolve(receipt());
      await tick();
      assert.equal(h.store.tasks[0].status, 'running');
      assert.equal(stopStatus(h), null);
      h.lists[0].reject(new Error('refresh timeout'));
      await pausing;
      assert.equal(h.store.tasks[0].status, 'running');
    } finally { h.stop(); }
  }
});

test('capture mutable arguments, reject duplicate pending stop, and do not commit after unmount', async () => {
  const h = await harness();
  const mutableTarget = task();
  try {
    const pausing = h.state.pauseTask(mutableTarget);
    assert.equal(await h.state.pauseTask(mutableTarget), false);
    assert.equal(h.calls.length, 1);
    mutableTarget._id.$oid = 'replacement';
    mutableTarget.isRemote = false;
    mutableTarget.name = 'other';
    h.command.resolve(receipt());
    await tick();
    assert.equal(stopStatus(h), 'paused');
    h.lists[0].resolve([task({ status: 'paused' })]);
    await pausing;
  } finally { h.stop(); }
  const unmounted = await harness();
  const pausing = unmounted.state.pauseTask(unmounted.store.tasks[0]);
  unmounted.stop();
  unmounted.command.resolve(receipt());
  await pausing;
  assert.equal(unmounted.store.tasks[0].status, 'running');
  assert.equal(stopStatus(unmounted), '停止状态未确认');
  assert.equal(unmounted.lists.length, 0);
});

test('store itself rejects no-proof confirmations; valid BSON persisted proof survives remount but not wrong ID/host', async () => {
  const h = await harness();
  try {
    const progress = seedCounts(h.store);
    const operation = h.store.beginTaskStop(task());
    for (const target of [null, {}, task({ _id: undefined }), task({ _id: 'wrong' }),
      task({ name: 'other' }), task({ isRemote: false }), task({ isRemote: undefined })]) {
      assert.equal(h.store.confirmTaskPaused(target, receipt(), operation), false);
      assert.equal(h.store.tasks[0].status, 'running');
    }
    assert.equal(h.store.confirmTaskPaused(task(), undefined, operation), false);
    assert.equal(h.store.confirmTaskPaused(task(), receipt(), { ...operation, generation: -1 }), false);
    assert.equal(h.store.confirmTaskPaused(task(), receipt(), operation), true);
    assert.equal(h.store.taskProgressMap['priority-a'], progress);
    h.store.removeTask('priority-a');
    assert.equal(h.store.confirmTaskPaused(task(), receipt(), operation), false);
    for (const timestamp of [proofAt, new Date(proofAt), new Date(proofAt).toISOString(),
      { $date: new Date(proofAt).toISOString() }, { $date: { $numberLong: String(proofAt) } }]) {
      const persisted = task({ status: 'paused', stopConfirmedAt: timestamp,
        stopConfirmedTaskId: 'id-a', stopConfirmedRemote: true });
      assert.equal(taskStopDisplayStatus(persisted), 'paused');
      assert.equal(taskStopDisplayStatus({ ...persisted, stopConfirmedTaskId: 'other' }), '暂停（运行状态未确认）');
      assert.equal(taskStopDisplayStatus({ ...persisted, stopConfirmedRemote: false }), '暂停（运行状态未确认）');
      assert.equal(taskStopDisplayStatus({ ...persisted, _id: undefined, stopConfirmedTaskId: undefined }), '暂停（运行状态未确认）');
    }
    for (const value of [null, undefined, false, 0, -1, NaN, Infinity, '', 'today', { $date: 'invalid' }]) {
      assert.equal(stopProofMillis(value), null);
      assert.equal(taskStopDisplayStatus(task({ status: 'paused', stopConfirmedAt: value,
        stopConfirmedTaskId: 'id-a', stopConfirmedRemote: true })), '暂停（运行状态未确认）');
    }
    assert.equal(validStopReceipt(task(), receipt()), true);
  } finally { h.stop(); }
});

test('explicit start clears prior action and persisted local proof without clearing real counts', async () => {
  const h = await harness();
  try {
    const progress = seedCounts(h.store);
    const operation = h.store.beginTaskStop(task());
    h.store.confirmTaskPaused(task(), receipt(), operation);
    assert.equal(stopStatus(h), 'paused');
    assert.equal(h.store.clearTaskStop(task({ _id: 'wrong' })), false);
    assert.equal(h.store.clearTaskStop(task()), true);
    assert.equal(stopStatus(h), '暂停（运行状态未确认）');
    assert.equal(h.store.taskProgressMap['priority-a'], progress);
    h.store.replaceTasks([task({ status: 'running' })]);
    assert.equal(stopStatus(h), null);
  } finally { h.stop(); }
});

test('actual generic auto-pause timer does not retry after an uncertain manual stop', async () => {
  const h = await harness(task({ mission_config: {} }));
  try {
    h.store.updateProgress({ collection: 'priority-a', task_id: 'id-a', is_remote: true,
      updated_at: 100, success: 10, error: 0, total: 10 });
    h.state.autoPause.reconcile();
    const timer = [...h.hooks.timers.values()].find(timer => timer.delay === 300000);
    assert.ok(timer);
    const pausing = h.state.pauseTask(h.store.tasks[0]);
    h.command.reject(new Error('DB writeback outcome unknown'));
    await assert.rejects(pausing, /outcome unknown/);
    await timer.callback();
    assert.equal(h.calls.filter(call => call.name === 'pause_task').length, 1);
    assert.equal(stopStatus(h), '停止状态未确认');
  } finally { h.stop(); }
});

test('uncertain operation rejects old persisted proof but accepts a later real matching proof', async () => {
  const h = await harness();
  try {
    const target = h.store.tasks[0];
    const operation = h.store.beginTaskStop(target);
    h.store.markTaskStopUnconfirmed(target, operation, 'DB outcome unknown');
    const persisted = task({ status: 'paused', stopConfirmedAt: operation.startedAt - 1,
      stopConfirmedTaskId: 'id-a', stopConfirmedRemote: true });
    h.store.replaceTasks([persisted]);
    assert.equal(stopStatus(h), '停止状态未确认');
    h.store.replaceTasks([{ ...persisted, stopConfirmedAt: operation.startedAt + 1 }]);
    assert.equal(stopStatus(h), 'paused');
    assert.equal(h.store.taskStopOperationMap['priority-a'], undefined);
  } finally { h.stop(); }
});

test('actual successful start handler clears previous verified pause so stale active list cannot leave a false paused badge', async () => {
  const h = await harness();
  try {
    const target = h.store.tasks[0];
    const operation = h.store.beginTaskStop(target);
    h.store.confirmTaskPaused(target, receipt(), operation);
    const originalInvoke = h.hooks.invoke;
    h.hooks.invoke = (name, args) => name === 'start_priority_task' ? Promise.resolve() : originalInvoke(name, args);
    await h.state.handlePriorityStartTask({ taskName: 'priority-a', config: {}, isRemote: true });
    assert.equal(h.store.taskStopOperationMap['priority-a'], undefined);
    assert.equal(stopStatus(h), '暂停（运行状态未确认）');
    h.lists[0].resolve([task({ status: 'running' })]);
    await tick();
    assert.equal(stopStatus(h), null);
  } finally { h.stop(); }
});
