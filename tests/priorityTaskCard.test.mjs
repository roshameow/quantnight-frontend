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
  assert.match(html, /Roots — ·\s*R0 — \/\s*R1 — \/\s*R2 — \/\s*R3 —/);
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
  assert.match(html, /R0 0 \/\s*R1 1/);
  assert.match(html, /R2 2/);
  assert.match(html, /R3 1/);
  assert.match(html, /regular-a, regular-b/);
  assert.match(html, /待slots 1/);
  assert.match(html, /无池全局预算/);
  assert.match(html, /未完成 190（含等待与未确认结果，不计为失败）/);
  const { main, details } = sections(html);
  assert.match(main, /Σ 240|Roots 4|regular-a, regular-b/);
  assert.doesNotMatch(main, /待slots|无池全局预算|未完成|UNKNOWN/);
  assert.match(details, /待slots 1|无池全局预算/);
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


// Native details retain their DOM content in SSR, while only summary is visible.
function sections(html) {
  const match = html.match(/<details\b([^>]*)>([\s\S]*?)<\/details>/);
  assert.ok(match, 'real card exposes an accessible details disclosure');
  assert.doesNotMatch(match[1], /\bopen(?:\s|=|$)/, 'details are closed by default');
  assert.match(match[2], /<summary>详情 \/ Details<\/summary>/);
  return { main: html.replace(match[0], ''), details: match[2] };
}

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

test('pending/processing totals are not UNKNOWN and all-zero secondary counters stay in details', async () => {
  for (const status of ['waiting', 'running']) {
    const saved = poolEvent({ status, reason: status === 'waiting' ? 'awaiting_consumer' : null,
      stage_distribution: { '0': 0, '1': 4, '2': 0, '3': 0 }, waiting_feedback: 0,
      waiting_slots: 0, completed: 0, blocked: 0, unresolved: 60,
      counts: { total: 60, success: 0, error: 0, priority_total: 60, priority_success: 0, priority_error: 0 } });
    const html = await render({ task, progress: { stale: false, priority_campaign: saved } });
    const { main, details } = sections(html);
    for (const value of [/✓ 0/, /✗ 0/, /Σ 60/, /Roots 4/, /R0 0 \/\s*R1 4/, /R2 0/, /R3 0/, /regular-a, regular-b/]) {
      assert.match(main, value);
    }
    assert.doesNotMatch(main, /待规划|待反馈|待slots|完成roots|阻塞roots|无池全局预算|冻结配置|source_remote|未完成|UNKNOWN|不是失败|awaiting_consumer/);
    for (const value of [/待规划 0/, /待反馈 0/, /待slots 0/, /完成roots 0/, /阻塞roots 0/,
      /每 root ≤3阶段/, /≤60\/阶段/, /≤180累计/, /无池全局预算/, /冻结配置/,
      /source_remote/, /未完成 60/]) assert.match(details, value);
    assert.doesNotMatch(html, /未完成\/UNKNOWN|不是失败/);
    const { state } = await setupCard({ task, progress: { stale: false, priority_campaign: saved } });
    assert.deepEqual(state.displayProgress.value, saved.counts); // all six raw counters survive
    assert.equal(state.runtime.value.unresolved, 60);
    assert.equal(state.runtimeWarning.value, null);
  }
});

