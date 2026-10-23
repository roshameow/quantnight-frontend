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
    .replaceAll("'../utils/taskDiagnostics.js'", JSON.stringify(new URL('../src/utils/taskDiagnostics.js', import.meta.url).href))
    .replace("'../utils/taskGeneration.js'", JSON.stringify(new URL('../src/utils/taskGeneration.js', import.meta.url).href))
    .replace(/import \{ invoke \} from "@tauri-apps\/api\/core";/, 'const invoke = (...args) => hooks.invoke(...args);')
    .replace(/import \{ listen \} from "@tauri-apps\/api\/event";/, 'const listen = (...args) => hooks.listen(...args);')
    .replace(/import \{ useRoute, useRouter \} from "vue-router";/,
      'const useRoute = () => ({ fullPath: "/tasks" }); const useRouter = () => ({ push() {} });')
    .replace(/import \{ useMessage, useDialog \} from "naive-ui";/,
      'const useMessage = () => ({ error: (...args) => hooks.errors.push(args), success() {} }); const useDialog = () => ({});')
    .replace(/import (\w+) from "[^"\n]+\.vue";/g, 'const $1 = {};');
  const hooksCode = `export const hooks = { errors: [], invoke: null, unmount: null, timers: new Map(), nextTimer: 0 };
    const setTimeout = (callback, delay) => { const id = ++hooks.nextTimer; hooks.timers.set(id, { callback, delay }); return id; };
    const clearTimeout = (id) => { hooks.timers.delete(id); };
    const createAutoPauseScheduler = (options) => realAutoPauseScheduler({ ...options, setTimeoutFn: setTimeout, clearTimeoutFn: clearTimeout });
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

test('existing manager poll includes regular, super and priority running tasks, never ready/paused; no DB mutation', async () => {
  const h = await harness(regularTask());
  try {
    h.store.replaceTasks([regularTask(), task({ _id: 'id-p', name: 'priority-p' }),
      regularTask({ _id: 'id-s', name: 'super-s', taskType: 'super' }),
      regularTask({ _id: 'id-r', name: 'ready-r', status: 'ready' }),
      regularTask({ _id: 'id-z', name: 'paused-z', status: 'paused' })]);
    const before = JSON.stringify(h.store.tasks);
    h.state.pollTaskStatus();
    h.state.pollTaskStatus(); // one in-flight request per target
    assert.deepEqual(h.calls.map(call => call.args.taskName), ['priority-a', 'priority-p', 'super-s']);
    assert.ok(h.calls.every(call => call.name === 'check_task_status'));
    assert.equal(h.hooks.timers.size, 3);
    h.probes.forEach(probe => probe.resolve(bashPane));
    await tick();
    assert.equal(h.hooks.timers.size, 0);
    const observation = h.state.taskRuntimeStatus.value['priority-a'];
    assert.equal(observation.taskId, 'id-a');
    assert.equal(observation.isRemote, true);
    assert.equal(observation.state, 'not_running');
    assert.ok(Number.isSafeInteger(observation.observedAt));
    assert.equal(JSON.stringify(h.store.tasks), before);
  } finally { h.stop(); }
});

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
