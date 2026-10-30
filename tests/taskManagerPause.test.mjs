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
const source = await readFile(process.env.QUANTNIGHT_TEST_MANAGER_SOURCE ?? new URL('../src/views/TaskManager.vue', import.meta.url), 'utf8');
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

async function harness(initial = task(), options = {}) {
  setActivePinia(createPinia());
  const store = useTaskStore();
  store.replaceTasks([initial]);
  const compiled = compileScript(descriptor, { id: `pause-${instance++}` }).content;
  const code = compiled
    .replace('createTaskProgressLifecycle,', 'createTaskProgressLifecycle as realTaskProgressLifecycle,')
    .replaceAll('performance.now()', 'hooks.now')
    .replace('ref, onMounted, onUnmounted, watch, computed', 'ref, watch, computed')
    .replaceAll('from "vue"', `from ${JSON.stringify(vueModule)}`)
    .replaceAll('from "pinia"', `from ${JSON.stringify(piniaModule)}`)
    .replace('"../stores/taskStore"', JSON.stringify(new URL('../src/stores/taskStore.js', import.meta.url).href))
    .replace('"../utils/taskProgress.js"', JSON.stringify(new URL('../src/utils/taskProgress.js', import.meta.url).href))
    .replaceAll("'../utils/taskDiagnostics.js'", JSON.stringify(new URL('../src/utils/taskDiagnostics.js', import.meta.url).href))
    .replace("'../utils/taskGeneration.js'", JSON.stringify(new URL('../src/utils/taskGeneration.js', import.meta.url).href))
    .replace(/import \{ invoke \} from "@tauri-apps\/api\/core";/, 'const invoke = (...args) => hooks.invoke(...args);')
    .replace(/import \{ listen \} from "@tauri-apps\/api\/event";/, 'const listen = (...args) => hooks.listen(...args);')
    .replace(/import \{ useRoute, useRouter \} from "vue-router";/,
      'const useRoute = () => hooks.route; const useRouter = () => hooks.router;')
    .replace(/import \{ useMessage, useDialog \} from "naive-ui";/,
      'const useMessage = () => ({ error: (...args) => hooks.errors.push(args), success: text => hooks.notices.push({ type: "success", text }), warning: text => hooks.notices.push({ type: "warning", text }), loading: (text, options) => { const entry = { text, options, destroyed: false }; hooks.messages.push(entry); return { destroy: () => { entry.destroyed = true; } }; } }); const useDialog = () => ({ warning: options => { hooks.dialog = options; hooks.dialogClosed = false; return { destroy: () => { hooks.dialogClosed = true; } }; } });')
    .replace(/import (\w+) from "[^"\n]+\.vue";/g, 'const $1 = {};');
  const hooksCode = `export const hooks = { errors: [], messages: [], notices: [], invoke: null, unmount: null, timers: new Map(), nextTimer: 0, now: 0, intervals: new Map(), nextInterval: 0 };
    const setTimeout = (callback, delay) => { const id = ++hooks.nextTimer; hooks.timers.set(id, { callback, delay, at: hooks.now + delay }); return id; };
    const clearTimeout = (id) => { hooks.timers.delete(id); };
    const createTaskProgressLifecycle = (options) => realTaskProgressLifecycle({ ...options,
      setIntervalFn: (callback, delay) => { const id = ++hooks.nextInterval; hooks.intervals.set(id, { callback, delay }); return id; },
      clearIntervalFn: id => hooks.intervals.delete(id) });
    const onMounted = (fn) => { hooks.mount = fn; };
    const onUnmounted = (fn) => { hooks.unmount = fn; };
  `;
  const module = await import(`data:text/javascript;base64,${Buffer.from(hooksCode + code + `\n// ${instance}`).toString('base64')}`);
  const calls = [], lists = [], probes = [], command = deferred();
  module.hooks.handlers = new Map(); module.hooks.cleaned = [];
  module.hooks.listen = async (name, handler) => {
    module.hooks.handlers.set(name, handler);
    return () => { module.hooks.handlers.delete(name); module.hooks.cleaned.push(name); };
  };
  let backgroundError = null;
  module.hooks.invoke = (name, args) => {
    calls.push({ name, args, status: store.tasks[0]?.status });
    if (name === 'frontend_ready') return Promise.resolve();
    if (name === 'pause_task') return command.promise;
    if (name === 'check_task_status') { const probe = deferred(); probes.push(probe); return probe.promise; }
    if (name === 'get_all_tasks') { const request = deferred(); lists.push(request); return request.promise; }
    if (name === 'refresh_task_progress') return backgroundError ? Promise.reject(backgroundError) : Promise.resolve();
    assert.fail(`unexpected native command: ${name}`);
  };
  module.hooks.route = options.route ?? { fullPath: '/tasks', query: {} };
  module.hooks.router = options.router ?? { push() {}, replace() {} };
  const state = module.default.setup({}, { expose() {} });
  return { store, state, hooks: module.hooks, command, lists, probes, calls, stop: () => module.hooks.unmount(),
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
    assert.deepEqual(h.calls[0], { name: 'pause_task', args: { taskName: 'priority-a', taskId: 'id-a', isRemote: true, requestId: h.store.taskStopOperationMap['priority-a'].requestId }, status: 'running' });
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
      assert.equal(stopStatus(h), /超时/.test(error) ? '停止确认超时' : '停止执行失败');
      assert.equal(h.store.taskStopOperationMap['priority-a'].status, 'stop_unconfirmed');
      assert.equal(h.store.taskProgressMap['priority-a'], progress);
      assert.equal(h.lists.length, 0);
      await advanceJoinTime(h, 3600000);
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
      await assert.rejects(pausing, /暂停回执校验失败/);
      assert.equal(h.store.tasks[0].status, 'running');
      assert.equal(stopStatus(h), '停止回执校验失败');
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
    assert.equal(stopStatus(h), '停止确认超时');
    h.command.resolve(receipt());
    await tick();
    assert.equal(h.store.tasks[0].status, 'running');
    assert.equal(stopStatus(h), '停止确认超时');
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
  assert.equal(stopStatus(unmounted), '停止回执核验中断');
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
      assert.equal(taskStopDisplayStatus({ ...persisted, stopConfirmedTaskId: 'other' }), '暂停标记无有效停止凭据');
      assert.equal(taskStopDisplayStatus({ ...persisted, stopConfirmedRemote: false }), '暂停标记无有效停止凭据');
      assert.equal(taskStopDisplayStatus({ ...persisted, _id: undefined, stopConfirmedTaskId: undefined }), '暂停标记无有效停止凭据');
    }
    for (const value of [null, undefined, false, 0, -1, NaN, Infinity, '', 'today', { $date: 'invalid' }]) {
      assert.equal(stopProofMillis(value), null);
      assert.equal(taskStopDisplayStatus(task({ status: 'paused', stopConfirmedAt: value,
        stopConfirmedTaskId: 'id-a', stopConfirmedRemote: true })), '暂停标记无有效停止凭据');
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
    assert.equal(stopStatus(h), '暂停标记无有效停止凭据');
    assert.equal(h.store.taskProgressMap['priority-a'], progress);
    h.store.replaceTasks([task({ status: 'running' })]);
    assert.equal(stopStatus(h), null);
  } finally { h.stop(); }
});

test('full progress never retries an uncertain manual stop after time advances', async () => {
  const h = await harness(task({ mission_config: {} }));
  try {
    h.store.updateProgress({ collection: 'priority-a', task_id: 'id-a', is_remote: true,
      updated_at: 100, success: 10, error: 0, total: 10 });
    await tick();
    assert.equal(h.hooks.timers.size, 0);
    const pausing = h.state.pauseTask(h.store.tasks[0]);
    h.command.reject(new Error('DB writeback outcome unknown'));
    await assert.rejects(pausing, /outcome unknown/);
    await advanceJoinTime(h, 3600000);
    assert.equal(h.calls.filter(call => call.name === 'pause_task').length, 1);
    assert.equal(stopStatus(h), '停止执行失败');
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
    assert.equal(stopStatus(h), '停止执行失败');
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
    await h.state.openStartModal(h.store.tasks[0]);
    await h.state.handlePriorityStartTask({ taskName: 'priority-a', config: {}, isRemote: true });
    assert.equal(h.store.taskStopOperationMap['priority-a'], undefined);
    assert.equal(stopStatus(h), '暂停标记无有效停止凭据');
    h.lists[0].resolve([task({ status: 'running' })]);
    await tick();
    assert.equal(stopStatus(h), null);
  } finally { h.stop(); }
});


const regularTask = (extra = {}) => task({ taskType: 'regular', mission_config: { max_concurrent: 3 }, ...extra });
const positivePane = '✅ Pane 0 (pid=21733) is running: python3.12';
const bashPane = '❌ Pane 0 (pid=21733) is running: bash';



test('native stdout contract only confirms recognized Python/uv, shell-only is missing consumer, errors remain unknown', async () => {
  const h = await harness(regularTask());
  try {
    for (const [output, expected] of [[positivePane, 'running'],
      ['✅ Pane 0 (pid=21733) is running: uv', 'running'],
      [bashPane, 'not_running'], [bashPane + '\n' + positivePane, 'running'],
      ['', 'unknown'], ['running', 'unknown'], ['paused', 'unknown'],
      ['❌ Session task 不存在 (local)', 'unknown'],
      ['Permission denied', 'unknown'], ['bash: source: No such file or directory', 'unknown'],
      [positivePane + '\nPermission denied', 'unknown'],
      ['✅ Pane 0 (pid=21733) is running: uvicorn', 'unknown'],
      [positivePane + ' Permission denied', 'unknown'], [bashPane + ' permission denied', 'unknown'], [null, 'unknown']]) {
      const check = h.state.checkTaskStatus(h.store.tasks[0]);
      assert.equal(h.state.taskRuntimeStatus.value['priority-a'].state, 'checking');
      h.probes.at(-1).resolve(output);
      await check;
      assert.equal(h.state.taskRuntimeStatus.value['priority-a'].state, expected, String(output));
    }
    for (const error of ['Native status command permission denied', 'script_config startup failed', 'SSH timeout']) {
      const check = h.state.checkTaskStatus(h.store.tasks[0]);
      h.probes.at(-1).reject(new Error(error));
      await check;
      assert.equal(h.state.taskRuntimeStatus.value['priority-a'].state, 'unknown');
      assert.match(h.state.taskRuntimeStatus.value['priority-a'].error, new RegExp(error));
    }
  } finally { h.stop(); }
});

test('regular runtime captures primitive ID/target, rejects replacement ID/host/deletion/control changes and away-back results', async () => {
  for (const replacement of [regularTask({ _id: 'new-id' }), regularTask({ isRemote: false }),
    regularTask({ status: 'paused' }), regularTask({ status: 'ready' }), null]) {
    for (const outcome of ['resolve', 'reject']) {
      const h = await harness(regularTask());
      try {
        const mutable = regularTask();
        const check = h.state.checkTaskStatus(mutable);
        mutable._id.$oid = 'mutated'; mutable.isRemote = false; mutable.name = 'other';
        assert.equal(typeof h.calls[0].args.requestId, 'string');
        assert.deepEqual(h.calls[0].args, { taskName: 'priority-a', taskId: 'id-a', isRemote: true, requestId: h.calls[0].args.requestId });
        h.store.replaceTasks(replacement ? [replacement] : []);
        assert.equal(h.state.taskRuntimeStatus.value['priority-a'], undefined);
        h.store.replaceTasks([regularTask()]); // same name/ID/target again is still a new lifecycle
        if (outcome === 'resolve') h.probes[0].resolve(positivePane);
        else h.probes[0].reject(new Error('late permission error'));
        await check;
        assert.equal(h.state.taskRuntimeStatus.value['priority-a'], undefined);
        assert.equal(h.store.tasks[0].status, 'running');
      } finally { h.stop(); }
    }
  }
});

test('bounded runtime timeout clears prior positive, late native success cannot overwrite newer bash-only evidence', async () => {
  const h = await harness(regularTask());
  try {
    let check = h.state.checkTaskStatus(h.store.tasks[0]);
    h.probes[0].resolve(positivePane); await check;
    assert.equal(h.state.taskRuntimeStatus.value['priority-a'].state, 'running');
    check = h.state.checkTaskStatus(h.store.tasks[0]);
    assert.equal(h.state.taskRuntimeStatus.value['priority-a'].state, 'checking');
    const [timer] = h.hooks.timers.values();
    assert.equal(timer.delay, 15000); timer.callback(); await check;
    assert.equal(h.state.taskRuntimeStatus.value['priority-a'].state, 'unknown');
    assert.equal(h.hooks.timers.size, 0);
    const newer = h.state.checkTaskStatus(h.store.tasks[0]);
    h.probes[2].resolve(bashPane); await newer;
    h.probes[1].resolve(positivePane); await tick();
    assert.equal(h.state.taskRuntimeStatus.value['priority-a'].state, 'not_running');
  } finally { h.stop(); }
});

test('same logical BSON ID reload keeps evidence; target switch prunes old cache and fences concurrent newer probe', async () => {
  const h = await harness(regularTask());
  try {
    const old = h.state.checkTaskStatus(h.store.tasks[0]);
    h.store.replaceTasks([regularTask()]);
    h.probes[0].resolve(positivePane); await old;
    assert.equal(h.state.taskRuntimeStatus.value['priority-a'].state, 'running');
    const remote = h.state.checkTaskStatus(h.store.tasks[0]);
    h.store.setTaskSource('priority-a', false);
    assert.equal(h.state.taskRuntimeStatus.value['priority-a'], undefined);
    const local = h.state.checkTaskStatus(h.store.tasks[0]);
    h.probes[2].resolve(bashPane); await local;
    h.probes[1].resolve(positivePane); await remote;
    assert.equal(h.state.taskRuntimeStatus.value['priority-a'].state, 'not_running');
    assert.equal(h.state.taskRuntimeStatus.value['priority-a'].isRemote, false);
  } finally { h.stop(); }
});

test('regular probe completion after manager unmount cannot publish or mutate control/progress', async () => {
  const h = await harness(regularTask());
  const before = JSON.stringify(h.store.tasks);
  const check = h.state.checkTaskStatus(h.store.tasks[0]);
  h.stop();
  h.probes[0].resolve(positivePane); await check;
  assert.equal(h.state.taskRuntimeStatus.value['priority-a'].state, 'checking');
  assert.equal(h.hooks.timers.size, 0);
  assert.equal(JSON.stringify(h.store.tasks), before);
});


test('invalid/missing task IDs never launch a runtime probe or publish evidence', async () => {
  for (const id of [undefined, null, '', {}, { $oid: '' }]) {
    const h = await harness(regularTask({ _id: id }));
    try {
      await h.state.checkTaskStatus(h.store.tasks[0]);
      assert.equal(h.calls.length, 0);
      assert.deepEqual(h.state.taskRuntimeStatus.value, {});
    } finally { h.stop(); }
  }
});

test('profile changes fence in-flight native dispatch metadata and invalidate previous process cache', async () => {
  const h = await harness(regularTask({ auth_profile: 'user1' }));
  try {
    const pending = h.state.checkTaskStatus(h.store.tasks[0]);
    h.store.replaceTasks([regularTask({ auth_profile: 'user2' })]);
    assert.equal(h.state.taskRuntimeStatus.value['priority-a'], undefined);
    h.probes[0].resolve(positivePane); await pending;
    assert.equal(h.state.taskRuntimeStatus.value['priority-a'], undefined);
    const next = h.state.checkTaskStatus(h.store.tasks[0]);
    h.probes[1].resolve(positivePane); await next;
    assert.equal(h.state.taskRuntimeStatus.value['priority-a'].authProfile, 'user2');
    assert.equal(h.calls[1].args.taskId, 'id-a');
  } finally { h.stop(); }
});

const stageEvent = (call, extra = {}) => ({ operation: call.name === 'pause_task' ? 'stop' : 'runtime',
  taskId: call.args.taskId, name: call.args.taskName, isRemote: call.args.isRemote,
  requestId: call.args.requestId, stage: 'ssh', message: '正在通过原 SSH 入口连接执行主机', ...extra });

