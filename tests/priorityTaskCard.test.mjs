import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { pathToFileURL } from 'node:url';
import { parse, compileScript } from '@vue/compiler-sfc';
import { createSSRApp, h } from 'vue';
import { renderToString } from '@vue/server-renderer';

const source = await readFile(new URL('../src/components/TaskCards/PriorityTaskCard.vue', import.meta.url), 'utf8');
const { descriptor } = parse(source);
const require = createRequire(import.meta.url);
const vueModule = pathToFileURL(require.resolve('vue/dist/vue.runtime.esm-bundler.js')).href;
// Compile the real SFC, not a copied helper or substitute card. Stub only native
// APIs; no Tauri service/database access and no browser are needed for SSR.
const compiled = compileScript(descriptor, { id: 'priority-test', inlineTemplate: true });
const moduleCode = compiled.content
  .replaceAll('from "vue"', `from ${JSON.stringify(vueModule)}`)
  .replaceAll("from 'vue'", `from ${JSON.stringify(vueModule)}`)
  .replace(/import \{ listen \} from '@tauri-apps\/api\/event';/, 'const listen = async () => () => {};')
  .replace(/import \{ invoke \} from '@tauri-apps\/api\/core';/, 'const invoke = async () => {};');
const card = await import(`data:text/javascript;base64,${Buffer.from(moduleCode).toString('base64')}`);
const { acceptPriorityRuntime, mergePriorityRuntime, priorityStatusType } = card;
const task = { _id: 'id-a', name: 'pipeline__priority', isRemote: true, status: 'error',
  mission_config: { rounds: { enabled: true, max_rounds: 3 } } };
const event = (extra = {}) => ({ collection: task.name, task_id: 'id-a', is_remote: true, updated_at: 100,
  campaign_id: 'campaign-a', status: 'blocked', reason: 'feedback_incomplete_or_wait_timeout',
  current_round: 1, max_rounds: 3, max_per_round: 60, stats_confirmed: true, stale: false,
  counts: { total: 60, success: 5, error: 2 }, unresolved: 53, ...extra });

test('metadata accepts only current identity, source and nondecreasing version', () => {
  assert.equal(acceptPriorityRuntime(event(), task, null), true);
  assert.equal(acceptPriorityRuntime(event(), { ...task, _id: { $oid: 'id-a' } }, null), true);
  for (const override of [{ task_id: 'old-id' }, { is_remote: false }, { collection: 'other' },
    { updated_at: -1 }, { updated_at: undefined }, { updated_at: 0.5 }]) {
    assert.equal(acceptPriorityRuntime(event(override), task, null), false);
  }
  assert.equal(acceptPriorityRuntime(event({ updated_at: 99 }), task, event()), false);
  assert.equal(acceptPriorityRuntime(event(), task, event()), true); // stage and final counts share version
});

test('failed refresh keeps actual counts, never substitutes summary reserved', () => {
  const previous = mergePriorityRuntime(event(), null);
  const failed = mergePriorityRuntime(event({ stats_confirmed: false, counts: null, reserved: 180,
    stale: true, last_error: 'ledger read timeout' }), previous);
  assert.deepEqual(failed.counts, { total: 60, success: 5, error: 2 });
  assert.equal(failed.stale, true);
  assert.equal(failed.last_error, 'ledger read timeout');
  assert.equal(mergePriorityRuntime(event({ stats_confirmed: false, counts: null, reserved: 60 }), null).counts, null);
});

test('new campaigns/tasks/sources cannot inherit old actual counts', () => {
  for (const override of [{ campaign_id: 'campaign-b' }, { task_id: 'id-b' }, { is_remote: false }]) {
    const merged = mergePriorityRuntime(event({ ...override, stats_confirmed: false, counts: null }), event());
    assert.equal(merged.counts, null);
  }
});