test('explicit uncertainty/blockers and actual failures remain prominent, not fabricated from unresolved', async () => {
  for (const extra of [
    { status: 'UNKNOWN', reason: null },
    { status: 'waiting', reason: 'campaign_outcome_unknown' },
    { status: 'waiting', reason: null, blocked: 2 },
    { status: 'blocked', reason: 'feedback_incomplete_or_wait_timeout' },
    { status: 'waiting', reason: 'source_consumer_upgrade_required' },
    { status: 'error', reason: 'consumer_failed' },
  ]) {
    const saved = poolEvent({ blocked: 0, counts: { total: 60, success: 5, error: 2 }, unresolved: 53, ...extra });
    const html = await render({ task, progress: { stale: false, priority_campaign: saved } });
    const { main } = sections(html);
    assert.match(main, /✓ 5/);
    assert.match(main, /✗ 2/);
    assert.match(main, /Σ 60/);
    assert.match(main, extra.reason ? new RegExp(extra.reason) : extra.status === 'UNKNOWN'
      ? /状态 UNKNOWN：请更新确认实际结果/ : /阻塞roots 2：请检查来源反馈并更新状态/);
    if (extra.reason === 'source_consumer_upgrade_required') assert.match(main, /勿启 standalone Consumer 抢槽/);
    assert.doesNotMatch(main, /未完成\/UNKNOWN|53（不是失败）/);
  }
});

test('stale/missing/fallback metadata and legacy read-only warning stay outside details', async () => {
  const saved = poolEvent();
  const cases = [
    [null, /进度未确认/],
    [{ ...saved, stale: true, last_error: 'registry timeout' }, /registry timeout/],
    [{ ...saved, counts: null, stats_confirmed: false, stale: true, reason: 'root_pool_not_registered',
      last_error: 'selected host registry missing' }, /selected host registry missing/],
    [{ ...saved, data_source: 'own_task_fallback' }, /仅旧本任务集合（账本未确认）/],
    [event({ mode: 'legacy' }), /旧 campaign（只读）/],
  ];
  for (const [payload, warning] of cases) {
    const html = await render({ task, progress: { stale: false, priority_campaign: payload } });
    const { main } = sections(html);
    assert.match(main, warning);
    if (payload?.last_error === 'registry timeout') {
      assert.match(main, /Σ 240/);
      assert.match(main, /保留上次实际统计/);
    }
  }
});

test('dynamic source fallback is compact; operation event bindings remain unchanged', async () => {
  const saved = poolEvent({ source_tasks: [] });
  const { main, details } = sections(await render({ task,
    progress: { stale: false, priority_campaign: saved } }));
  assert.match(main, /来源任务：动态 active regular/);
  assert.doesNotMatch(main, /所选Priority主机control|source_remote/);
  assert.match(details, /所选Priority主机control同 auth/);
  for (const operation of ['start', 'update', 'pause', 'delete']) {
    assert.ok(source.includes(`@click="$emit('${operation}')"`));
  }
  assert.ok(source.includes(`@update:value="(val) => $emit('update:isRemote', val)"`));
});

test('routine explanations never suppress an actual blocker, UNKNOWN state or unfamiliar reason', () => {
  assert.equal(card.priorityRuntimeWarning(poolEvent({ status: 'waiting', reason: 'no_eligible_source_results', blocked: 0 })), null);
  for (const status of ['blocked', 'error', 'failed', 'unknown']) {
    assert.equal(card.priorityRuntimeWarning(poolEvent({ status, reason: 'awaiting_consumer' })), 'awaiting_consumer');
  }
  assert.equal(card.priorityRuntimeWarning(poolEvent({ status: 'waiting', reason: 'new_unrecognized_problem' })), 'new_unrecognized_problem');
});

test('R0 comes from stage zero immediately before R1/R2/R3 with no duplicate planning main text', async () => {
  const saved = poolEvent({ root_count: 118, status: 'waiting', reason: null,
    stage_distribution: { '0': 29, '1': 89, '2': 0, '3': 0 } });
  const { main, details } = sections(await render({ task,
    progress: { stale: false, priority_campaign: saved } }));
  assert.match(main, /Roots 118 ·\s*R0 29 \/\s*R1 89 \/\s*R2 0 \/\s*R3 0/);
  assert.match(main, /Σ 240/);
  assert.match(main, /✓ 50/);
  assert.match(main, /regular-a, regular-b/);
  assert.doesNotMatch(main, /待规划|阶段分布|未完成/);
  assert.match(details, /待规划 29/);
});