test('actual handler routes only fixed stages for the exact pending operation/request/task/host; stages are not proof', async () => {
  const h = await harness(regularTask());
  try {
    const saved = seedCounts(h.store);
    const check = h.state.checkTaskStatus(h.store.tasks[0]);
    const pausing = h.state.pauseTask(h.store.tasks[0]);
    const [runtimeCall, stopCall] = h.calls;
    assert.equal(typeof runtimeCall.args.requestId, 'string');
    assert.equal(typeof stopCall.args.requestId, 'string');
    assert.notEqual(runtimeCall.args.requestId, stopCall.args.requestId);
    for (const patch of [{ taskId: 'other' }, { name: 'other' }, { isRemote: false }, { isRemote: 'true' },
      { requestId: 'old-request' }, { operation: 'stop' }, { operation: 'start' }, { stage: 'not_a_fixed_stage' },
      { stage: 'control_write' }, { stage: 'ssh\n' }, { message: '' }, { message: 'line\nbreak' }]) {
      assert.equal(h.state.handleTaskOperationStage(stageEvent(runtimeCall, patch)), false);
      assert.equal(h.state.taskRuntimeStatus.value['priority-a'].stage, 'native_request');
    }
    assert.equal(h.state.handleTaskOperationStage(stageEvent(runtimeCall)), true);
    assert.equal(h.state.taskRuntimeStatus.value['priority-a'].stage, 'ssh');
    assert.equal(h.state.taskRuntimeStatus.value['priority-a'].state, 'checking');
    for (const patch of [{ taskId: 'other' }, { name: 'other' }, { isRemote: false },
      { requestId: runtimeCall.args.requestId }, { operation: 'runtime' }, { stage: 'dispatch_read' }]) {
      assert.equal(h.state.handleTaskOperationStage(stageEvent(stopCall, patch)), false);
    }
    assert.equal(h.state.handleTaskOperationStage(stageEvent(stopCall, { stage: 'completed', message: '伪造成功文字不是回执' })), true);
    assert.equal(stopStatus(h), 'stopping');
    assert.equal(h.store.tasks[0].status, 'running');
    assert.equal(h.store.taskProgressMap['priority-a'], saved);
    h.probes[0].reject('[ssh] Permission denied (publickey): exact runtime failure');
    await check;
    const observation = h.state.taskRuntimeStatus.value['priority-a'];
    assert.equal(observation.stage, 'ssh');
    assert.equal(observation.error, '[ssh] Permission denied (publickey): exact runtime failure');
    assert.equal(h.state.handleTaskOperationStage(stageEvent(runtimeCall)), false);
    h.command.reject('[control_write] 执行退出已核验，但 CAS matched_count=0；原任务目标变化，未写入 paused');
    await assert.rejects(pausing, /matched_count=0/);
    const operation = h.store.taskStopOperationMap['priority-a'];
    assert.equal(operation.stage, 'control_write');
    assert.match(operation.error, /执行退出已核验.*matched_count=0.*原任务目标变化/);
    assert.equal(operation.status, 'stop_unconfirmed');
    assert.equal(stopStatus(h), '停止执行失败');
    assert.equal(h.state.handleTaskOperationStage(stageEvent(stopCall)), false);
    assert.equal(h.store.tasks[0].status, 'running');
    assert.equal(h.store.taskProgressMap['priority-a'], saved);
  } finally { h.stop(); }
});

test('runtime timeout freezes the last arrived stage; late stage/result cannot revive it or a newer generation', async () => {
  const h = await harness(regularTask());
  try {
    const first = h.state.checkTaskStatus(h.store.tasks[0]);
    const call = h.calls[0];
    assert.equal(h.state.handleTaskOperationStage(stageEvent(call, { stage: 'tmux', message: '正在查找精确会话及全部窗口' })), true);
    [...h.hooks.timers.values()][0].callback();
    // The queued rejection hasn't reached catch yet: the timeout latch already closes events.
    assert.equal(h.state.handleTaskOperationStage(stageEvent(call)), false);
    await first;
    assert.equal(h.state.taskRuntimeStatus.value['priority-a'].stage, 'tmux');
    assert.match(h.state.taskRuntimeStatus.value['priority-a'].error, /15 秒/);
    const next = h.state.checkTaskStatus(h.store.tasks[0]);
    assert.notEqual(h.calls[1].args.requestId, call.args.requestId);
    assert.equal(h.state.handleTaskOperationStage(stageEvent(call)), false);
    h.probes[0].resolve(positivePane); await tick();
    assert.equal(h.state.taskRuntimeStatus.value['priority-a'].state, 'checking');
    h.probes[1].resolve(bashPane); await next;
    assert.equal(h.state.taskRuntimeStatus.value['priority-a'].state, 'not_running');
  } finally { h.stop(); }
});

test('stop timeout and away/back replacement fence late stages, proof and older stop generations', async () => {
  const h = await harness();
  try {
    const pausing = h.state.pauseTask(h.store.tasks[0]);
    const old = h.calls[0];
    h.state.handleTaskOperationStage(stageEvent(old, { stage: 'process_exit', message: '等待已标识消费者退出' }));
    [...h.hooks.timers.values()][0].callback();
    assert.equal(h.state.handleTaskOperationStage(stageEvent(old)), false);
    await assert.rejects(pausing, /120 秒/);
    assert.equal(h.store.taskStopOperationMap['priority-a'].stage, 'process_exit');
    assert.equal(stopStatus(h), '停止确认超时');
    h.store.setTaskSource('priority-a', false); h.store.setTaskSource('priority-a', true);
    const newer = h.store.beginTaskStop(task());
    assert.notEqual(newer.requestId, old.args.requestId);
    assert.equal(h.state.handleTaskOperationStage(stageEvent(old)), false);
    h.command.resolve(receipt()); await tick();
    assert.equal(h.store.tasks[0].status, 'running');
    assert.equal(h.store.taskStopOperationMap['priority-a'].requestId, newer.requestId);
  } finally { h.stop(); }
});

test('real manager listener registers before task loading and cleans up; disposed events cannot publish', async () => {
  const h = await harness(regularTask());
  const mounting = h.hooks.mount(); await tick();
  assert.ok(h.hooks.handlers.has('task-operation-stage'));
  assert.ok(h.lists.length === 1);
  const eventHandler = h.hooks.handlers.get('task-operation-stage');
  const check = h.state.checkTaskStatus(h.store.tasks[0]);
  eventHandler({ payload: stageEvent(h.calls.find(call => call.name === 'check_task_status')) });
  assert.equal(h.state.taskRuntimeStatus.value['priority-a'].stage, 'ssh');
  h.stop();
  assert.equal(h.hooks.cleaned.filter(name => name === 'task-operation-stage').length, 1);
  const before = JSON.stringify(h.state.taskRuntimeStatus.value);
  eventHandler({ payload: stageEvent(h.calls.find(call => call.name === 'check_task_status'), { stage: 'completed' }) });
  assert.equal(JSON.stringify(h.state.taskRuntimeStatus.value), before);
  h.lists[0].resolve([regularTask()]); h.probes[0].resolve(positivePane);
  await mounting; await check;
  assert.equal(h.hooks.timers.size, 0);
  assert.equal(h.hooks.handlers.size, 0);
});

test('a stage-listener promise resolving after unmount is disposed immediately, without registry/native work', async () => {
  const h = await harness(), registration = deferred();
  let cleanup = 0;
  h.hooks.listen = (name) => { assert.equal(name, 'task-operation-stage'); return registration.promise; };
  const mounting = h.hooks.mount(); h.stop();
  registration.resolve(() => cleanup++); await mounting;
  assert.equal(cleanup, 1);
  assert.equal(h.calls.length, 0);
});