test('blocked/waiting are warning rather than error or completion', () => {
  assert.equal(priorityStatusType('blocked'), 'warning');
  assert.equal(priorityStatusType('waiting'), 'warning');
  assert.equal(priorityStatusType('UNKNOWN'), 'default');
  assert.equal(priorityStatusType('error'), 'error');
  assert.equal(priorityStatusType('completed'), 'success');
});

async function render(props) {
  const app = createSSRApp({ render: () => h(card.default, props) });
  app.config.warnHandler = () => {};
  for (const name of ['n-card', 'n-tag', 'n-text', 'n-space', 'n-switch', 'n-button']) {
    app.component(name, { setup: (_, { slots }) => () => h('div', [slots.header?.(), slots.default?.()]) });
  }
  return renderToString(app);
}

test('real card initially renders unconfirmed values, not 0/0/0 or tmux error', async () => {
  const html = await render({ task, taskRuntimeStatus: 'error',
    progress: { success: 0, error: 0, total: 0, stale: true } });
  assert.match(html, /进度未确认/);
  assert.match(html, /等待root池状态确认/);
  assert.match(html, /阶段分布/);
  assert.doesNotMatch(html, /Round.*\/.*3/);
  assert.doesNotMatch(html, /Σ 0|✗ 0|✓ 0|任务状态：error/);
});

test('legacy card preserves real numeric statistics without opting into rounds', async () => {
  const html = await render({ task: { ...task, status: 'running', mission_config: {} },
    taskRuntimeStatus: 'running', progress: { success: 5, error: 2, total: 60, stale: false } });
  assert.match(html, /✓ 5/);
  assert.match(html, /✗ 2/);
  assert.match(html, /Σ 60/);
});