test('the actual native operation-specific script stage whitelist reaches the real pending UI handlers', async () => {
  const native = await readFile(new URL('../src-tauri/src/commands.rs', import.meta.url), 'utf8');
  const section = native.match(/let allowed: &\[&str\] = match self.operation \{([\s\S]*?)_ => &\[\]/)?.[1];
  assert.ok(section, 'native script-stage contract must remain inspectable; do not silently replace it with local fixtures');
  for (const operation of ['runtime', 'stop']) {
    const ids = [...section.match(new RegExp(`"${operation}" => &\\[([\\s\\S]*?)\\]`))[1].matchAll(/"([a-z_]+)"/g)].map(match => match[1]);
    const { TASK_SCRIPT_STAGES } = await import('../src/utils/taskDiagnostics.js');
    assert.deepEqual([...TASK_SCRIPT_STAGES[operation]], ids);
    const h = await harness(regularTask());
    try {
      const pending = operation === 'runtime' ? h.state.checkTaskStatus(h.store.tasks[0]) : h.state.pauseTask(h.store.tasks[0]);
      const call = h.calls[0];
      for (const stage of ids) {
        const detail = `原生实际阶段 ${stage}：跨端合同检查`;
        assert.equal(h.state.handleTaskOperationStage(stageEvent(call, { stage, message: detail })), true, `${operation}:${stage}`);
        const diagnostic = operation === 'runtime' ? h.state.taskRuntimeStatus.value['priority-a'] : h.store.taskStopOperationMap['priority-a'];
        assert.equal(diagnostic.stage, stage); assert.equal(diagnostic.detail, detail);
      }
      if (operation === 'runtime') { h.probes[0].resolve(bashPane); await pending; }
      else { h.command.reject('[receipt] synthetic operation failure'); await assert.rejects(pending); }
    } finally { h.stop(); }
  }
});

test('control_write followed by client timeout/disposal records missing final result, not a definite CAS failure', async () => {
  for (const outcome of ['timeout', 'dispose', 'native_timeout']) {
    const h = await harness();
    try {
      const pending = h.state.pauseTask(h.store.tasks[0]);
      h.state.handleTaskOperationStage(stageEvent(h.calls[0], { stage: 'control_write', message: '执行退出已核验；开始 CAS 写入' }));
      if (outcome === 'timeout') {
        [...h.hooks.timers.values()][0].callback(); await assert.rejects(pending, /120 秒/);
      } else if (outcome === 'native_timeout') {
        h.command.reject('[control_write] 执行退出已核验，但控制记录回写超时；不要盲目重试'); await assert.rejects(pending, /回写超时/);
      } else { h.stop(); h.command.resolve(receipt()); await pending; }
      const op = h.store.taskStopOperationMap['priority-a'];
      assert.equal(op.stage, 'control_write'); assert.equal(op.status, 'stop_unconfirmed');
      assert.equal(op.failureSource, outcome === 'timeout' ? 'client_timeout' : outcome === 'dispose' ? 'client_disposed' : 'native');
      assert.equal(h.store.tasks[0].status, 'running');
      const { stopDiagnostic } = await import('../src/utils/taskDiagnostics.js');
      const detail = stopDiagnostic(h.store.tasks[0], op, stopStatus(h)).detail;
      assert.match(detail, /control_write 最终写入结果待核对/);
      assert.doesNotMatch(detail, /控制记录写入失败/);
      if (outcome === 'timeout') { h.command.resolve(receipt()); await tick(); assert.equal(h.store.tasks[0].status, 'running'); }
    } finally { h.stop(); }
  }
});

test('launch changes fence late native business errors through original request owner and real Pinia watcher', async () => {
  const h = await harness(regularTask({ launchAttemptId: 'old-launch' }));
  try {
    const pending = h.state.checkTaskStatus(h.store.tasks[0]);
    h.store.replaceTasks([regularTask({ launchAttemptId: 'new-launch' })]);
    assert.equal(h.state.taskRuntimeStatus.value['priority-a'], undefined);
    const now = Date.now();
    const old = { schemaVersion:1, taskId:'id-a', name:'priority-a', isRemote:true, authProfile:'user1',
      source:'priority_capability', consumerRunId:'0123456789abcdef0123456789abcdef', launchAttemptId:'old-launch', launchMatched:true,
      heartbeatAt:now, occurredAt:now-1000, errorClass:'simulation_recovery_phase_timeout', reason:'old launch failed',
      phase:'simulation_recovery_phase', timeoutSeconds:300, scope:'accepted_receipts', recoveryState:'waiting_original_recovery', nextRetryAt:null };
    h.probes[0].resolve(`${positivePane}\nQUANTNIGHT_BUSINESS_DIAGNOSTIC ${JSON.stringify(old)}`);
    await pending;
    assert.equal(h.state.taskRuntimeStatus.value['priority-a'], undefined);
    const next = h.state.checkTaskStatus(h.store.tasks[0]);
    h.probes[1].resolve(bashPane); await next;
    assert.equal(h.state.taskRuntimeStatus.value['priority-a'].launchAttemptId, 'new-launch');
    assert.equal(h.state.taskRuntimeStatus.value['priority-a'].businessDiagnostic, null);
    assert.equal(h.store.tasks[0].status, 'running');
    assert.ok(h.calls.every(call => call.name === 'check_task_status'));
  } finally { h.stop(); }
});


// Start regression fixtures now traverse open-modal and the same original
// native diagnostic at submit; valid pause evidence is explicit, not inferred.
const startableTask = (extra = {}) => task({ status: 'paused', stopConfirmedAt: proofAt,
  stopConfirmedTaskId: 'id-a', stopConfirmedRemote: true, ...extra });
const absentPane = "❌ Session 'priority-a' 不存在 (remote)";
async function openForStart(h) {
  await h.state.openStartModal(h.store.tasks[0]);
  assert.equal(h.state.showStartModal.value, true);
}

for (const [handler, native, taskType] of [
  ['handleStartTask', 'start_task', 'regular'],
  ['handleStartSuperTask', 'start_super_task', 'super'],
  ['handlePriorityStartTask', 'start_priority_task', 'priority'],
]) {
  test(`actual ${handler} binds captured ID/full config/target; failure is specific and never retries`, async () => {
    const h = await harness(startableTask({ taskType }));
    try {
      await openForStart(h);
      const calls = [], checks = [];
      h.hooks.invoke = async (name, args) => {
        if (name === 'check_task_status') { checks.push(args); return absentPane; }
        calls.push({ name, args });
        throw 'taskctl start: wrong taskId/host; stdout=receipt stderr=execution_context_unverified';
      };
      const config = { max_concurrent: 3, rounds: { enabled: true }, marker: "space ' quote" };
      await h.state[handler]({ taskName: 'priority-a', config, isRemote: true });
      assert.deepEqual(calls, [{ name: native, args: { taskName: 'priority-a', taskId: 'id-a',
        config: JSON.stringify(config), isRemote: true, expectedRemote: true } }]);
      assert.equal(checks.length, 0);
      assert.equal(h.store.tasks[0].status, 'paused');
      assert.equal(h.lists.length, 0);
      assert.match(h.hooks.errors[0][0], /wrong taskId\/host.*stdout=receipt stderr=execution_context_unverified/);
      h.store.replaceTasks([]);
      await h.state[handler]({ taskName: 'priority-a', config, isRemote: true });
      assert.equal(calls.length, 1, 'missing GUI identity must not invoke native');
      assert.match(h.hooks.errors[1][0], /任务身份缺失/);
    } finally { h.stop(); }
  });
}

test('all native GUI start entrypoints delegate once without a second launch or status writer', async () => {
  const rust = await readFile(new URL('../src-tauri/src/commands.rs', import.meta.url), 'utf8');
  for (const name of ['start_task', 'start_super_task', 'start_priority_task']) {
    const body = rust.slice(rust.indexOf(`pub async fn ${name}(`)).split('\n}')[0];
    assert.match(body, /task_id: String/);
    assert.equal((body.match(/start_via_core\(/g) || []).length, 1);
    assert.doesNotMatch(body, /update_one|find_one|run_bash_script|launchAttemptId/);
  }
  const body = rust.slice(rust.indexOf('async fn start_via_core('), rust.indexOf('pub async fn start_task('));
  assert.equal((body.match(/core.start\(/g) || []).length, 1);
  assert.doesNotMatch(body, /update_one|find_one|run_bash_script|launchAttemptId/);
  const request = rust.slice(rust.indexOf('fn start_request('), rust.indexOf('async fn start_via_core('));
  for (const arg of ['expected_task_id', 'notification.no_notify = true', 'request.remote = Some(remote)', 'request.config_json = config.map(str::to_owned)']) assert.ok(request.includes(arg));
  assert.match(rust, /use quantnight_task_core::/);
  assert.match(body, /operation_task_identity/);
});

for (const handler of ['handleStartTask', 'handleStartSuperTask', 'handlePriorityStartTask']) {
  test(`${handler} rejects checkbox host migration and duplicate in-flight launches without losing original stop evidence`, async () => {
    const taskType = handler === 'handleStartSuperTask' ? 'super' : handler === 'handleStartTask' ? 'regular' : 'priority';
    const h = await harness(startableTask({ taskType }));
    try {
      await openForStart(h);
      const calls = [], pending = deferred();
      h.hooks.invoke = (name, args) => {
        if (name === 'check_task_status') return Promise.resolve(absentPane);
        calls.push({ name, args }); return pending.promise;
      };
      const args = { taskName: 'priority-a', config: { full: { keep: true } }, isRemote: false };
      await h.state[handler](args);
      assert.equal(calls.length, 0);
      assert.match(h.hooks.errors[0][0], /不能切换实际执行端/);
      args.isRemote = true;
      const first = h.state[handler](args);
      await tick();
      await h.state[handler](args);
      assert.equal(calls.length, 1);
      args.config.full.keep = false;
      assert.equal(JSON.parse(calls[0].args.config).full.keep, true);
      assert.equal(h.store.tasks[0].isRemote, true);
      pending.reject(new Error('launch receipt unconfirmed; do not retry'));
      await first;
      assert.equal(calls.length, 1);
      assert.equal(h.store.tasks[0].status, 'paused');
      assert.equal(h.hooks.timers.size, 0);
    } finally { h.stop(); }
  });
}

test('sync sends exact ID once, never follows with a bare host update, and stale host is not optimistically committed', async () => {
  const h = await harness(task({ status: 'ready', isRemote: false }));
  try {
    const calls = [], pending = deferred();
    h.hooks.invoke = (name, args) => {
      calls.push({ name, args });
      if (name === 'sync_remote_task') return pending.promise;
      if (name === 'get_all_tasks') return Promise.resolve([task({ status: 'ready', isRemote: true })]);
      return Promise.resolve();
    };
    const first = h.state.syncRemoteTask('priority-a');
    await tick();
    await h.state.syncRemoteTask('priority-a');
    assert.deepEqual(calls, [{ name: 'sync_remote_task', args: { taskId: 'id-a', alphaMissionList: 'priority-a', expectedRemote: false } }]);
    assert.equal(h.store.tasks[0].isRemote, false);
    pending.resolve();
    await first;
    assert.equal(h.store.tasks[0].isRemote, true);
    assert.ok(!calls.some(call => call.name === 'update_remote_status'));
  } finally { h.stop(); }
});

test('host toggle refusal retains original host and Priority update binds original ID/host without marking running', async () => {
  const h = await harness(task({ status: 'paused' }));
  try {
    const calls = [];
    h.hooks.invoke = async (name, args) => { calls.push({ name, args }); throw new Error('shared core refused unsafe mutation'); };
    await h.state.updateIsRemote('id-a', false);
    assert.equal(h.store.tasks[0].isRemote, true);
    await h.state.handleUpdatePriorityTask({ taskName: 'priority-a', config: { rounds: { enabled: true } }, isRemote: true });
    assert.deepEqual(calls[1], { name: 'update_priority_task', args: { taskName: 'priority-a', taskId: 'id-a', config: '{"rounds":{"enabled":true}}', isRemote: true } });
    assert.equal(h.store.tasks[0].status, 'paused');
    await h.state.handleUpdatePriorityTask({ taskName: 'priority-a', config: {}, isRemote: false });
    assert.equal(calls.length, 2);
    assert.match(h.hooks.errors.at(-1)[0], /不能迁移执行端/);
  } finally { h.stop(); }
});

test('start forwards persisted creation destination separately from captured actual host, never grants eligibility in UI', async () => {
  const h = await harness(task({ taskType: 'priority', status: 'pending', isRemote: false, targetRemote: true }));
  try {
    const calls = [];
    h.hooks.invoke = async (name, args) => { calls.push({ name, args }); throw new Error('core decides pristine destination eligibility'); };
    await h.state.openStartModal(h.store.tasks[0]);
    await h.state.handlePriorityStartTask({ taskName: 'priority-a', config: { exact: [] }, isRemote: false });
    assert.deepEqual(calls, [{ name: 'start_priority_task', args: { taskName: 'priority-a', taskId: 'id-a',
      config: '{"exact":[]}', isRemote: true, expectedRemote: false } }]);
    assert.equal(h.store.tasks[0].isRemote, false);
    assert.equal(h.store.tasks[0].status, 'pending');
    assert.equal(h.hooks.timers.size, 0);
    assert.match(h.hooks.errors[0][0], /core decides pristine destination eligibility/);
  } finally { h.stop(); }
});

test('delete captures original host/ID at confirmation, rejects duplicate actions and never removes on core refusal', async () => {
  const h = await harness(task({ status: 'paused', isRemote: true, stopConfirmedAt: 1000, stopConfirmedTaskId: 'id-a', stopConfirmedRemote: true }));
  try {
    const pending = deferred(), calls = [];
    h.hooks.invoke = (name, args) => { calls.push({ name, args }); return pending.promise; };
    h.state.deleteTask({ $oid: 'id-a' });
    assert.match(h.hooks.dialog.content, /保留候选回执和正式结果/);
    h.store.replaceTasks([task({ status: 'paused', isRemote: false })]);
    const first = h.hooks.dialog.onPositiveClick();
    assert.equal(h.hooks.dialogClosed, true, "close before the native receipt settles");
    assert.equal(h.store.tasks.length, 1, "do not remove before confirmation");
    assert.equal(h.hooks.messages.length, 1);
    assert.equal(h.hooks.messages[0].destroyed, false);
    await h.hooks.dialog.onPositiveClick();
    assert.equal(h.hooks.messages.length, 1, "duplicate click must not create another operation");
    assert.deepEqual(calls, [{ name: 'delete_task', args: { id: 'id-a', expectedRemote: true } }]);
    pending.reject(new Error('original execution host changed before locked cleanup'));
    await first;
    assert.equal(h.store.tasks.length, 1);
    assert.equal(h.store.tasks[0].isRemote, false);
    assert.match(h.hooks.errors[0][0], /original execution host changed/);
    assert.equal(h.hooks.messages[0].destroyed, true);
    assert.equal(calls.length, 1);
  } finally { h.stop(); }
});


test('archive confirmation closes immediately while progress remains until the actual success receipt', async () => {
  const h = await harness(task({ status: 'paused', stopConfirmedAt: 1000, stopConfirmedTaskId: 'id-a', stopConfirmedRemote: true }));
  try {
    const pending = deferred();
    const calls = [];
    h.hooks.invoke = (name, args) => {
      calls.push({ name, args });
      if (name === 'delete_task') return pending.promise;
      if (name === 'get_all_tasks') return Promise.resolve([]);
      if (name === 'refresh_task_progress') return Promise.resolve();
      assert.fail(`unexpected command: ${name}`);
    };
    h.state.deleteTask({ $oid: 'id-a' });
    const operation = h.hooks.dialog.onPositiveClick();
    assert.equal(h.hooks.dialogClosed, true);
    assert.equal(h.store.tasks.length, 1);
    assert.equal(h.hooks.messages[0].options.duration, 0);
    assert.equal(h.hooks.messages[0].destroyed, false);
    assert.match(h.hooks.messages[0].text, /核验停止状态.*归档/);
    pending.resolve();
    await operation;
    assert.equal(calls.filter(call => call.name === 'delete_task').length, 1);
    assert.equal(h.store.tasks.length, 0);
    assert.equal(h.hooks.messages[0].destroyed, true);
    assert.equal(h.hooks.errors.length, 0);
  } finally { h.stop(); }
});


test('one confirmation pauses running, unknown launch and missing-proof tasks before archive', async () => {
  for (const status of ['running', 'launch_unknown', 'paused']) {
    const h = await harness(task({ status }));
    try {
      const paused = deferred(), archived = deferred(), calls = [];
      h.hooks.invoke = (name, args) => {
        calls.push({ name, args });
        if (name === 'pause_task') return paused.promise;
        if (name === 'delete_task') return archived.promise;
        if (name === 'get_all_tasks') return Promise.resolve([]);
        if (name === 'refresh_task_progress') return Promise.resolve();
        assert.fail(`unexpected command: ${name}`);
      };
      h.state.deleteTask({ $oid: 'id-a' });
      assert.equal(calls.length, 0, 'opening confirmation must not mutate');
      assert.match(h.hooks.dialog.content, /先暂停.*归档/);
      const operation = h.hooks.dialog.onPositiveClick();
      assert.equal(h.hooks.dialogClosed, true);
      assert.deepEqual(calls.map(c => c.name), ['pause_task']);
      await h.hooks.dialog.onPositiveClick();
      assert.equal(calls.length, 1, 'duplicate confirmation must not start another pause');
      paused.resolve(receipt()); await tick(); await tick();
      assert.deepEqual(calls.map(c => c.name), ['pause_task', 'delete_task']);
      assert.deepEqual(calls[1].args, { id: 'id-a', expectedRemote: true });
      assert.equal(h.store.tasks.length, 1);
      archived.resolve({ unresolvedItemCount: 10 });
      await operation;
      assert.equal(h.store.tasks.length, 0);
      assert.equal(h.hooks.errors.length, 0);
      assert.match(h.hooks.notices[0].text, /已归档.*10条/);
    } finally { h.stop(); }
  }
});

test('failed or invalid stopping proof aborts the single archive workflow', async () => {
  for (const outcome of ['failure', 'invalid-proof', 'generation']) {
    const h = await harness(task({ status: outcome === 'generation' ? 'generation_unknown' : 'running' }));
    try {
      const calls = [];
      h.hooks.invoke = async (name) => {
        calls.push(name); assert.equal(name, 'pause_task');
        if (outcome === 'invalid-proof') return receipt({ taskId: 'another-id' });
        throw new Error(outcome === 'generation' ? 'candidate generation is not the simulation consumer; reconcile it before using pause' : 'fixture stop failed');
      };
      h.state.deleteTask({ $oid: 'id-a' });
      await h.hooks.dialog.onPositiveClick();
      assert.deepEqual(calls, ['pause_task']);
      assert.equal(h.store.tasks.length, 1);
      assert.equal(h.hooks.messages[0].destroyed, true);
      assert.equal(h.hooks.errors.length, 1);
      if (outcome === 'generation') assert.match(h.hooks.errors[0][0], /生成.*不能代替/);
    } finally { h.stop(); }
  }
});

test('archive joins an already pending original pause without a second confirmation or pause', async () => {
  const h = await harness(task());
  try {
    const paused = deferred(), calls = [];
    let deleted = false;
    h.hooks.invoke = (name, args) => {
      calls.push({ name, args });
      if (name === 'pause_task') return paused.promise;
      if (name === 'delete_task') { deleted = true; return Promise.resolve({ unresolvedItemCount: 0 }); }
      if (name === 'get_all_tasks') return Promise.resolve(deleted ? [] : [task({ status:'paused', stopConfirmedAt:proofAt, stopConfirmedTaskId:'id-a', stopConfirmedRemote:true })]);
      if (name === 'refresh_task_progress') return Promise.resolve();
      assert.fail(`unexpected command: ${name}`);
    };
    const stopping = h.state.pauseTask(h.store.tasks[0]);
    h.state.deleteTask({ $oid: 'id-a' });
    const archiving = h.hooks.dialog.onPositiveClick();
    assert.equal(calls.filter(c => c.name === 'pause_task').length, 1);
    assert.equal(calls.some(c => c.name === 'delete_task'), false);
    paused.resolve(receipt());
    await Promise.all([stopping, archiving]);
    assert.equal(calls.filter(c => c.name === 'pause_task').length, 1);
    assert.equal(calls.filter(c => c.name === 'delete_task').length, 1);
    assert.equal(h.store.tasks.length, 0);
  } finally { h.stop(); }
});

test('leaving the view during automatic pause does not start a later archive', async () => {
  const h = await harness(task());
  const paused = deferred(), calls = [];
  h.hooks.invoke = (name) => { calls.push(name); assert.equal(name, 'pause_task'); return paused.promise; };
  h.state.deleteTask({ $oid: 'id-a' });
  const operation = h.hooks.dialog.onPositiveClick();
  h.stop(); paused.resolve(receipt());
  await operation;
  assert.deepEqual(calls, ['pause_task']);
  assert.equal(h.store.tasks.length, 1);
  assert.equal(h.hooks.messages[0].destroyed, true);
});

test('stopped task with unresolved receipt summary archives once and reports preservation instead of failure', async () => {
  const h = await harness(task({ status: 'paused', stopConfirmedAt: 1000, stopConfirmedTaskId: 'id-a', stopConfirmedRemote: true }));
  try {
    const calls = [];
    h.hooks.invoke = async (name, args) => {
      calls.push({ name, args });
      if (name === 'delete_task') return { unresolvedItemCount: 10, receiptAssessmentUnknownCount: 0, itemsPreserved: true, platformRequestsCancelled: false, capacityReleased: false };
      if (name === 'get_all_tasks') return [];
      if (name === 'refresh_task_progress') return;
      assert.fail(`unexpected native action: ${name}`);
    };
    h.state.deleteTask({ $oid: 'id-a' });
    assert.match(h.hooks.dialog.content, /未决 simulation 回执也会保留/);
    await h.hooks.dialog.onPositiveClick();
    assert.equal(calls.filter(c => c.name === 'delete_task').length, 1);
    assert.equal(calls.some(c => c.name === 'pause_task' || c.name === 'start_task'), false);
    assert.equal(h.store.tasks.length, 0);
    assert.equal(h.hooks.errors.length, 0);
    assert.match(h.hooks.notices[0].text, /已归档.*10条.*回执已保留/);
  } finally { h.stop(); }
});

test('backend remains authoritative when stop state changed after opening confirmation', async () => {
  const h = await harness(task({ status: 'paused', stopConfirmedAt: 1000, stopConfirmedTaskId: 'id-a', stopConfirmedRemote: true }));
  try {
    h.hooks.invoke = async () => { throw '共享 TaskControl delete 未确认：' + JSON.stringify({ command: 'delete', ok: false, error: 'current valid stop confirmation required before delete', taskConfig: { ignoredLargeBody: 'x'.repeat(2000) } }); };
    h.state.deleteTask({ $oid: 'id-a' });
    await h.hooks.dialog.onPositiveClick();
    assert.equal(h.store.tasks.length, 1);
    assert.match(h.hooks.errors[0][0], /停止状态仍未确认.*归档未执行/);
    assert.doesNotMatch(h.hooks.errors[0][0], /ignoredLargeBody/);
  } finally { h.stop(); }
});

for (const taskType of ['regular', 'super']) {
  const handler = taskType === 'super' ? 'handleStartSuperTask' : 'handleStartTask';




  test(`${taskType}: normal action reaches core once without PID preflight; callback cannot reuse consumed modal`, async () => {
    const h = await harness(regularTask({ taskType, status: 'ready' }));
    try {
      const calls = [], launch = deferred();
      h.hooks.invoke = (name, args) => {
        calls.push({ name, args });
        if (name === 'check_task_status') return Promise.resolve(absentPane);
        if (name.startsWith('start_')) return launch.promise;
        if (name === 'get_all_tasks') return Promise.resolve([regularTask({ taskType, status: 'running', launchAttemptId: 'new-launch' })]);
        if (name === 'refresh_task_progress') return Promise.resolve();
        assert.fail(name);
      };
      await h.state.openStartModal(h.store.tasks[0]);
      assert.equal(h.state.showStartModal.value, true);
      const args = { taskName: 'priority-a', config: { max_concurrent: 3 }, isRemote: true };
      const first = h.state[handler](args); await tick();
      await h.state[handler](args);
      assert.equal(calls.filter(call => call.name.startsWith('start_')).length, 1);
      launch.resolve(); await first; await tick();
      assert.equal(h.state.startModalTarget.value, null);
      assert.equal(h.state.showStartModal.value, false);
      assert.equal(calls.filter(call => call.name === 'check_task_status').length, 0);
      assert.equal(h.hooks.notices.filter(notice => notice.type === 'success').length, 1);
      await h.state[handler](args);
      assert.equal(calls.filter(call => call.name.startsWith('start_')).length, 1);
    } finally { h.stop(); }
  });


}







// v2 follow-up: exercise the actual setup -> invoke boundary, not button text.
for (const taskType of ['regular', 'super']) for (const actual of [false, true]) {
  test(`followup v2 ${taskType} actual=${actual}: stale targetRemote never overrides actual execution host`, async () => {
    const initial = regularTask({ taskType, status: 'ready', isRemote: actual, targetRemote: !actual });
    const h = await harness(initial);
    try {
      const calls = [];
      h.hooks.invoke = async (name, args) => {
        calls.push({ name, args });
        if (name === 'check_task_status') return `❌ Session 'priority-a' 不存在 (${actual ? 'remote' : 'local'})`;
        if (name.startsWith('start_')) return;
        if (name === 'get_all_tasks') return [{ ...initial, status: 'running', launchAttemptId: 'new' }];
        if (name === 'refresh_task_progress') return;
        assert.fail(name);
      };
      await h.state.openStartModal(h.store.tasks[0]);
      assert.equal(h.state.showStartModal.value, true);
      await h.state[taskType === 'super' ? 'handleStartSuperTask' : 'handleStartTask']({ taskName: initial.name, config: { max_concurrent: 3 }, isRemote: actual });
      await tick();
      const starts = calls.filter(call => call.name.startsWith('start_'));
      assert.equal(starts.length, 1);
      assert.equal(starts[0].args.isRemote, actual, 'stale creation intent is not actual host');
      assert.equal(starts[0].args.expectedRemote, actual);
      assert.deepEqual(calls.filter(call => call.name === 'check_task_status'), []);
    } finally { h.stop(); }
  });
}

test('followup v2 Priority initial remote target preserves actual-local fence without unrelated local runtime preflight', async () => {
  for (const runtime of [null, 'unknown']) {
    const initial = task({ status: 'pending', isRemote: false, targetRemote: true, launchAttemptId: null });
    const h = await harness(initial);
    try {
      if (runtime) h.state.taskRuntimeStatus.value[initial.name] = {
        ...h.state.runtimeTarget(initial), state: runtime, observedAt: Date.now(), diagnostic: 'actual-local observation, not target-remote authority',
      };
      const calls = [];
      h.hooks.invoke = async (name, args) => {
        calls.push({ name, args });
        if (name === 'start_priority_task') return;
        if (name === 'get_all_tasks') return [{ ...initial, status: 'running', isRemote: true, launchAttemptId: 'first-remote-launch' }];
        if (name === 'refresh_task_progress') return;
        assert.fail(`unexpected actual-local preflight: ${name}`);
      };
      await h.state.openStartModal(h.store.tasks[0]);
      assert.equal(h.state.showStartModal.value, true, `local ${runtime} must not decide pristine remote destination`);
      await h.state.handlePriorityStartTask({ taskName: initial.name, config: { rounds: { enabled: true } }, isRemote: false });
      await tick();
      const starts = calls.filter(call => call.name === 'start_priority_task');
      assert.equal(starts.length, 1);
      assert.equal(starts[0].args.isRemote, true);
      assert.equal(starts[0].args.expectedRemote, false);
      assert.equal(calls.filter(call => call.name === 'check_task_status').length, 0);
    } finally { h.stop(); }
  }
});

test('v6 legacy own-start result supplies no join proof: no conflicting pause and no later retry', async () => {
  const initial = regularTask({ status: 'ready', isRemote: true });
  const h = await harness(initial);
  const launch = deferred();
  try {
    const calls = [];
    h.hooks.invoke = (name, args) => {
      calls.push({ name, args });
      if (name === 'check_task_status') return Promise.resolve(absentPane);
      if (name === 'start_task') return launch.promise;
      if (name === 'pause_task') return Promise.resolve(receipt()); // bad v1 route must fail assertions, not hang
      if (name === 'get_all_tasks') return Promise.resolve([{ ...initial, status: 'running', launchAttemptId: 'claimed-launch' }]);
      if (name === 'refresh_task_progress') return Promise.resolve();
      assert.fail(name);
    };
    await h.state.openStartModal(h.store.tasks[0]);
    const starting = h.state.handleStartTask({ taskName: initial.name, config: {}, isRemote: true });
    await tick();
    assert.equal(calls.filter(call => call.name === 'start_task').length, 1);
    const pausing = h.state.pauseTask(h.store.tasks[0]);
    launch.resolve(); await starting; await tick();
    assert.equal(await pausing, false);
    assert.equal(calls.filter(call => call.name === 'pause_task').length, 0);
    assert.equal(h.store.taskStopOperationMap[initial.name], undefined);
    assert.ok(h.hooks.notices.some(notice => notice.type === 'warning' && /未发送.*暂停|暂停.*未发送/.test(notice.text)));
    assert.equal(h.hooks.notices.filter(notice => notice.type === 'success').length, 1, 'only actual start may succeed');
    await tick();
    assert.equal(calls.filter(call => call.name === 'pause_task').length, 0, 'no hidden retry after start ends');
  } finally { launch.resolve(); h.stop(); }
});

test('followup v2 pending public pause blocks start/sync/host mutations but one-confirm archive joins that same pause', async () => {
  const initial = regularTask();
  const h = await harness(initial);
  const paused = deferred();
  try {
    const calls = []; let archived = false;
    h.hooks.invoke = (name, args) => {
      calls.push({ name, args });
      if (name === 'pause_task') return paused.promise;
      if (name === 'delete_task') { archived = true; return Promise.resolve({ unresolvedItemCount: 2 }); }
      if (name === 'get_all_tasks') return Promise.resolve(archived ? [] : [{ ...initial, status: 'paused', stopConfirmedAt: proofAt, stopConfirmedTaskId: 'id-a', stopConfirmedRemote: true }]);
      if (name === 'refresh_task_progress' || name === 'sync_remote_task' || name === 'update_remote_status') return Promise.resolve();
      assert.fail(name);
    };
    const stopping = h.state.pauseTask(h.store.tasks[0]);
    await h.state.openStartModal(h.store.tasks[0]);
    await h.state.handleStartTask({ taskName: initial.name, config: {}, isRemote: true });
    await h.state.syncRemoteTask(initial.name);
    await h.state.updateIsRemote(initial._id, false);
    const conflicts = calls.filter(call => ['sync_remote_task', 'update_remote_status', 'start_task'].includes(call.name));
    h.state.deleteTask(initial._id);
    const archiving = h.hooks.dialog.onPositiveClick();
    assert.equal(h.hooks.dialogClosed, true);
    assert.equal(calls.filter(call => call.name === 'pause_task').length, 1);
    paused.resolve(receipt()); await Promise.all([stopping, archiving]);
    assert.deepEqual(conflicts, []);
    assert.equal(calls.filter(call => call.name === 'delete_task').length, 1);
    assert.equal(h.store.tasks.length, 0);
    assert.equal(h.hooks.notices.filter(notice => notice.type === 'success').length, 0, 'only archive warning about retained receipts, no fake sync/source success');
  } finally { paused.resolve(receipt()); h.stop(); }
});

for (const taskType of ['regular', 'super']) test(`followup v2 ${taskType} legal start -> pause -> start -> one-confirm pause/archive preserves original lifecycle`, async () => {
  const initial = regularTask({ taskType, status: 'ready', targetRemote: false, isRemote: true });
  const h = await harness(initial);
  try {
    let control = { ...initial }; let live = false; let launches = 0; let archived = false;
    const calls = [];
    h.hooks.invoke = async (name, args) => {
      calls.push({ name, args });
      if (name === 'check_task_status') return live ? positivePane : absentPane;
      if (name === (taskType === 'super' ? 'start_super_task' : 'start_task')) {
        assert.equal(args.isRemote, true); assert.equal(args.expectedRemote, true);
        live = true; control = { ...control, status: 'running', launchAttemptId: `launch-${++launches}`,
          stopConfirmedAt: null, stopConfirmedTaskId: null, stopConfirmedRemote: null }; return;
      }
      if (name === 'pause_task') {
        assert.equal(args.isRemote, true); live = false;
        control = { ...control, status: 'paused', stopConfirmedAt: proofAt, stopConfirmedTaskId: 'id-a', stopConfirmedRemote: true };
        return receipt();
      }
      if (name === 'delete_task') { assert.equal(live, false); archived = true; return { unresolvedItemCount: 0 }; }
      if (name === 'get_all_tasks') return archived ? [] : [{ ...control }];
      if (name === 'refresh_task_progress') return;
      assert.fail(name);
    };
    const start = async () => {
      await h.state.openStartModal(h.store.tasks[0]);
      assert.equal(h.state.showStartModal.value, true);
      await h.state[taskType === 'super' ? 'handleStartSuperTask' : 'handleStartTask']({ taskName: initial.name, config: { max_concurrent: 3 }, isRemote: true });
      await tick(); assert.equal(h.store.tasks[0].status, 'running');
    };
    await start();
    assert.equal(await h.state.pauseTask(h.store.tasks[0]), true);
    assert.equal(h.store.tasks[0].status, 'paused');
    assert.equal(stopStatus(h), 'paused');
    await start();
    assert.equal(h.store.taskStopOperationMap[initial.name], undefined);
    h.state.deleteTask(initial._id);
    await h.hooks.dialog.onPositiveClick();
    assert.equal(h.hooks.dialogClosed, true);
    assert.equal(h.store.tasks.length, 0);
    assert.equal(h.hooks.errors.length, 0);
    assert.deepEqual(calls.filter(call => /^(start_task|start_super_task|pause_task|delete_task)$/.test(call.name)).map(call => call.name),
      [taskType === 'super' ? 'start_super_task' : 'start_task', 'pause_task', taskType === 'super' ? 'start_super_task' : 'start_task', 'pause_task', 'delete_task']);
  } finally { h.stop(); }
});

test('followup v2 native mutator lock rejection is not pause success, not archived and not automatically retried', async () => {
  const h = await harness(regularTask());
  try {
    const calls = [];
    h.hooks.invoke = async (name, args) => { calls.push({ name, args }); throw new Error('task mutator lock busy; launch claimed by another client'); };
    await assert.rejects(h.state.pauseTask(h.store.tasks[0]), /mutator lock busy/);
    assert.deepEqual(calls.map(call => call.name), ['pause_task']);
    assert.equal(h.store.tasks[0].status, 'running');
    assert.equal(h.store.taskStopOperationMap['priority-a'].status, 'stop_unconfirmed');
    assert.equal(h.hooks.notices.filter(notice => notice.type === 'success').length, 0);
    assert.match(h.hooks.errors[0][0], /mutator lock busy/);
    await tick(); assert.equal(calls.length, 1);
  } finally { h.stop(); }
});

test('followup v2 Priority modal fences replacement identity and changed saved destination including away-and-back', async () => {
  for (const patch of [{ _id: 'new-id' }, { isRemote: true }, { auth_profile: 'other' }, { targetRemote: false }]) {
    const initial = task({ status: 'pending', isRemote: false, targetRemote: true, launchAttemptId: null });
    const h = await harness(initial);
    try {
      const calls = [];
      h.hooks.invoke = async (name, args) => { calls.push({ name, args }); throw new Error('no native action should be sent for stale modal'); };
      await h.state.openStartModal(h.store.tasks[0]);
      assert.equal(h.state.showStartModal.value, true);
      h.store.replaceTasks([{ ...initial, ...patch }]);
      h.store.replaceTasks([initial]);
      await h.state.handlePriorityStartTask({ taskName: initial.name, config: {}, isRemote: false });
      assert.deepEqual(calls, [], 'stale Priority modal is not authority for a replacement task/destination');
      assert.equal(h.state.showStartModal.value, false);
    } finally { h.stop(); }
  }
});











// Restored original UX: display saved JSON and always submit the editor value.
// Native None selector tests remain CLI/core contracts, not a hidden GUI editor policy.
const parityTable = JSON.parse(await readFile(new URL('./fixtures/startConfigParity.json', import.meta.url), 'utf8'));
for (const row of parityTable.rows) test(`visible JSON GUI adapter: actual StartTaskModal -> TaskManager -> invoke ${row.id}`, async () => {
  // Current Task Option<JsonValue> wire represents both absent and null as null.
  const wire = Object.fromEntries(['mission_config', 'startConfig', 'config'].map(key => [key, row.rawTask[key] ?? null]));
  const initial = task({ ...wire, taskType: row.kind, templatefile: 'pipeline', status: 'ready' });
  const h = await harness(initial);
  try {
    const calls = [];
    h.hooks.invoke = async (name, args) => {
      calls.push({ name, args });
      if (name === 'check_task_status') return absentPane;
      if (name.startsWith('start_')) return;
      if (name === 'get_all_tasks') return [{ ...initial, status: 'running' }];
      if (name === 'refresh_task_progress') return;
      assert.fail(name);
    };
    await h.state.openStartModal(h.store.tasks[0]);
    const originalModal = await readFile(new URL('../src/components/modals/StartTaskModal.vue', import.meta.url), 'utf8');
    const compiled = compileScript(parse(originalModal).descriptor, { id: `parity-${row.id}` }).content
      .replace(/import \{ useMessage \} from "naive-ui";/, 'const useMessage = () => ({ error: text => { throw new Error(text); } });')
      .replaceAll('"@/utils/taskConfig"', JSON.stringify(new URL('../src/utils/taskConfig.js', import.meta.url).href))
      .replaceAll('from "vue"', `from ${JSON.stringify(vueModule)}`);
    const modal = (await import(`data:text/javascript;base64,${Buffer.from(compiled).toString('base64')}`)).default;
    const emitted = [], pending = [];
    const handlers = { 'start-task': h.state.handleStartTask, 'start-super-task': h.state.handleStartSuperTask, 'start-priority-task': h.state.handlePriorityStartTask };
    const form = modal.setup({ show: true, task: h.state.selectedTask.value, isSuper: row.kind === 'super', isPriority: row.kind === 'priority' }, {
      expose() {}, emit(name, args) { emitted.push({ name, args }); if (handlers[name]) pending.push(handlers[name](args)); },
    });
    const displayed = JSON.parse(form.form.value.config);
    if (!row.edit) assert.deepEqual(displayed, row.observedGui, 'original saved JSON/default template is visible');
    if (row.edit) form.form.value.config = JSON.stringify(row.edit);
    form.onConfirm(); await Promise.all(pending); await tick();
    const native = calls.find(call => call.name.startsWith('start_'));
    assert.ok(native, 'actual emitted modal config must reach native boundary');
    assert.equal(emitted.filter(event => event.name.startsWith('start-')).length, 1);
    assert.deepEqual(JSON.parse(native.args.config), row.edit ?? displayed,
      'edited or untouched editor JSON reaches native unchanged');
    assert.deepEqual(emitted.find(event => event.name.startsWith('start-')).args.config, row.edit ?? displayed);
  } finally { h.stop(); }
});

test('v4 R3 confirmed remote-only blocks actual handlers, retains exact-ID details, while old/local remote tasks still start', async () => {
  for (const taskType of ['regular', 'super', 'priority']) {
    const original = task({ taskType, status: 'ready', isRemote: true, controlSource: 'remote_only' });
    const h = await harness(original);
    try {
      const pushed = []; h.hooks.router.push = target => pushed.push(target);
      await h.state.generateList(original._id);
      await h.state.syncRemoteTask(original.name);
      await h.state.updateIsRemote(original._id, false);
      await h.state.openStartModal(original);
      await h.state.handleStartTask({ taskName: original.name, config: {}, isRemote: true });
      await h.state.handleStartSuperTask({ taskName: original.name, config: {}, isRemote: true });
      await h.state.handlePriorityStartTask({ taskName: original.name, config: {}, isRemote: true });
      assert.equal(await h.state.pauseTask(original), false);
      h.state.deleteTask(original._id); h.state.openUpdateModal(original);
      await h.state.handleUpdateTask({ task: original, config: {}, isRemote: true });
      await h.state.handleUpdatePriorityTask({ taskName: original.name, config: {}, isRemote: true });
      assert.deepEqual(h.calls, [], 'no unsupported native mutator/probe calls for known remote-only');
      assert.equal(h.hooks.dialog, undefined);
      assert.equal(h.hooks.notices.filter(n => n.type === 'success').length, 0);
      h.state.viewTask(original._id);
      assert.deepEqual(pushed, [{ name: 'TaskManager', query: { taskId: 'id-a' } }]);
    } finally { h.stop(); }
  }
  for (const controlSource of [undefined, null, 'local', 'unknown', true]) {
    const initial = regularTask({ status: 'ready', isRemote: true, controlSource });
    const h = await harness(initial);
    try {
      const calls = [];
      h.hooks.invoke = async (name, args) => {
        calls.push({ name, args });
        if (name === 'check_task_status') return absentPane;
        if (name === 'start_task') return;
        if (name === 'get_all_tasks') return [{ ...initial, status: 'running' }];
        if (name === 'refresh_task_progress') return;
        assert.fail(name);
      };
      await h.state.openStartModal(h.store.tasks[0]);
      await h.state.handleStartTask({ taskName: initial.name, isRemote: true }); await tick();
      assert.equal(calls.filter(c => c.name === 'start_task').length, 1, 'legacy/unknown source is not a global denial');
      assert.equal(calls.find(c => c.name === 'start_task').args.expectedRemote, true);
    } finally { h.stop(); }
  }
});

test('v4 R2 actual router.resolve + original view handler renders exact-ID task details, not a fake results route', async () => {
  const routerModule = import.meta.resolve('vue-router');
  const routerSource = (await readFile(new URL('../src/router/index.js', import.meta.url), 'utf8'))
    .replaceAll('createWebHistory', 'createMemoryHistory')
    .replace("from 'vue-router'", `from ${JSON.stringify(routerModule)}`)
    .replace(/import (\w+) from '[^'\n]+\.vue';/g, 'const $1 = { render() { return null; } };');
  const router = (await import(`data:text/javascript;base64,${Buffer.from(routerSource).toString('base64')}`)).default;
  await router.push('/task'); await router.isReady();
  const route = new Proxy({}, { get: (_, key) => router.currentRoute.value[key] });
  const original = regularTask({ status: 'completed_with_errors', controlSource: 'remote_only' });
  const h = await harness(original, { router, route });
  try {
    h.hooks.invoke = async (name) => {
      if (name === 'get_all_tasks') return h.store.tasks;
      if (name === 'refresh_task_progress') return;
      assert.fail(`view must not invent a task-results backend: ${name}`);
    };
    await h.state.viewTask(original._id); await tick();
    const resolved = router.resolve(router.currentRoute.value.fullPath);
    assert.equal(resolved.name, 'TaskManager'); assert.equal(resolved.matched.length, 1);
    assert.equal(resolved.path, '/task'); assert.equal(resolved.query.taskId, 'id-a');
    assert.equal(h.state.detailTask.value.name, original.name);
    const detailSource = await readFile(new URL('../src/components/TaskCards/TaskDetailsPanel.vue', import.meta.url), 'utf8');
    const compiled = compileScript(parse(detailSource).descriptor, { id: 'real-details', inlineTemplate: true }).content
      .replaceAll("'../../utils/taskDiagnostics.js'", JSON.stringify(new URL('../src/utils/taskDiagnostics.js', import.meta.url).href))
      .replaceAll("'../../stores/taskStore.js'", JSON.stringify(new URL('../src/stores/taskStore.js', import.meta.url).href))
      .replaceAll('from "vue"', `from ${JSON.stringify(vueModule)}`).replaceAll("from 'vue'", `from ${JSON.stringify(vueModule)}`);
    const panel = (await import(`data:text/javascript;base64,${Buffer.from(compiled).toString('base64')}`)).default;
    const { createSSRApp, h: renderH } = await import('vue');
    const { renderToString } = await import('@vue/server-renderer');
    const renderDetails = () => renderToString(createSSRApp({ render: () => renderH(panel, {
      taskId: h.state.detailTaskId.value, task: h.state.detailTask.value,
      progress: h.state.detailTask.value && h.store.taskProgressMap[h.state.detailTask.value.name],
    }) }));
    let html = await renderDetails();
    assert.match(html, /任务详情/); assert.match(html, /仅远端登记/);
    assert.match(html, /未决.*已接受回执明细/); assert.match(html, /不是本任务完整结果/);
    assert.match(html, /taskctl results/);
    h.store.replaceTasks([{ ...original, _id: 'new-id' }]);
    assert.equal(h.state.detailTask.value, null);
    html = await renderDetails(); assert.match(html, /未找到当前活动记录/); assert.match(html, /不.*按同名任务认领/);
    h.store.replaceTasks([{ ...original, status: 'deactive' }]);
    html = await renderDetails(); assert.match(html, /该记录已归档/);
    await h.state.closeTaskDetails(); await tick(); assert.equal(router.currentRoute.value.query.taskId, undefined);
    assert.equal(h.state.detailTaskId.value, '');
    // Real handler is wired from BOTH cards and detail panel in actual SFC.
    assert.equal((source.match(/@view="viewTask\(task\._id\)"/g) || []).length, 2);
    assert.match(source, /<TaskDetailsPanel :task-id="detailTaskId" :task="detailTask"/);
  } finally { h.stop(); }
});

const typedStart = (extra = {}) => ({ schemaVersion: 1, command: 'start', task: 'priority-a', ok: true,
  taskId: 'id-a', remote: true, launchAttemptId: 'original-launch', outcome: 'already_running',
  startedNew: false, consumerRunning: true, configApplied: false, configComparison: 'not_requested', requestSatisfied: true, ...extra });

test('v5 typed start: actual handler distinguishes started and existing consumer, does not clear existing stop evidence as new launch', async () => {
  for (const existing of [true, false]) {
    const h = await harness(startableTask({ taskType: 'super' }));
    try {
      const snapshot = { ...h.store.tasks[0] };
      const calls = [];
      h.hooks.invoke = async (name, args) => {
        calls.push({ name, args });
        if (name === 'check_task_status') return absentPane;
        if (name === 'start_super_task') return typedStart(existing ? {} : { outcome: 'started', startedNew: true, configApplied: true });
        if (name === 'get_all_tasks') return [snapshot];
        if (name === 'refresh_task_progress') return;
        assert.fail(name);
      };
      const clear = h.store.clearTaskStop; let clears = 0;
      h.store.clearTaskStop = (...args) => { clears++; return clear(...args); };
      await h.state.openStartModal(h.store.tasks[0]);
      await h.state.handleStartSuperTask({ taskName: snapshot.name, isRemote: true }); await tick();
      assert.equal(calls.filter(c => c.name === 'start_super_task').length, 1);
      assert.equal(clears, existing ? 0 : 1);
      assert.equal(h.hooks.errors.length, 0);
      assert.match(h.hooks.notices.at(-1).text, existing ? /原任务已运行.*未新启动.*未应用新配置/ : /任务启动已确认/);
    } finally { h.stop(); }
  }
});

test('v5 typed failure: rejected or incorrectly resolved ok=false preserves live facts without claiming success or applying config', async () => {
  for (const comparison of ['different', 'unverified']) for (const reject of [true, false]) {
    const h = await harness(startableTask({ taskType: 'super' }));
    try {
      const outcome = typedStart({ ok: false, requestSatisfied: false, configComparison: comparison, error: 'original consumer continues' });
      const before = JSON.stringify(h.store.tasks);
      const calls = [];
      h.hooks.invoke = async (name, args) => {
        calls.push({ name, args });
        if (name === 'check_task_status') return absentPane;
        assert.equal(name, 'start_super_task');
        if (reject) throw JSON.stringify(outcome);
        return outcome;
      };
      await h.state.openStartModal(h.store.tasks[0]);
      await h.state.handleStartSuperTask({ taskName: 'priority-a', isRemote: true, config: { requested: 'changed' } });
      assert.equal(h.hooks.notices.filter(n => n.type === 'success').length, 0);
      assert.match(h.hooks.notices.at(-1).text, /原消费者仍在运行.*本次配置未应用.*继续查看原任务/);
      assert.equal(JSON.stringify(h.store.tasks), before, 'no false local proof/config state change');
      assert.deepEqual(calls.map(c => c.name), ['start_super_task']);
    } finally { h.stop(); }
  }
});

test('v5 typed failure is never inferred from free text, foreign identity/host or contradictory flags', async () => {
  for (const error of [
    `prefix ${JSON.stringify(typedStart({ ok: false, requestSatisfied: false, configComparison: 'different' }))}`,
    JSON.stringify(typedStart({ ok: false, taskId: 'another', requestSatisfied: false, configComparison: 'different' })),
    JSON.stringify(typedStart({ ok: false, remote: false, requestSatisfied: false, configComparison: 'different' })),
    JSON.stringify(typedStart({ ok: false, startedNew: true, requestSatisfied: false, configComparison: 'different' })),
  ]) {
    const h = await harness(startableTask({ taskType: 'super' }));
    try {
      h.hooks.invoke = async name => { if (name === 'check_task_status') return absentPane; throw error; };
      await h.state.openStartModal(h.store.tasks[0]);
      await h.state.handleStartSuperTask({ taskName: 'priority-a', isRemote: true });
      assert.equal(h.hooks.notices.length, 0, 'no unverified live/idempotent claim');
      assert.match(h.hooks.errors.at(-1)[0], /任务启动失败/);
    } finally { h.stop(); }
  }
});

test('v5 malformed success receipt cannot become a GUI success', async () => {
  for (const output of [{}, typedStart({ taskId: 'foreign' }), typedStart({ requestSatisfied: false }), typedStart({ configApplied: true })]) {
    const h = await harness(startableTask({ taskType: 'super' }));
    try {
      h.hooks.invoke = async name => name === 'check_task_status' ? absentPane : output;
      await h.state.openStartModal(h.store.tasks[0]);
      await h.state.handleStartSuperTask({ taskName: 'priority-a', isRemote: true });
      assert.equal(h.hooks.notices.filter(n => n.type === 'success').length, 0);
      assert.equal(h.hooks.errors.length, 1);
    } finally { h.stop(); }
  }
});

test('v5 typed Priority refusal may preserve the original actual-host consumer without granting a requested host migration', async () => {
  const initial = task({ status: 'pending', isRemote: false, targetRemote: true });
  const h = await harness(initial);
  try {
    const calls = [];
    h.hooks.invoke = async (name, args) => {
      calls.push({ name, args }); assert.equal(name, 'start_priority_task');
      throw JSON.stringify(typedStart({ ok: false, remote: false, requestSatisfied: false, configComparison: 'unverified', error: 'original local consumer remains' }));
    };
    await h.state.openStartModal(h.store.tasks[0]);
    await h.state.handlePriorityStartTask({ taskName: initial.name, isRemote: false, config: {} });
    assert.equal(calls[0].args.isRemote, true); assert.equal(calls[0].args.expectedRemote, false);
    assert.equal(h.store.tasks[0].isRemote, false);
    assert.equal(h.hooks.notices.filter(n => n.type === 'success').length, 0);
    assert.match(h.hooks.notices.at(-1).text, /原消费者仍在运行.*本次配置未应用/);
    assert.equal(calls.length, 1);
  } finally { h.stop(); }
});

test('v5 typed early refusal keeps concrete original error without inventing consumer identity', async () => {
  const h = await harness(startableTask({ taskType: 'super' }));
  try {
    const refusal = { schemaVersion: 1, command: 'start', task: 'priority-a', ok: false, outcome: 'refused',
      startedNew: false, configApplied: false, requestSatisfied: false, error: 'original execution identity unresolved' };
    h.hooks.invoke = async name => { if (name === 'check_task_status') return absentPane; throw JSON.stringify(refusal); };
    await h.state.openStartModal(h.store.tasks[0]);
    await h.state.handleStartSuperTask({ taskName: 'priority-a', isRemote: true });
    assert.equal(h.hooks.notices.length, 0);
    assert.equal(h.hooks.errors.at(-1)[0], '任务启动失败：original execution identity unresolved');
  } finally { h.stop(); }
});

test('v5 legacy void null remains usable but is never upgraded to an already-running or exact-launch receipt', async () => {
  const h = await harness(startableTask({ taskType: 'super' }));
  try {
    const initial = { ...h.store.tasks[0] }; const calls = [];
    h.hooks.invoke = async name => {
      calls.push(name);
      if (name === 'check_task_status') return absentPane;
      if (name === 'start_super_task') return null; // serde unit/old Tauri command
      if (name === 'get_all_tasks') return [initial];
      if (name === 'refresh_task_progress') return;
      assert.fail(name);
    };
    await h.state.openStartModal(h.store.tasks[0]);
    await h.state.handleStartSuperTask({ taskName: initial.name, isRemote: true }); await tick();
    assert.equal(h.hooks.errors.length, 0);
    assert.match(h.hooks.notices.at(-1).text, /任务启动已确认/);
    assert.doesNotMatch(h.hooks.notices.at(-1).text, /原任务已运行|配置已应用/);
    assert.equal(calls.filter(name => name === 'pause_task').length, 0);
  } finally { h.stop(); }
});

const settledStart = (extra = {}) => typedStart({ outcome: 'started', startedNew: true, configApplied: true,
  taskType: 'super', authProfile: 'user1', launchAttemptId: 'launch-owned', operationSettled: true, ...extra });
async function ownStartHarness(extra = {}) {
  const initial = task({ taskType: 'super', status: 'ready', launchAttemptId: 'launch-before', ...extra });
  const h = await harness(initial);
  const native = deferred(), fresh = deferred(), stop = deferred(), calls = [];
  const latest = { ...initial, status: 'running', launchAttemptId: 'launch-owned' };
  h.hooks.invoke = (name, args) => {
    calls.push({ name, args });
    if (name === 'check_task_status') return Promise.resolve(`❌ Session '${initial.name}' 不存在 (${initial.isRemote ? 'remote' : 'local'})`);
    if (name.startsWith('start_')) return native.promise;
    if (name === 'get_all_tasks') return fresh.promise;
    if (name === 'refresh_task_progress') return Promise.resolve();
    if (name === 'pause_task') return stop.promise;
    assert.fail(name);
  };
  await h.state.openStartModal(h.store.tasks[0]);
  const start = h.state[initial.taskType === 'priority' ? 'handlePriorityStartTask' : initial.taskType === 'regular' ? 'handleStartTask' : 'handleStartSuperTask']({ taskName: initial.name, isRemote: initial.isRemote });
  await tick();
  assert.equal(calls.filter(c => c.name.startsWith('start_')).length, 1);
  return { ...h, native, fresh, stopReply: stop, ownCalls: calls, starting: start, latest };
}
const advanceJoinTime = async (h, ms) => {
  h.hooks.now += ms;
  for (const [id, timer] of [...h.hooks.timers].sort((a, b) => a[1].at - b[1].at)) {
    if (timer.at <= h.hooks.now && h.hooks.timers.has(id)) { h.hooks.timers.delete(id); timer.callback(); }
  }
  await tick();
};

test('v6 join positive: one owned start -> one clicked pause succeeds through original stop, with one promise/reservation', async () => {
  const h = await ownStartHarness();
  try {
    const pausing = h.state.pauseTask(h.store.tasks[0]);
    assert.equal(h.state.pauseTask(h.store.tasks[0]), pausing, 'duplicate intent shares the same Promise');
    assert.match(h.state.taskOperationDetail(h.store.tasks[0]), /尚未发送停止/);
    assert.equal(h.store.taskStopOperationMap['priority-a'], undefined, 'waiting intent is not a sent stop');
    h.native.resolve(settledStart()); await h.starting; await tick();
    assert.equal(h.state.mutationRequests.value.has('id-a'), true, 'start finally must not release reservation');
    assert.equal(h.ownCalls.filter(c => c.name === 'get_all_tasks').length, 1, 'fresh read is not duplicate start UI refresh');
    await h.state.syncRemoteTask('priority-a');
    await h.state.generateList('id-a');
    await h.state.updateIsRemote('id-a', false);
    h.state.deleteTask('id-a'); await h.hooks.dialog.onPositiveClick();
    assert.equal(h.ownCalls.some(c => ['sync_remote_task', 'generate_list', 'update_remote_status', 'delete_task'].includes(c.name)), false);
    h.fresh.resolve([h.latest]); await tick(); await tick();
    const pauses = h.ownCalls.filter(c => c.name === 'pause_task');
    assert.equal(pauses.length, 1);
    assert.equal(pauses[0].args.expectedLaunchAttemptId, 'launch-owned');
    assert.equal(Object.hasOwn(pauses[0].args, 'expectedLaunchAbsent'), false);
    assert.equal(pauses[0].args.remainingMs, 120000);
    assert.equal(h.state.mutationRequests.value.has('id-a'), true);
    h.stopReply.resolve(receipt());
    assert.equal(await pausing, true);
    assert.equal(h.store.tasks[0].status, 'paused');
    assert.equal(h.state.mutationRequests.value.has('id-a'), false);
    assert.equal(h.hooks.timers.size, 0);
    assert.equal(h.hooks.messages.filter(m => /尚未发送停止/.test(m.text)).every(m => m.destroyed), true);
  } finally { h.native.resolve(settledStart()); h.fresh.resolve([h.latest]); h.stopReply.resolve(receipt()); h.stop(); }
});

test('v6 join explicit observed null uses must-be-absent, never a guessed latest launch', async () => {
  const h = await ownStartHarness({ launchAttemptId: null });
  try {
    const pausing = h.state.pauseTask(h.store.tasks[0]);
    h.native.resolve(settledStart({ launchAttemptId: null })); await h.starting; await tick();
    h.fresh.resolve([{ ...h.latest, launchAttemptId: null }]); await tick(); await tick();
    const call = h.ownCalls.find(c => c.name === 'pause_task');
    assert.ok(call); assert.equal(call.args.expectedLaunchAbsent, true);
    assert.equal(Object.hasOwn(call.args, 'expectedLaunchAttemptId'), false);
    h.stopReply.resolve(receipt({ confirmationBasis: 'session_absent' }));
    assert.equal(await pausing, true);
  } finally { h.native.resolve(settledStart()); h.fresh.resolve([h.latest]); h.stopReply.resolve(receipt()); h.stop(); }
});

test('v6 join settled partial remains a failed config request but permits the independent same-click pause', async () => {
  const h = await ownStartHarness();
  try {
    const pausing = h.state.pauseTask(h.store.tasks[0]);
    h.native.reject(JSON.stringify(settledStart({ ok: false, outcome: 'already_running', startedNew: false,
      configApplied: false, requestSatisfied: false, configComparison: 'different', launchAttemptId: 'launch-before', error: 'config not applied' })));
    await h.starting; await tick();
    h.fresh.resolve([{ ...h.latest, launchAttemptId: 'launch-before' }]); await tick(); await tick();
    const call = h.ownCalls.find(c => c.name === 'pause_task'); assert.ok(call);
    assert.equal(call.args.expectedLaunchAttemptId, 'launch-before');
    h.stopReply.resolve(receipt()); assert.equal(await pausing, true);
    assert.ok(h.hooks.notices.some(n => n.type === 'warning' && /本次配置未应用/.test(n.text)));
    assert.equal(h.ownCalls.filter(c => c.name.startsWith('start_')).length, 1);
    assert.equal(h.ownCalls.filter(c => c.name === 'pause_task').length, 1);
  } finally { h.native.resolve(settledStart()); h.fresh.resolve([h.latest]); h.stopReply.resolve(receipt()); h.stop(); }
});

test('v6 join Priority follows only this settled native-approved actual host, never saved target intent alone', async () => {
  const h = await ownStartHarness({ taskType: 'priority', status: 'pending', isRemote: false, targetRemote: true, launchAttemptId: null });
  try {
    const pausing = h.state.pauseTask(h.store.tasks[0]);
    h.native.resolve(settledStart({ taskType: 'priority', remote: true })); await h.starting; await tick();
    h.fresh.resolve([{ ...h.latest, isRemote: true }]); await tick(); await tick();
    const call = h.ownCalls.find(c => c.name === 'pause_task');
    assert.ok(call); assert.equal(call.args.isRemote, true); assert.equal(call.args.expectedLaunchAttemptId, 'launch-owned');
    h.stopReply.resolve(receipt()); assert.equal(await pausing, true);
    assert.equal(h.store.tasks[0].isRemote, true); assert.equal(h.store.tasks[0].status, 'paused');
  } finally { h.native.resolve(settledStart()); h.fresh.resolve([h.latest]); h.stopReply.resolve(receipt()); h.stop(); }
});

test('v6 join missing/malformed identity or settlement, old void and arbitrary errors never become a pause authorization', async () => {
  const omittedLaunch = settledStart(); delete omittedLaunch.launchAttemptId;
  const cases = [omittedLaunch, settledStart({ launchAttemptId: '' }), settledStart({ launchAttemptId: false }),
    settledStart({ launchAttemptId: {} }), settledStart({ operationSettled: false }), settledStart({ operationSettled: undefined }),
    settledStart({ taskType: 'regular' }), settledStart({ authProfile: 'other' }), settledStart({ taskId: 'foreign' }),
    settledStart({ remote: false }), null, undefined,
    { rejection: 'timeout with guessed launchAttemptId=launch-owned' },
    { rejection: JSON.stringify(settledStart({ ok: false, outcome: 'already_running', startedNew: false, configApplied: false,
      configComparison: 'unverified', requestSatisfied: false, operationSettled: false })) }];
  for (const outcome of cases) {
    const h = await ownStartHarness();
    try {
      const pausing = h.state.pauseTask(h.store.tasks[0]);
      if (outcome?.rejection) h.native.reject(outcome.rejection); else h.native.resolve(outcome);
      await h.starting; await tick();
      assert.equal(await pausing, false);
      assert.equal(h.ownCalls.filter(c => c.name === 'pause_task').length, 0);
      assert.equal(h.store.taskStopOperationMap['priority-a'], undefined);
      assert.equal(h.hooks.timers.size, 0);
    } finally { h.native.resolve(settledStart()); h.fresh.resolve([h.latest]); h.stopReply.resolve(receipt()); h.stop(); }
  }
});

test('v6 join fresh identity/host/type/profile/launch/source conflicts stop before native pause; no same-name adoption', async () => {
  for (const patch of [{ _id: 'successor-id' }, { name: 'other-name' }, { taskType: 'regular' }, { auth_profile: 'other' },
    { isRemote: false }, { launchAttemptId: 'successor-launch' }, { launchAttemptId: false },
    { status: 'deactive' }, { controlSource: 'remote_only' }]) {
    const h = await ownStartHarness();
    try {
      const pausing = h.state.pauseTask(h.store.tasks[0]);
      h.native.resolve(settledStart()); await h.starting; await tick();
      h.fresh.resolve([{ ...h.latest, ...patch }]); await tick();
      assert.equal(await pausing, false, JSON.stringify(patch));
      assert.equal(h.ownCalls.filter(c => c.name === 'pause_task').length, 0);
      assert.equal(h.hooks.timers.size, 0);
    } finally { h.native.resolve(settledStart()); h.fresh.resolve([h.latest]); h.stopReply.resolve(receipt()); h.stop(); }
  }
});

test('v6 join identity away-and-back or intervening foreign launch cannot revive an old intent', async () => {
  for (const patch of [{ _id: 'other-id' }, { auth_profile: 'other' }, { launchAttemptId: 'foreign-then-returned' }]) {
    const h = await ownStartHarness();
    try {
      const before = { ...h.store.tasks[0] };
      const pausing = h.state.pauseTask(h.store.tasks[0]);
      h.store.replaceTasks([{ ...before, ...patch }]); h.store.replaceTasks([before]);
      h.native.resolve(settledStart()); await h.starting; await tick();
      h.fresh.resolve([h.latest]);
      assert.equal(await pausing, false);
      assert.equal(h.ownCalls.filter(c => c.name === 'pause_task').length, 0);
    } finally { h.native.resolve(settledStart()); h.fresh.resolve([h.latest]); h.stopReply.resolve(receipt()); h.stop(); }
  }
});

test('v6 join reserves actual bounded path budgets, shares original 120s deadline, and never resets remaining at dispatch', async () => {
  const h = await ownStartHarness();
  try {
    const budget = h.state.pauseJoinBudget;
    const wait = budget.ui - budget.fresh - budget.adapter - budget.core - budget.reply;
    const pausing = h.state.pauseTask(h.store.tasks[0]);
    assert.equal([...h.hooks.timers.values()][0].delay, wait);
    await advanceJoinTime(h, 20000);
    h.native.resolve(settledStart()); await h.starting; await tick();
    assert.equal([...h.hooks.timers.values()][0].delay, budget.fresh);
    await advanceJoinTime(h, 6000);
    h.fresh.resolve([h.latest]); await tick(); await tick();
    const call = h.ownCalls.find(c => c.name === 'pause_task'); assert.ok(call);
    assert.equal(call.args.remainingMs, budget.ui - 26000);
    assert.ok(call.args.remainingMs >= budget.adapter + budget.core + budget.reply);
    assert.equal(h.hooks.timers.size, 1, 'original pause owns the only stop wait timer');
    assert.equal([...h.hooks.timers.values()][0].at, budget.ui);
    h.stopReply.resolve(receipt()); assert.equal(await pausing, true);
    assert.equal(h.hooks.timers.size, 0);
  } finally { h.native.resolve(settledStart()); h.fresh.resolve([h.latest]); h.stopReply.resolve(receipt()); h.stop(); }
});

test('v6 join wait deadline/unmount cancels unsent continuation; late native completion cannot issue pause', async () => {
  for (const cause of ['deadline', 'unmount']) {
    const h = await ownStartHarness();
    try {
      const pausing = h.state.pauseTask(h.store.tasks[0]);
      if (cause === 'deadline') {
        const b = h.state.pauseJoinBudget;
        await advanceJoinTime(h, b.ui - b.fresh - b.adapter - b.core - b.reply);
      } else h.stop();
      assert.equal(await pausing, false);
      h.native.resolve(settledStart()); await h.starting; await tick(); h.fresh.resolve([h.latest]); await tick();
      assert.equal(h.ownCalls.filter(c => c.name === 'pause_task').length, 0);
      assert.equal(h.hooks.timers.size, 0);
    } finally { h.native.resolve(settledStart()); h.fresh.resolve([h.latest]); h.stopReply.resolve(receipt()); h.stop(); }
  }
});

test('v6 join bounded fresh failure/timeout invalidates late results and cannot defer a stop', async () => {
  for (const cause of ['timeout', 'read-error']) {
    const h = await ownStartHarness();
    try {
      const pausing = h.state.pauseTask(h.store.tasks[0]);
      h.native.resolve(settledStart()); await h.starting; await tick();
      if (cause === 'timeout') await advanceJoinTime(h, h.state.pauseJoinBudget.fresh);
      else h.fresh.reject(new Error('fresh original list unavailable'));
      assert.equal(await pausing, false);
      h.fresh.resolve([h.latest]); await tick();
      assert.equal(h.ownCalls.filter(c => c.name === 'pause_task').length, 0);
      assert.equal(h.hooks.timers.size, 0);
    } finally { h.native.resolve(settledStart()); h.fresh.resolve([h.latest]); h.stopReply.resolve(receipt()); h.stop(); }
  }
});

test('v6 joined stop passes exact fence for native lock recheck; successor rejection is not retried or reported paused', async () => {
  const h = await ownStartHarness();
  try {
    let signals = 0;
    const invoke = h.hooks.invoke;
    h.hooks.invoke = (name, args) => {
      if (name !== 'pause_task') return invoke(name, args);
      h.ownCalls.push({ name, args });
      // Boundary fixture models a successor appearing AFTER the fresh read.
      // Actual lock/stop-runtime rejection is a core-writer native test, not a JS proof.
      assert.equal(args.expectedLaunchAttemptId, 'launch-owned');
      h.store.replaceTasks([{ ...h.latest, launchAttemptId: 'successor' }]);
      if (args.expectedLaunchAttemptId !== 'successor') return Promise.reject(new Error('native exact launch conflict before stop'));
      signals++; return Promise.resolve(receipt());
    };
    const pausing = h.state.pauseTask(h.store.tasks[0]);
    h.native.resolve(settledStart()); await h.starting; await tick();
    h.fresh.resolve([h.latest]);
    assert.equal(await pausing, false);
    assert.equal(signals, 0);
    assert.equal(h.store.tasks[0].status, 'running');
    assert.equal(h.store.tasks[0].launchAttemptId, 'successor');
    assert.equal(h.ownCalls.filter(c => c.name === 'pause_task').length, 1);
  } finally { h.native.resolve(settledStart()); h.fresh.resolve([h.latest]); h.stopReply.resolve(receipt()); h.stop(); }
});

test('v6 sent stop unmount/late receipt cannot falsely cancel stop or overwrite a successor launch', async () => {
  for (const cause of ['unmount', 'successor', 'profile', 'type']) {
    const h = await ownStartHarness();
    try {
      const pausing = h.state.pauseTask(h.store.tasks[0]);
      h.native.resolve(settledStart()); await h.starting; await tick();
      h.fresh.resolve([h.latest]); await tick(); await tick();
      assert.equal(h.ownCalls.filter(c => c.name === 'pause_task').length, 1);
      if (cause === 'unmount') h.stop();
      else h.store.replaceTasks([{ ...h.latest, ...(cause === 'profile' ? { auth_profile: 'other' } : cause === 'type' ? { taskType: 'regular' } : { launchAttemptId: 'newer-run' }) }]);
      assert.equal(await pausing, false);
      h.stopReply.resolve(receipt()); await tick(); await tick();
      assert.equal(h.store.tasks[0].status, 'running');
      if (cause !== 'unmount') assert.equal(h.store.taskStopOperationMap['priority-a'], undefined);
      else assert.equal(h.store.taskStopOperationMap['priority-a'].status, 'stop_unconfirmed');
      assert.equal(h.hooks.timers.size, 0);
    } finally { h.native.resolve(settledStart()); h.fresh.resolve([h.latest]); h.stopReply.resolve(receipt()); h.stop(); }
  }
});

test('v6 join confirmed pause releases owner for a legal next start on the same task without stale stop proof', async () => {
  const h = await ownStartHarness();
  try {
    const pausing = h.state.pauseTask(h.store.tasks[0]);
    h.native.resolve(settledStart()); await h.starting; await tick();
    h.fresh.resolve([h.latest]); await tick(); await tick();
    h.stopReply.resolve(receipt()); assert.equal(await pausing, true);
    assert.equal(h.store.tasks[0].status, 'paused');
    const after = { ...h.latest, status: 'running', launchAttemptId: 'next-legitimate-launch' };
    h.hooks.invoke = async (name, args) => {
      h.ownCalls.push({ name, args });
      if (name === 'check_task_status') return absentPane;
      if (name === 'start_super_task') return settledStart({ launchAttemptId: after.launchAttemptId });
      if (name === 'get_all_tasks') return [after];
      if (name === 'refresh_task_progress') return;
      assert.fail(name);
    };
    await h.state.openStartModal(h.store.tasks[0]);
    await h.state.handleStartSuperTask({ taskName: 'priority-a', isRemote: true }); await tick();
    assert.equal(h.ownCalls.filter(c => c.name === 'start_super_task').length, 2);
    assert.equal(h.ownCalls.filter(c => c.name === 'pause_task').length, 1);
    assert.equal(h.store.tasks[0].status, 'running');
    assert.equal(h.store.tasks[0].launchAttemptId, after.launchAttemptId);
    assert.equal(h.store.taskStopOperationMap['priority-a'], undefined);
    assert.equal(h.state.mutationRequests.value.size, 0);
  } finally { h.native.resolve(settledStart()); h.fresh.resolve([h.latest]); h.stopReply.resolve(receipt()); h.stop(); }
});

test('v6 join already-sent stop deadline preserves unconfirmed state and late receipt never becomes a second action', async () => {
  const h = await ownStartHarness();
  try {
    const pausing = h.state.pauseTask(h.store.tasks[0]);
    const rejected = assert.rejects(pausing, /停止确认超时/);
    h.native.resolve(settledStart()); await h.starting; await tick();
    h.fresh.resolve([h.latest]); await tick(); await tick();
    assert.equal(h.ownCalls.filter(c => c.name === 'pause_task').length, 1);
    await advanceJoinTime(h, h.state.pauseJoinBudget.ui);
    await rejected;
    assert.equal(h.store.taskStopOperationMap['priority-a'].status, 'stop_unconfirmed');
    assert.equal(h.store.tasks[0].status, 'running');
    h.stopReply.resolve(receipt()); await tick();
    assert.equal(h.store.taskStopOperationMap['priority-a'].status, 'stop_unconfirmed');
    assert.equal(h.ownCalls.filter(c => c.name === 'pause_task').length, 1);
    assert.equal(h.hooks.timers.size, 0);
  } finally { h.native.resolve(settledStart()); h.fresh.resolve([h.latest]); h.stopReply.resolve(receipt()); h.stop(); }
});

test('v6 regular successful start and positively settled preflight refusal both honor the same single pause intent', async () => {
  for (const refused of [false, true]) {
    const h = await ownStartHarness({ taskType: 'regular' });
    try {
      const pausing = h.state.pauseTask(h.store.tasks[0]);
      if (refused) h.native.reject(JSON.stringify({ schemaVersion: 1, command: 'start', task: 'priority-a', taskId: 'id-a', remote: true,
        taskType: 'regular', authProfile: 'user1', launchAttemptId: 'launch-before', operationSettled: true,
        ok: false, outcome: 'refused', startedNew: false, configApplied: false, requestSatisfied: false, error: 'settled preflight refusal' }));
      else h.native.resolve(settledStart({ taskType: 'regular' }));
      await h.starting; await tick();
      const launch = refused ? 'launch-before' : 'launch-owned';
      h.fresh.resolve([{ ...h.latest, launchAttemptId: launch }]); await tick(); await tick();
      const call = h.ownCalls.find(c => c.name === 'pause_task'); assert.ok(call);
      assert.equal(call.args.expectedLaunchAttemptId, launch);
      h.stopReply.resolve(receipt()); assert.equal(await pausing, true);
      assert.equal(h.ownCalls.filter(c => c.name === 'start_task').length, 1);
      assert.equal(h.ownCalls.filter(c => c.name === 'pause_task').length, 1);
      if (refused) assert.ok(h.hooks.errors.some(([message]) => /settled preflight refusal/.test(message)));
    } finally { h.native.resolve(settledStart()); h.fresh.resolve([h.latest]); h.stopReply.resolve(receipt()); h.stop(); }
  }
});

test('v6 explicit no-join option still does not create a deferred user join intent', async () => {
  const h = await ownStartHarness();
  try {
    assert.equal(await h.state.pauseTask(h.store.tasks[0], { joinOwnedStart: false }), false);
    assert.equal(h.state.mutationRequests.value.get('id-a').pauseIntent, null);
    h.native.resolve(settledStart()); await h.starting; h.fresh.resolve([h.latest]); await tick();
    assert.equal(h.ownCalls.filter(c => c.name === 'pause_task').length, 0);
    assert.doesNotMatch(source, /createAutoPauseScheduler/);
  } finally { h.native.resolve(settledStart()); h.fresh.resolve([h.latest]); h.stopReply.resolve(receipt()); h.stop(); }
});

test('v8 normal open is local-only and one submit reaches core without PID/proof preflight across control states and all task types', async () => {
  for (const taskType of ['regular', 'super', 'priority']) {
    for (const status of ['ready', 'running', 'launch_unknown', 'completed', 'completed_with_errors', 'paused', 'generation_unknown', 'unknown']) {
      const initial = task({ taskType, status });
      const h = await harness(initial);
      try {
        const calls = [];
        h.hooks.invoke = async (name, args) => {
          calls.push({ name, args });
          if (name.startsWith('start_')) {
            if (status === 'generation_unknown') throw JSON.stringify({ schemaVersion: 1, command: 'start', task: initial.name,
              ok: false, outcome: 'refused', startedNew: false, configApplied: false, requestSatisfied: false, error: 'original generation owner must reconcile' });
            return settledStart({ taskType, outcome: 'already_running', startedNew: false, configApplied: false });
          }
          if (name === 'get_all_tasks') return [initial];
          if (name === 'refresh_task_progress') return;
          assert.fail(`normal action must not inspect processes: ${name}`);
        };
        await h.state.openStartModal(h.store.tasks[0]);
        assert.equal(h.state.showStartModal.value, true, `${taskType}:${status}`);
        assert.deepEqual(calls, [], 'open must not query native');
        await h.state[taskType === 'priority' ? 'handlePriorityStartTask' : taskType === 'super' ? 'handleStartSuperTask' : 'handleStartTask']({ taskName: initial.name, isRemote: true });
        await tick();
        const starts = calls.filter(c => c.name.startsWith('start_'));
        assert.equal(starts.length, 1, `${taskType}:${status}`);
        assert.deepEqual(starts[0].args, { taskName: initial.name, taskId: 'id-a', isRemote: true, expectedRemote: true });
        assert.equal(calls.filter(c => c.name === 'check_task_status').length, 0);
        if (status === 'generation_unknown') assert.match(h.hooks.errors.at(-1)[0], /original generation owner must reconcile/);
        else assert.match(h.hooks.notices.at(-1).text, /原任务已运行/);
      } finally { h.stop(); }
    }
  }
});

test('v8 cached PID/absence/unknown/stale/wrong-identity observations cannot grant or block core start authority', async () => {
  for (const observation of [null, { state: 'running' }, { state: 'not_running' }, { state: 'unknown', error: 'PID query failed' },
    { state: 'session_absent' }, { state: 'running', taskId: 'foreign', observedAt: 1 }]) {
    const initial = regularTask({ taskType: 'super', status: 'completed_with_errors' });
    const h = await harness(initial);
    try {
      if (observation) h.state.taskRuntimeStatus.value[initial.name] = { ...h.state.runtimeTarget(initial), observedAt: Date.now(), ...observation };
      const calls = [];
      h.hooks.invoke = async (name, args) => {
        calls.push({ name, args });
        if (name === 'start_super_task') throw JSON.stringify(settledStart({ ok: false, outcome: 'already_running', startedNew: false,
          configApplied: false, requestSatisfied: false, configComparison: 'unverified', error: 'core retained original consumer' }));
        assert.fail(`unexpected query/mutation: ${name}`);
      };
      await h.state.openStartModal(h.store.tasks[0]);
      await h.state.handleStartSuperTask({ taskName: initial.name, isRemote: true, config: { changed: true } });
      assert.deepEqual(calls.map(c => c.name), ['start_super_task']);
      assert.equal(h.hooks.notices.filter(n => n.type === 'success').length, 0);
      assert.match(h.hooks.notices.at(-1).text, /原消费者仍在运行.*配置未应用/);
      assert.equal(h.store.tasks[0].status, 'completed_with_errors', 'UI does not write process/control truth');
    } finally { h.stop(); }
  }
});

test('v8 only view-owned in-flight work blocks start; a returned unconfirmed stop leaves reconciliation to core', async () => {
  for (const busy of ['mutation', 'generation', 'stopping', 'returned-unconfirmed']) {
    const initial = regularTask({ status: 'paused' });
    const h = await harness(initial);
    try {
      if (busy === 'mutation') h.state.mutationRequests.value.set('id-a', { kind: 'mutation' });
      if (busy === 'generation') h.state.generationRequests.value['id-a'] = true;
      if (busy === 'stopping' || busy === 'returned-unconfirmed') {
        const operation = h.store.beginTaskStop(initial);
        if (busy === 'returned-unconfirmed') h.store.markTaskStopUnconfirmed(initial, operation, 'old response uncertain', 'client_timeout');
      }
      const calls = [];
      h.hooks.invoke = async name => { calls.push(name); throw new Error('core refuses unresolved stop; no optimistic success'); };
      await h.state.openStartModal(h.store.tasks[0]);
      if (busy !== 'returned-unconfirmed') {
        assert.equal(h.state.showStartModal.value, false); assert.deepEqual(calls, []);
      } else {
        assert.equal(h.state.showStartModal.value, true);
        await h.state.handleStartTask({ taskName: initial.name, isRemote: true });
        assert.deepEqual(calls, ['start_task']);
        assert.match(h.hooks.errors.at(-1)[0], /core refuses unresolved stop/);
        assert.equal(h.store.taskStopOperationMap[initial.name].status, 'stop_unconfirmed');
      }
    } finally { h.stop(); }
  }
});

test('v8 modal fences task/source/type/profile replacement but a same-task launch epoch is decided by core, not cached PID metadata', async () => {
  for (const patch of [{ _id: 'replacement' }, { isRemote: false }, { auth_profile: 'other' }, { taskType: 'priority' }, { launchAttemptId: 'later-run' }]) {
    const initial = regularTask({ status: 'ready', launchAttemptId: 'before' });
    const h = await harness(initial);
    try {
      const calls = [];
      h.hooks.invoke = async name => { calls.push(name); throw new Error('original core decision'); };
      await h.state.openStartModal(h.store.tasks[0]);
      h.store.replaceTasks([{ ...initial, ...patch }]);
      await h.state.handleStartTask({ taskName: initial.name, isRemote: true });
      assert.equal(calls.length, Object.hasOwn(patch, 'launchAttemptId') ? 1 : 0);
      assert.ok(calls.every(name => name === 'start_task'));
    } finally { h.stop(); }
  }
});

test('v8 mount/history adds no process queries or interval; progress refresh never auto-pauses', async () => {
  const active = regularTask({ _id: 'active-id', name: 'business-progress' });
  let rows = [active, ...Array.from({ length: 1453 }, (_, i) => regularTask({ _id: `old-${i}`, name: `history-${i}`,
    status: 'completed_with_errors', launchAttemptId: `old-launch-${i}` }))];
  const h = await harness(active);
  try {
    const calls = [];
    h.hooks.invoke = async (name, args) => {
      calls.push({ name, args });
      if (name === 'frontend_ready' || name === 'refresh_task_progress') return;
      if (name === 'get_all_tasks') return rows;
      if (name === 'pause_task') {
        rows = [{ ...active, status: 'paused', stopConfirmedAt: proofAt, stopConfirmedTaskId: 'active-id', stopConfirmedRemote: true }, ...rows.slice(1)];
        return receipt({ taskId: 'active-id', taskName: active.name });
      }
      assert.fail(`background PID query is not an authorized progress dependency: ${name}`);
    };
    await h.hooks.mount(); await tick();
    assert.deepEqual(calls.map(c => c.name), ['frontend_ready', 'get_all_tasks', 'refresh_task_progress']);
    assert.equal(h.store.tasks.length, 1454);
    assert.equal(h.hooks.intervals.size, 0);
    assert.deepEqual(Object.keys(h.state.taskRuntimeStatus.value), []);
    assert.equal(h.state.pollTaskStatus, undefined);
    const emitProgress = success => h.hooks.handlers.get('task-progress-update')({ payload: {
      collection: active.name, task_id: 'active-id', is_remote: true, updated_at: 1000 + success,
      success, total: 2, error: 0, priority_success: 0, priority_error: 0, priority_total: 0,
    } });
    emitProgress(1); await tick();
    assert.equal(h.store.taskProgressMap[active.name].success, 1);
    assert.equal(h.hooks.timers.size, 0);
    emitProgress(2); await tick();
    assert.equal(h.store.taskProgressMap[active.name].success, 2);
    assert.equal(h.hooks.timers.size, 0, 'full counts must not register an automatic stop');
    await advanceJoinTime(h, 300000); await tick(); await tick();
    assert.equal(calls.filter(c => c.name === 'pause_task').length, 0);
    assert.equal(calls.filter(c => c.name === 'check_task_status').length, 0);
    assert.equal(h.store.tasks[0].status, 'running');
    assert.deepEqual(calls.map(c => c.name), ['frontend_ready', 'get_all_tasks', 'refresh_task_progress']);
  } finally { h.stop(); }
});

test('v8 explicit optional diagnostic is read-only and does not cause pause, status writes or prerequisite state', async () => {
  const initial = regularTask(); const h = await harness(initial);
  try {
    const before = JSON.stringify(h.store.tasks);
    const check = h.state.checkTaskStatus(h.store.tasks[0]);
    h.probes[0].resolve(absentPane); await check;
    assert.deepEqual(h.calls.map(c => c.name), ['check_task_status']);
    assert.equal(JSON.stringify(h.store.tasks), before);
    assert.deepEqual(h.store.taskStopOperationMap, {});
    assert.equal(h.state.taskRuntimeStatus.value[initial.name].state, 'session_absent');
    assert.equal(h.state.startBlockReason(h.store.tasks[0]), '');
  } finally { h.stop(); }
});

test('running task accepts not-compared receipt for explicit edited JSON without claiming config was applied', async () => {
  const h = await harness(startableTask({ taskType: 'super' }));
  try {
    const snapshot = { ...h.store.tasks[0] }, edited = { max_concurrent: 3, user_extension: { keep: true } };
    let starts = 0;
    h.hooks.invoke = async (name, args) => {
      if (name === 'start_super_task') {
        starts++;
        assert.deepEqual(JSON.parse(args.config), edited);
        return typedStart({ configComparison: 'not_compared' });
      }
      if (name === 'get_all_tasks') return [snapshot];
      if (name === 'refresh_task_progress') return;
      assert.fail(name);
    };
    await h.state.openStartModal(h.store.tasks[0]);
    await h.state.handleStartSuperTask({ taskName: snapshot.name, isRemote: true, config: edited });
    await tick();
    assert.equal(starts, 1);
    assert.equal(h.hooks.errors.length, 0);
    assert.match(h.hooks.notices.at(-1).text, /原任务已运行.*未新启动.*未应用新配置/);
  } finally { h.stop(); }
});

// R1: exact native-generated scope responses, not a manually settled UI stub.
// Refreshed from implementation/review_r1/native scope wire after the fix.
// BEGIN R1 NATIVE SCOPE WIRE
const r1NativeScopeWire = [{"schemaVersion":1,"command":"start","task":"fixture","ok":true,"at":"2026-10-29T02:50:28.325646+00:00","taskId":"6ae2b473f9c53b4a1765adaa","remote":false,"taskType":"priority","authProfile":"user1","launchAttemptId":"old-launch","operationSettled":true,"outcome":"already_running","startedNew":false,"consumerRunning":true,"configApplied":false,"configComparison":"not_compared","requestSatisfied":true,"executionObservation":{"schemaVersion":1,"taskName":"fixture","remote":false,"state":"unverified","consumerRunning":true,"identityVerified":false,"ownedResidue":false,"configComparison":"not_requested","sessionId":"$4","sessionCreatedAt":1700000000000,"bindingBasis":"unverified","pid":1002,"pidCreatedAt":1700000010.0,"consumerRunId":null,"consumerPredatesLaunch":false,"reason":"independent_task_or_launch_binding_unverified","explicitPauseVerified":true,"taskId":"6ae2b473f9c53b4a1765adaa","consumerScopeVerified":true,"consumerWorkdir":"/var/folders/31/tw6qcw8546s40nbg64vtgm4c0000gn/T/quantnight-native-89433-4","processes":[{"pid":1001,"pidCreatedAt":1700000000.0,"argvDigest":"2d4e4fd3a999b340a805afbc8740a13fad92c278a25e18e69917c2d17c6240f2"},{"pid":1002,"pidCreatedAt":1700000010.0,"argvDigest":"f75076004f527d19c75d6064587fca283b54c63041d0372928b05b9cb26e14ea"}],"panes":[{"pid":1001,"dead":false}],"tmuxContext":{"schemaVersion":1,"backend":"tmux","socketName":"default","binary":"/private/var/folders/31/tw6qcw8546s40nbg64vtgm4c0000gn/T/quantnight-native-89433-4/native-tmux","sessionId":"$4","sessionCreatedAt":1700000000000,"taskId":"6ae2b473f9c53b4a1765adaa","launchAttemptId":"old-launch","remote":false}},"controlStatus":"running","controlReconciled":false,"error":null},{"schemaVersion":1,"command":"start","task":"fixture","ok":true,"at":"2026-10-29T02:50:31.527854+00:00","taskId":"6ae2b475f9c53b4a1765adae","remote":true,"taskType":"priority","authProfile":"user1","launchAttemptId":"old-launch","operationSettled":true,"outcome":"already_running","startedNew":false,"consumerRunning":true,"configApplied":false,"configComparison":"not_compared","requestSatisfied":true,"executionObservation":{"schemaVersion":1,"taskName":"fixture","remote":true,"state":"unverified","consumerRunning":true,"identityVerified":false,"ownedResidue":false,"configComparison":"not_requested","sessionId":"$4","sessionCreatedAt":1700000000000,"bindingBasis":"unverified","pid":1002,"pidCreatedAt":1700000010.0,"consumerRunId":null,"consumerPredatesLaunch":false,"reason":"independent_task_or_launch_binding_unverified","explicitPauseVerified":true,"taskId":"6ae2b475f9c53b4a1765adae","consumerScopeVerified":true,"consumerWorkdir":"/var/folders/31/tw6qcw8546s40nbg64vtgm4c0000gn/T/quantnight-native-89433-5","processes":[{"pid":1001,"pidCreatedAt":1700000000.0,"argvDigest":"2d4e4fd3a999b340a805afbc8740a13fad92c278a25e18e69917c2d17c6240f2"},{"pid":1002,"pidCreatedAt":1700000010.0,"argvDigest":"ea565c7dc3b2eb3099752d0f04f0b98e041af04a0e0719e3c99005535438288c"}],"panes":[{"pid":1001,"dead":false}],"tmuxContext":{"schemaVersion":1,"backend":"tmux","socketName":"default","binary":"/private/var/folders/31/tw6qcw8546s40nbg64vtgm4c0000gn/T/quantnight-native-89433-5/native-tmux","sessionId":"$4","sessionCreatedAt":1700000000000,"taskId":"6ae2b475f9c53b4a1765adae","launchAttemptId":"old-launch","remote":true}},"controlStatus":"running","controlReconciled":false,"error":null},{"schemaVersion":1,"command":"start","task":"fixture","ok":true,"at":"2026-10-29T02:50:15.537598+00:00","taskId":"6ae2b466f9c53b4a1765ad9a","remote":false,"taskType":"regular","authProfile":"user1","launchAttemptId":"old-launch","operationSettled":true,"outcome":"already_running","startedNew":false,"consumerRunning":true,"configApplied":false,"configComparison":"not_compared","requestSatisfied":true,"executionObservation":{"schemaVersion":1,"taskName":"fixture","remote":false,"state":"unverified","consumerRunning":true,"identityVerified":false,"ownedResidue":false,"configComparison":"not_requested","sessionId":"$4","sessionCreatedAt":1700000000000,"bindingBasis":"unverified","pid":1002,"pidCreatedAt":1700000010.0,"consumerRunId":null,"consumerPredatesLaunch":false,"reason":"independent_task_or_launch_binding_unverified","explicitPauseVerified":true,"taskId":"6ae2b466f9c53b4a1765ad9a","consumerScopeVerified":true,"consumerWorkdir":"/var/folders/31/tw6qcw8546s40nbg64vtgm4c0000gn/T/quantnight-native-89433-0","processes":[{"pid":1001,"pidCreatedAt":1700000000.0,"argvDigest":"2d4e4fd3a999b340a805afbc8740a13fad92c278a25e18e69917c2d17c6240f2"},{"pid":1002,"pidCreatedAt":1700000010.0,"argvDigest":"692c2a530398211becd49918e64f89a407145979aeacfa36164afa61d06a41fa"}],"panes":[{"pid":1001,"dead":false}],"tmuxContext":{"schemaVersion":1,"backend":"tmux","socketName":"default","binary":"/private/var/folders/31/tw6qcw8546s40nbg64vtgm4c0000gn/T/quantnight-native-89433-0/native-tmux","sessionId":"$4","sessionCreatedAt":1700000000000,"taskId":"6ae2b466f9c53b4a1765ad9a","launchAttemptId":"old-launch","remote":false}},"controlStatus":"running","controlReconciled":false,"error":null},{"schemaVersion":1,"command":"start","task":"fixture","ok":true,"at":"2026-10-29T02:50:18.990769+00:00","taskId":"6ae2b469f9c53b4a1765ad9e","remote":true,"taskType":"regular","authProfile":"user1","launchAttemptId":"old-launch","operationSettled":true,"outcome":"already_running","startedNew":false,"consumerRunning":true,"configApplied":false,"configComparison":"not_compared","requestSatisfied":true,"executionObservation":{"schemaVersion":1,"taskName":"fixture","remote":true,"state":"unverified","consumerRunning":true,"identityVerified":false,"ownedResidue":false,"configComparison":"not_requested","sessionId":"$4","sessionCreatedAt":1700000000000,"bindingBasis":"unverified","pid":1002,"pidCreatedAt":1700000010.0,"consumerRunId":null,"consumerPredatesLaunch":false,"reason":"independent_task_or_launch_binding_unverified","explicitPauseVerified":true,"taskId":"6ae2b469f9c53b4a1765ad9e","consumerScopeVerified":true,"consumerWorkdir":"/var/folders/31/tw6qcw8546s40nbg64vtgm4c0000gn/T/quantnight-native-89433-1","processes":[{"pid":1001,"pidCreatedAt":1700000000.0,"argvDigest":"2d4e4fd3a999b340a805afbc8740a13fad92c278a25e18e69917c2d17c6240f2"},{"pid":1002,"pidCreatedAt":1700000010.0,"argvDigest":"01185715109663a3a0aeae2d45452707aad06b0085c29640da9b4b1288f60141"}],"panes":[{"pid":1001,"dead":false}],"tmuxContext":{"schemaVersion":1,"backend":"tmux","socketName":"default","binary":"/private/var/folders/31/tw6qcw8546s40nbg64vtgm4c0000gn/T/quantnight-native-89433-1/native-tmux","sessionId":"$4","sessionCreatedAt":1700000000000,"taskId":"6ae2b469f9c53b4a1765ad9e","launchAttemptId":"old-launch","remote":true}},"controlStatus":"running","controlReconciled":false,"error":null},{"schemaVersion":1,"command":"start","task":"fixture","ok":true,"at":"2026-10-29T02:50:22.122841+00:00","taskId":"6ae2b46cf9c53b4a1765ada2","remote":false,"taskType":"super","authProfile":"user1","launchAttemptId":"old-launch","operationSettled":true,"outcome":"already_running","startedNew":false,"consumerRunning":true,"configApplied":false,"configComparison":"not_compared","requestSatisfied":true,"executionObservation":{"schemaVersion":1,"taskName":"fixture","remote":false,"state":"unverified","consumerRunning":true,"identityVerified":false,"ownedResidue":false,"configComparison":"not_requested","sessionId":"$4","sessionCreatedAt":1700000000000,"bindingBasis":"unverified","pid":1002,"pidCreatedAt":1700000010.0,"consumerRunId":null,"consumerPredatesLaunch":false,"reason":"independent_task_or_launch_binding_unverified","explicitPauseVerified":true,"taskId":"6ae2b46cf9c53b4a1765ada2","consumerScopeVerified":true,"consumerWorkdir":"/var/folders/31/tw6qcw8546s40nbg64vtgm4c0000gn/T/quantnight-native-89433-2","processes":[{"pid":1001,"pidCreatedAt":1700000000.0,"argvDigest":"2d4e4fd3a999b340a805afbc8740a13fad92c278a25e18e69917c2d17c6240f2"},{"pid":1002,"pidCreatedAt":1700000010.0,"argvDigest":"963fadd4ca7fdbf73dae7b69fae251bf691238d41a15ae288dc2f7aee131ae4a"}],"panes":[{"pid":1001,"dead":false}],"tmuxContext":{"schemaVersion":1,"backend":"tmux","socketName":"default","binary":"/private/var/folders/31/tw6qcw8546s40nbg64vtgm4c0000gn/T/quantnight-native-89433-2/native-tmux","sessionId":"$4","sessionCreatedAt":1700000000000,"taskId":"6ae2b46cf9c53b4a1765ada2","launchAttemptId":"old-launch","remote":false}},"controlStatus":"running","controlReconciled":false,"error":null},{"schemaVersion":1,"command":"start","task":"fixture","ok":true,"at":"2026-10-29T02:50:25.314501+00:00","taskId":"6ae2b46ff9c53b4a1765ada6","remote":true,"taskType":"super","authProfile":"user1","launchAttemptId":"old-launch","operationSettled":true,"outcome":"already_running","startedNew":false,"consumerRunning":true,"configApplied":false,"configComparison":"not_compared","requestSatisfied":true,"executionObservation":{"schemaVersion":1,"taskName":"fixture","remote":true,"state":"unverified","consumerRunning":true,"identityVerified":false,"ownedResidue":false,"configComparison":"not_requested","sessionId":"$4","sessionCreatedAt":1700000000000,"bindingBasis":"unverified","pid":1002,"pidCreatedAt":1700000010.0,"consumerRunId":null,"consumerPredatesLaunch":false,"reason":"independent_task_or_launch_binding_unverified","explicitPauseVerified":true,"taskId":"6ae2b46ff9c53b4a1765ada6","consumerScopeVerified":true,"consumerWorkdir":"/var/folders/31/tw6qcw8546s40nbg64vtgm4c0000gn/T/quantnight-native-89433-3","processes":[{"pid":1001,"pidCreatedAt":1700000000.0,"argvDigest":"2d4e4fd3a999b340a805afbc8740a13fad92c278a25e18e69917c2d17c6240f2"},{"pid":1002,"pidCreatedAt":1700000010.0,"argvDigest":"18d9d3c8a4fdcd7fc91612952feb6729efbeb3939ea016d6abc0af73e4a4623f"}],"panes":[{"pid":1001,"dead":false}],"tmuxContext":{"schemaVersion":1,"backend":"tmux","socketName":"default","binary":"/private/var/folders/31/tw6qcw8546s40nbg64vtgm4c0000gn/T/quantnight-native-89433-3/native-tmux","sessionId":"$4","sessionCreatedAt":1700000000000,"taskId":"6ae2b46ff9c53b4a1765ada6","launchAttemptId":"old-launch","remote":true}},"controlStatus":"running","controlReconciled":false,"error":null}];
// END R1 NATIVE SCOPE WIRE
async function r1ScopeResponses() {
  if (!process.env.QUANTNIGHT_TEST_START_WIRE) return r1NativeScopeWire;
  return Promise.all(r1NativeScopeWire.map(wire => readFile(
    `${process.env.QUANTNIGHT_TEST_START_WIRE}/${wire.taskType}-${wire.remote}.json`, 'utf8').then(JSON.parse)));
}
const r1ScopeInitial = wire => ({ _id: { $oid: wire.taskId }, name: wire.task,
  taskType: wire.taskType, auth_profile: wire.authProfile, isRemote: wire.remote,
  targetRemote: wire.remote, status: 'launch_unknown', launchAttemptId: wire.launchAttemptId,
  launchSettledAttemptId: null });
const r1ScopeReceipt = wire => receipt({taskId:wire.taskId,taskName:wire.task,isRemote:wire.remote,
  confirmationBasis:'runners_exited'});

test('R1 actual native scope response completes this START and joins one queued original pause', async () => {
  for (const wire of await r1ScopeResponses()) {
    assert.equal(wire.executionObservation.identityVerified, false);
    assert.equal(wire.executionObservation.consumerScopeVerified, true);
    assert.equal(wire.configApplied, false); assert.equal(wire.startedNew, false);
    const h = await ownStartHarness(r1ScopeInitial(wire));
    const latest = {...h.latest, launchAttemptId:wire.launchAttemptId,launchSettledAttemptId:null};
    try {
      const pausing=h.state.pauseTask(h.store.tasks[0]);
      assert.equal(h.state.pauseTask(h.store.tasks[0]),pausing,'duplicate intent must share original promise');
      assert.equal(h.ownCalls.filter(c=>c.name==='pause_task').length,0,'not before current START completes');
      h.native.resolve(wire); await h.starting; await tick();
      h.fresh.resolve([latest]); await tick(); await tick();
      const pauses=h.ownCalls.filter(c=>c.name==='pause_task');
      assert.equal(pauses.length,1,`${wire.taskType}/${wire.remote}: finished ensure-running must not cancel requested pause`);
      assert.equal(pauses[0].args.taskId,wire.taskId);
      assert.equal(pauses[0].args.taskName,wire.task);
      assert.equal(pauses[0].args.isRemote,wire.remote);
      assert.equal(pauses[0].args.expectedLaunchAttemptId,wire.launchAttemptId);
      assert.equal(h.store.tasks[0].launchSettledAttemptId,null,'no fabricated historical launch settlement before stop proof');
      h.stopReply.resolve(r1ScopeReceipt(wire));
      assert.equal(await pausing,true); assert.equal(h.store.tasks[0].status,'paused');
      assert.equal(h.ownCalls.filter(c=>c.name.startsWith('start_')).length,1);
      assert.equal(h.ownCalls.filter(c=>c.name==='pause_task').length,1);
      assert.equal(h.hooks.timers.size,0);
    } finally {h.native.resolve(wire);h.fresh.resolve([latest]);h.stopReply.resolve(r1ScopeReceipt(wire));h.stop();}
  }
});

test('R1 actual scope response cannot revive cancelled pause or adopt fresh successor launch', async () => {
  for (const wire of await r1ScopeResponses()) for (const cause of ['unmount','successor']) {
    const h=await ownStartHarness(r1ScopeInitial(wire));
    const latest={...h.latest,launchAttemptId:wire.launchAttemptId,launchSettledAttemptId:null};
    try {
      const pausing=h.state.pauseTask(h.store.tasks[0]);
      if(cause==='unmount') h.stop();
      h.native.resolve(wire);await h.starting;await tick();
      h.fresh.resolve([{...latest,...(cause==='successor'?{launchAttemptId:'foreign-successor'}:{})}]);await tick();
      assert.equal(await pausing,false);
      assert.equal(h.ownCalls.filter(c=>c.name==='pause_task').length,0);
      assert.equal(h.ownCalls.filter(c=>c.name.startsWith('start_')).length,1);
    } finally {h.native.resolve(wire);h.fresh.resolve([latest]);h.stopReply.resolve(r1ScopeReceipt(wire));h.stop();}
  }
});

// R01: item success precedes result collection; the GUI is not a completion owner.
// These execute the production SFC + Pinia listeners; only native I/O and time are fake.
for (const taskType of ['regular', 'super', 'priority']) {
  for (const isRemote of [false, true]) {
    test(`R01 full success with collection outstanding never pauses: ${taskType}/${isRemote}`, async () => {
      const initial = task({ taskType, isRemote, mission_config: {},
        simulatorRuntime: { lifecycleClosure: { phase: 'collecting' } } });
      const h = await harness(initial);
      try {
        const calls = [];
        let rows = [initial];
        h.hooks.invoke = async (name, args) => {
          calls.push({ name, args });
          if (name === 'get_all_tasks') return rows;
          if (name === 'frontend_ready' || name === 'refresh_task_progress') return;
          // Baseline may reach this: record the real stop command, do not mask it.
          if (name === 'pause_task') return receipt({ isRemote });
          assert.fail(`unexpected automatic operation: ${name}`);
        };
        await h.hooks.mount(); await tick();
        const counts = { success: 10, error: 0, total: 10,
          priority_success: 3, priority_error: 0, priority_total: 3 };
        h.hooks.handlers.get('task-progress-update')({ payload: {
          collection: initial.name, task_id: 'id-a', is_remote: isRemote,
          updated_at: 100, ...counts,
        } });
        await tick();
        for (const [key, value] of Object.entries(counts)) assert.equal(h.store.taskProgressMap[initial.name][key], value);
        assert.equal(h.store.tasks[0].status, 'running');
        await advanceJoinTime(h, 300000); await tick();
        await advanceJoinTime(h, 3600000); await tick();
        assert.equal(calls.filter(c => c.name === 'pause_task').length, 0,
          'full item success must not stop a consumer still collecting results');
        assert.equal(h.hooks.timers.size, 0, 'no count-completion timer registration');
        assert.equal(h.hooks.intervals.size, 0, 'no replacement polling');
        assert.deepEqual(calls.map(c => c.name), ['frontend_ready', 'get_all_tasks', 'refresh_task_progress']);
        assert.deepEqual(h.store.taskStopOperationMap, {});
        assert.equal(h.store.tasks[0].status, 'running');
        // Only the existing registry snapshot supplies a later completed state.
        rows = [{ ...initial, status: 'completed', completedAt: '2026-10-30T00:00:00Z' }];
        h.hooks.handlers.get('task-registry-changed')({ payload: {} }); await tick();
        assert.equal(h.store.tasks[0].status, 'completed');
        assert.equal(h.store.taskProgressMap[initial.name].success, 10);
        await advanceJoinTime(h, 3600000); await tick();
        assert.deepEqual(calls.map(c => c.name), ['frontend_ready', 'get_all_tasks', 'refresh_task_progress',
          'get_all_tasks', 'refresh_task_progress']);
      } finally { h.stop(); }
    });
  }
}

test('R01 complete progress registers no automatic timer or callback in the production SFC', async () => {
  const h = await harness(regularTask());
  try {
    h.store.updateProgress({ collection: 'priority-a', task_id: 'id-a', is_remote: true,
      updated_at: 100, success: 10, error: 0, total: 10 });
    await tick();
    assert.equal(h.hooks.timers.size, 0);
    assert.equal(h.hooks.intervals.size, 0);
    assert.equal(h.state.autoPause, undefined);
    assert.equal(h.state.stopAutoPauseWatch, undefined);
    assert.doesNotMatch(source, /createAutoPauseScheduler|autoPause\.reconcile/);
  } finally { h.stop(); }
});

test('R01 full counts still allow explicit pause through the original task/host-fenced core request', async () => {
  for (const taskType of ['regular', 'super', 'priority']) {
    for (const isRemote of [false, true]) {
      const initial = task({ taskType, isRemote, mission_config: {} });
      const h = await harness(initial);
      try {
        h.store.updateProgress({ collection: initial.name, task_id: 'id-a', is_remote: isRemote,
          updated_at: 100, success: 10, error: 0, total: 10 });
        await tick();
        await advanceJoinTime(h, 3600000);
        assert.equal(h.calls.length, 0);
        const progress = h.store.taskProgressMap[initial.name];
        const pausing = h.state.pauseTask(h.store.tasks[0]);
        const operation = h.store.taskStopOperationMap[initial.name];
        assert.deepEqual(h.calls, [{ name: 'pause_task', status: 'running', args: {
          taskName: initial.name, taskId: 'id-a', isRemote, requestId: operation.requestId,
        } }]);
        assert.equal(operation.status, 'stopping');
        h.command.resolve(receipt({ isRemote })); await tick();
        assert.equal(h.store.tasks[0].status, 'paused');
        h.lists[0].resolve([{ ...h.store.tasks[0] }]);
        await pausing;
        assert.equal(h.store.taskProgressMap[initial.name], progress);
        await advanceJoinTime(h, 3600000);
        assert.deepEqual(h.calls.map(call => call.name), ['pause_task', 'get_all_tasks', 'refresh_task_progress']);
        assert.equal(h.hooks.timers.size, 0);
      } finally { h.stop(); }
    }
  }
});