// Run the real compiled setup with only lifecycle/native APIs replaced. This
// exercises event-to-computed rendering state and listener cleanup without DOM.
let instanceNumber = 0;
async function setupCard(initial) {
  const hooksCode = `export const hooks = { cleanup: 0, refresh: 0, mount: null, unmount: null, handler: null };
    const onMounted = (fn) => { hooks.mount = fn; };
    const onUnmounted = (fn) => { hooks.unmount = fn; };
    const listen = async (name, handler) => {
      hooks.eventName = name; hooks.handler = handler; await hooks.gate;
      return () => { hooks.cleanup++; };
    };
    const invoke = async (name) => { hooks.refresh++; hooks.command = name; };
  `;
  const code = compileScript(descriptor, { id: `priority-instance-${instanceNumber++}` }).content
    .replace('computed, onMounted, onUnmounted, ref, watch', 'computed, ref, watch')
    .replaceAll("from 'vue'", `from ${JSON.stringify(vueModule)}`)
    .replaceAll('from "vue"', `from ${JSON.stringify(vueModule)}`)
    .replace(/import \{ listen \} from '@tauri-apps\/api\/event';/, '')
    .replace(/import \{ invoke \} from '@tauri-apps\/api\/core';/, '');
  const module = await import(`data:text/javascript;base64,${Buffer.from(hooksCode + code + `\n// ${instanceNumber}`).toString('base64')}`);
  const { reactive, nextTick } = await import('vue');
  const props = reactive(initial);
  const state = module.default.setup(props, { expose() {}, emit() {} });
  return { hooks: module.hooks, state, props, nextTick };
}

test('actual card event handler displays blocked reason/round/60 real entries despite control/tmux error', async () => {
  const { hooks, state } = await setupCard({ task, taskRuntimeStatus: 'error',
    progress: { success: 0, error: 0, total: 0, stale: true } });
  await hooks.mount();
  assert.equal(hooks.eventName, 'task-priority-runtime-update');
  assert.equal(hooks.command, 'refresh_task_progress');
  hooks.handler({ payload: event() });
  assert.equal(state.displayStatus.value, 'blocked');
  assert.equal(state.displayRuntimeStatus.value, 'blocked');
  assert.equal(state.runtime.value.reason, 'feedback_incomplete_or_wait_timeout');
  assert.equal(state.runtime.value.current_round, 1);
  assert.equal(state.runtime.value.max_rounds, 3);
  assert.equal(state.runtime.value.unresolved, 53);
  assert.deepEqual(state.displayProgress.value, { total: 60, success: 5, error: 2 });
  hooks.handler({ payload: event({ updated_at: 101, campaign_id: null, stage: null, status: null,
    stats_confirmed: false, counts: null, stale: true, last_error: 'registry timeout' }) });
  assert.equal(state.displayStatus.value, 'blocked');
  assert.equal(state.displayProgress.value.total, 60);
  assert.equal(state.progressStale.value, true);
  assert.equal(state.progressError.value, 'registry timeout');
  hooks.unmount();
  assert.equal(hooks.cleanup, 1);
});

test('card source changes clear old data and retain global progress ordering watermark', async () => {
  const { hooks, state, props, nextTick } = await setupCard({ task: { ...task }, taskRuntimeStatus: 'error',
    progress: { stale: false, updated_at: 100 } });
  await hooks.mount();
  hooks.handler({ payload: event() });
  props.task.isRemote = false;
  await nextTick();
  assert.equal(state.runtime.value, null);
  hooks.handler({ payload: event({ is_remote: false, updated_at: 99 }) });
  assert.equal(state.runtime.value, null);
  hooks.handler({ payload: event({ is_remote: false, updated_at: 101 }) });
  assert.equal(state.displayProgress.value.total, 60);
  hooks.unmount();
  hooks.handler({ payload: event({ is_remote: false, updated_at: 102, status: 'completed' }) });
  assert.equal(state.displayStatus.value, 'blocked');
});

test('late listener setup after unmount is cleaned without requesting another refresh', async () => {
  const { hooks } = await setupCard({ task, taskRuntimeStatus: 'error', progress: { stale: true } });
  let release;
  hooks.gate = new Promise((resolve) => { release = resolve; });
  const mount = hooks.mount();
  hooks.unmount();
  release();
  await mount;
  assert.equal(hooks.cleanup, 1);
  assert.equal(hooks.refresh, 0);
});

test('UNKNOWN/missing actual rows cannot display summary completed as completion', () => {
  assert.equal(card.priorityDisplayStatus(event({ status: 'completed', unresolved: 1 }), true, 'error'), 'waiting');
  assert.equal(card.priorityDisplayStatus(event({ status: 'completed', counts: null, unresolved: null }), true, 'error'), 'waiting');
  assert.equal(card.priorityDisplayStatus(event({ status: 'completed', unresolved: 0 }), true, 'error'), 'completed');
});

test('remounted card immediately displays cached campaign summary and ledger actual counts', async () => {
  const saved = event({ max_per_round: 60 });
  const { state } = await setupCard({ task, taskRuntimeStatus: 'error', progress: {
    success: 5, error: 2, total: 60, stale: false, updated_at: saved.updated_at,
    priority_campaign: saved,
  } });
  assert.equal(state.displayStatus.value, 'blocked');
  assert.equal(state.runtime.value.current_round, 1);
  assert.equal(state.runtime.value.max_rounds, 3);
  assert.equal(state.runtime.value.max_per_round, 60);
  assert.equal(state.displayProgress.value.total, 60);
});

test('all 60 pending reserved entries display 0 success/0 failures/60 planned, waiting not runtime error', async () => {
  const saved = event({ status: 'waiting', reason: 'awaiting_consumer', unresolved: 60,
    counts: { total: 60, success: 0, error: 0 } });
  const { state } = await setupCard({ task, taskRuntimeStatus: 'error', progress: {
    success: 0, error: 0, total: 60, stale: false, updated_at: saved.updated_at,
    priority_campaign: saved,
  } });
  assert.equal(state.displayRuntimeStatus.value, 'waiting');
  assert.equal(state.runtime.value.reason, 'awaiting_consumer');
  assert.deepEqual(state.displayProgress.value, { total: 60, success: 0, error: 0 });
  assert.equal(state.runtime.value.unresolved, 60);
});


const poolEvent = (extra = {}) => event({ mode: 'per_root', campaign_id: null, pool_id: 'pool-a',
  root_count: 4, stage_distribution: { '1': 1, '2': 2, '3': 1 }, waiting_feedback: 2,
  waiting_slots: 1, completed: 1, blocked: 0, source_tasks: ['regular-a', 'regular-b'],
  counts: { total: 240, success: 50, error: 0 }, unresolved: 190, ...extra });

test('root pool SSR shows independent distribution, origins and more than 180 entries without task round', async () => {
  const saved = poolEvent();
  const html = await render({ task, progress: { stale: false, updated_at: saved.updated_at, priority_campaign: saved } });
  assert.match(html, /Σ 240/);
  assert.match(html, /Roots 4/);
  assert.match(html, /R1 1/);
  assert.match(html, /R2 2/);
  assert.match(html, /R3 1/);
  assert.match(html, /regular-a, regular-b/);
  assert.match(html, /待slots 1/);
  assert.match(html, /无池全局预算/);
  assert.match(html, /190（不是失败）/);
  assert.doesNotMatch(html, /Round|Σ\s*240\s*\/\s*180/);
});

test('pool identity isolates counts; metadata timeout keeps root distribution visibly stale', () => {
  const previous = poolEvent();
  const incoming = poolEvent({ pool_id: 'pool-b', stats_confirmed: false, counts: null });
  assert.equal(mergePriorityRuntime(incoming, previous).counts, null);
  const failed = mergePriorityRuntime(poolEvent({ pool_id: null, stats_confirmed: false, counts: null,
    stage_distribution: null, root_count: null, source_tasks: [], last_error: 'registry timeout', stale: true }), previous);
  assert.equal(failed.pool_id, 'pool-a');
  assert.deepEqual(failed.stage_distribution, previous.stage_distribution);
  assert.equal(failed.counts.total, 240);
  assert.equal(failed.stale, true);
});

test('legacy campaign has read-only warning and no whole-task round UI', async () => {
  const saved = event({ mode: 'legacy' });
  const html = await render({ task, progress: { stale: false, updated_at: saved.updated_at, priority_campaign: saved } });
  assert.match(html, /旧 campaign（只读）/);
  assert.doesNotMatch(html, /Round|R1 1|Roots/);
});


test('per-root switch remains selectable and reflects Priority host, not source execution', async () => {
  for (const isRemote of [true, false]) {
    const saved = poolEvent({ is_remote: isRemote });
    const html = await render({ task: { ...task, isRemote },
      progress: { stale: false, updated_at: saved.updated_at, priority_campaign: saved } });
    assert.match(html, new RegExp(`value="${isRemote}"`));
    assert.doesNotMatch(html, /disabled|本地controller/);
    assert.match(html, /来源执行独立按source_remote，可local\/remote/);
    assert.match(html, /regular-a, regular-b/);
  }
});

test('selected-host missing root registry cannot revive an old global campaign or counts', () => {
  for (const is_remote of [true, false]) {
    const previous = event({ is_remote, mode: 'legacy', status: 'completed', unresolved: 0 });
    const missing = event({ is_remote, campaign_id: null, pool_id: null, mode: 'per_root',
      reason: 'root_pool_not_registered', status: null, current_round: null,
      stats_confirmed: false, counts: null, stale: true, last_error: 'selected host registry missing' });
    const merged = mergePriorityRuntime(missing, previous);
    assert.equal(merged.campaign_id, null);
    assert.equal(merged.mode, 'per_root');
    assert.equal(merged.reason, 'root_pool_not_registered');
    assert.equal(merged.counts, null);
    assert.equal(card.priorityDisplayStatus(merged, true, 'error'), 'waiting');
    const oldPool = poolEvent({ is_remote });
    assert.equal(mergePriorityRuntime(missing, oldPool).pool_id, null);
    assert.equal(mergePriorityRuntime(missing, oldPool).counts, null);
  }
});
