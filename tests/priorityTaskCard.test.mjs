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
const configModule = new URL('../src/utils/taskConfig.js', import.meta.url).href;
const stopModule = new URL('../src/stores/taskStore.js', import.meta.url).href;
const compiled = compileScript(descriptor, { id: 'priority-test', inlineTemplate: true });
const moduleCode = compiled.content
  .replaceAll("'../../utils/taskDiagnostics.js'", JSON.stringify(new URL('../src/utils/taskDiagnostics.js', import.meta.url).href))
  .replaceAll("'../../stores/taskStore.js'", JSON.stringify(stopModule))
  .replaceAll("'../../utils/taskConfig.js'", JSON.stringify(configModule))
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
  assert.match(html, /尚未取得 root 池状态：元数据检查尚未返回有效阶段或研究结果/);
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
    .replaceAll("'../../utils/taskDiagnostics.js'", JSON.stringify(new URL('../src/utils/taskDiagnostics.js', import.meta.url).href))
  .replaceAll("'../../stores/taskStore.js'", JSON.stringify(stopModule))
    .replaceAll("'../../utils/taskConfig.js'", JSON.stringify(configModule))
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
  waiting_slots: 1, completed: 1, blocked: 0, source_tasks: ['regular-a'],
  counts: { total: 240, success: 50, error: 0 }, unresolved: 190, ...extra });

test('root pool SSR shows independent distribution, origins and more than 180 entries without task round', async () => {
  const saved = poolEvent();
  const html = await render({ task, progress: { stale: false, updated_at: saved.updated_at, priority_campaign: saved } });
  assert.match(html, /Σ 240/);
  assert.match(html, /Roots 4/);
  assert.match(html, /R0 0 \/\s*R1 1/);
  assert.match(html, /R2 2/);
  assert.match(html, /R3 1/);
  assert.match(html, /regular-a/);
  assert.match(html, /待slots 1/);
  assert.match(html, /每 root ≤3阶段 · ≤60\/阶段 · ≤180累计/);
  assert.match(html, /待完成 190（含等待与待核验结果）/);
  const { main, details } = sections(html);
  assert.match(main, /Σ 240|Roots 4|regular-a/);
  assert.doesNotMatch(main, /待slots|无池全局预算|待完成|UNKNOWN/);
  assert.match(details, /待slots 1/);
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
  assert.match(html, /旧 campaign 记录（只读）/);
  assert.doesNotMatch(html, /Round|R1 1|Roots/);
});


// Native details retain their DOM content in SSR, while only summary is visible.
function sections(html) {
  const match = html.match(/<details\b([^>]*\bclass="runtime-details"[^>]*)>([\s\S]*?)<\/details>/);
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
    assert.match(html, /regular-a/);
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
    for (const value of [/✓ 0/, /✗ 0/, /Σ 60/, /Roots 4/, /R0 0 \/\s*R1 4/, /R2 0/, /R3 0/, /regular-a/]) {
      assert.match(main, value);
    }
    assert.doesNotMatch(main, /待规划|待反馈|待slots|完成roots|阻塞roots|无池全局预算|冻结配置|source_remote|待完成|UNKNOWN|不是失败|awaiting_consumer/);
    for (const value of [/待规划 0/, /待反馈 0/, /待slots 0/, /完成roots 0/, /阻塞roots 0/,
      /每 root ≤3阶段/, /≤60\/阶段/, /≤180累计/, /每 root ≤3阶段 · ≤60\/阶段 · ≤180累计/, /冻结配置/,
      /source_remote/, /待完成 60/]) assert.match(details, value);
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
      ? /研究元数据缺少有效结果及失败原因/ : /阻塞roots 2：请检查来源反馈并更新状态/);
    if (extra.reason === 'source_consumer_upgrade_required') assert.match(main, /等待来源任务 normal consumer 的 protocol3 支持/);
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
    [event({ mode: 'legacy' }), /旧 campaign 记录（只读）/],
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
  assert.match(main, /regular-a/);
  assert.doesNotMatch(main, /待规划|阶段分布|未完成/);
  assert.match(details, /待规划 29/);
});

const sourceSummaries = [
  { source_task: 'glb_spread', source_remote: false, root_count: 118,
    stage_distribution: { '0': 29, '1': 89, '2': 0, '3': 0 } },
  { source_task: 'asi_revision', source_remote: true, root_count: 26,
    stage_distribution: { '0': 7, '1': 3, '2': 8, '3': 8 } },
];
const groupedEvent = (extra = {}) => poolEvent({ status: 'active', reason: null, root_count: 144,
  stage_distribution: { '0': 36, '1': 92, '2': 8, '3': 8 },
  source_tasks: ['glb_spread', 'asi_revision'], source_summaries: sourceSummaries, ...extra });
const plainText = html => html.replace(/<[^>]*>/g, '').replace(/\s+/g, ' ').trim();

test('two real source groups show their own Roots and R0–R3, not duplicated global counts', async () => {
  const saved = groupedEvent();
  const { main, details } = sections(await render({ task, progress: { stale: false, priority_campaign: saved } }));
  const text = plainText(main);
  assert.match(text, /glb_spread · Roots 118 · R0 29 \/ R1 89 \/ R2 0 \/ R3 0/);
  assert.match(text, /asi_revision · Roots 26 · R0 7 \/ R1 3 \/ R2 8 \/ R3 8/);
  assert.equal((text.match(/Roots /g) || []).length, 2);
  assert.doesNotMatch(text, /Roots 144|R0 36|来源任务：|待规划|来源分组未确认/);
  assert.match(text, /Σ 240/);
  assert.match(text, /✓ 50/);
  assert.match(plainText(details), /总 Roots 144 · R0 36/);
  assert.match(plainText(details), /来源执行：glb_spread · local/);
  assert.match(plainText(details), /来源执行：asi_revision · remote/);
  assert.doesNotMatch(text, /来源执行：/);
  const groups = card.prioritySourceGroups(saved);
  assert.equal(groups[0].root_count, 118);
  assert.equal(groups[1].root_count, 26);
  assert.notEqual(groups[0].key, groups[1].key);
  assert.deepEqual(groups.map(group => group.source_remote), [false, true]);
});

test('single-source old metadata uses global distribution once without redundant origin/global lines', async () => {
  const saved = poolEvent({ source_tasks: ['glb_spread'], root_count: 118,
    stage_distribution: { '0': 29, '1': 89, '2': 0, '3': 0 } });
  const { main } = sections(await render({ task, progress: { stale: false, priority_campaign: saved } }));
  const text = plainText(main);
  assert.match(text, /glb_spread · Roots 118 · R0 29 \/ R1 89 \/ R2 0 \/ R3 0/);
  assert.equal((text.match(/glb_spread/g) || []).length, 1);
  assert.equal((text.match(/Roots /g) || []).length, 1);
  assert.doesNotMatch(text, /来源任务：|待规划/);
  assert.equal(card.prioritySourceGroups(saved)[0].source_remote, null); // never assume Priority host
});

test('single actual source summary overrides old global fallback and is never duplicated', async () => {
  const saved = groupedEvent({ source_tasks: ['glb_spread'], source_summaries: sourceSummaries.slice(0, 1),
    root_count: 118, stage_distribution: sourceSummaries[0].stage_distribution });
  const { main } = sections(await render({ task, progress: { stale: false, priority_campaign: saved } }));
  const text = plainText(main);
  assert.match(text, /glb_spread · Roots 118 · R0 29/);
  assert.equal((text.match(/glb_spread/g) || []).length, 1);
  assert.equal((text.match(/Roots /g) || []).length, 1);
  assert.doesNotMatch(text, /来源任务：|来源分组未确认/);
});

test('old multi-source metadata shows unknown group counts until updated, never assigns aggregate per source', async () => {
  for (const source_summaries of [undefined, null]) {
    const saved = groupedEvent({ source_summaries });
    const { main, details } = sections(await render({ task, progress: { stale: false, priority_campaign: saved } }));
    const text = plainText(main);
    for (const name of ['glb_spread', 'asi_revision']) {
      assert.match(text, new RegExp(`${name} · Roots — · R0 — / R1 — / R2 — / R3 —`));
    }
    assert.doesNotMatch(text, /Roots 144|Roots 118|Roots 26|R0 36|来源任务：/);
    assert.match(text, /来源分组未确认，请更新状态/);
    assert.match(plainText(details), /总 Roots 144 · R0 36/);
    assert.match(text, /Σ 240/); // actual item counts are still confirmed
  }
});

test('same source name local/remote groups remain distinct and are not rebound by Priority host switch', async () => {
  const sameName = sourceSummaries.map(group => ({ ...group, source_task: 'same-source' }));
  for (const isRemote of [false, true]) {
    const saved = groupedEvent({ is_remote: isRemote, source_tasks: ['same-source'], source_summaries: sameName });
    const { main } = sections(await render({ task: { ...task, isRemote },
      progress: { stale: false, priority_campaign: saved } }));
    const text = plainText(main);
    assert.match(text, /same-source（本地） · Roots 118 · R0 29/);
    assert.match(text, /same-source（远程） · Roots 26 · R0 7/);
    assert.deepEqual(card.prioritySourceGroups(saved).map(group => group.root_count), [118, 26]);
  }
});

test('active source without a group is visibly unconfirmed, not assigned another group or the aggregate', async () => {
  const saved = groupedEvent({ source_tasks: ['glb_spread', 'asi_revision', 'new-source'] });
  const { main } = sections(await render({ task, progress: { stale: false, priority_campaign: saved } }));
  const text = plainText(main);
  assert.match(text, /new-source · Roots — · R0 —/);
  assert.doesNotMatch(text, /new-source · Roots (144|118|26)/);
});

test('metadata transport failure preserves source groups visibly stale; missing/new pool does not inherit them', async () => {
  const { hooks, state } = await setupCard({ task, progress: { stale: false, priority_campaign: groupedEvent() } });
  await hooks.mount();
  hooks.handler({ payload: groupedEvent({ updated_at: 101, pool_id: null, source_summaries: null,
    source_tasks: [], root_count: null, stage_distribution: null, stale: true, last_error: 'registry timeout',
    stats_confirmed: false, counts: null }) });
  assert.deepEqual(state.runtime.value.source_summaries, sourceSummaries);
  assert.deepEqual(state.sourceGroups.value.map(group => group.root_count), [118, 26]);
  assert.equal(state.progressStale.value, true);
  assert.equal(state.progressError.value, 'registry timeout');
  for (const extra of [{ pool_id: 'pool-b' }, { pool_id: null, reason: 'root_pool_not_registered' }]) {
    const merged = mergePriorityRuntime(groupedEvent({ ...extra, source_summaries: null, stale: true,
      last_error: 'not confirmed', counts: null, stats_confirmed: false }), groupedEvent());
    assert.equal(merged.source_summaries, null);
    assert.equal(merged.counts, null);
    assert.ok(card.prioritySourceGroups(merged).every(group => group.root_count == null));
  }
  hooks.unmount();
});

test('confirmed empty source summaries mean no roots, not fabricated uncertainty on normal waiting', async () => {
  const saved = groupedEvent({ root_count: 0, source_summaries: [], status: 'waiting',
    stage_distribution: { '0': 0, '1': 0, '2': 0, '3': 0 }, reason: 'no_eligible_source_results',
    counts: { total: 0, success: 0, error: 0 }, unresolved: 0 });
  const { main } = sections(await render({ task, progress: { stale: false, priority_campaign: saved } }));
  const text = plainText(main);
  for (const name of ['glb_spread', 'asi_revision']) {
    assert.match(text, new RegExp(`${name} · Roots 0 · R0 0 / R1 0 / R2 0 / R3 0`));
  }
  assert.doesNotMatch(text, /来源分组未确认|UNKNOWN|待规划/);
});

test('card labels configured/default scheduling weight compactly with tooltip, not verbose main copy', async () => {
  const { PRIORITY_SCHEDULING_HINT } = await import('../src/utils/taskConfig.js');
  for (const status of ['pending', 'running', 'paused']) {
    for (const priority of [undefined, 1, 2, 7.5, 250]) {
      const config = { rounds: { enabled: true }, filter_params: { sharpe_th: 1.8 },
        ...(priority === undefined ? {} : { priority }) };
      const savedTask = { ...task, status, mission_config: config };
      const { main, details } = sections(await render({ task: savedTask,
        progress: { stale: false, priority_campaign: groupedEvent() } }));
      const text = plainText(main);
      assert.match(text, new RegExp(`调度优先级 ${priority ?? 1}(?![\\d.])`));
      assert.equal((text.match(/调度优先级/g) || []).length, 1);
      assert.ok(main.includes(`title="${PRIORITY_SCHEDULING_HINT}"`));
      assert.doesNotMatch(text, /Filter|Params|数值越大|先任务优先级|不抢占|严格全局|跨机器保证/);
      assert.match(plainText(details), /Filter Params:/);
      assert.match(text, /glb_spread · Roots 118 · R0 29 \/ R1 89 \/ R2 0 \/ R3 0/);
    }
  }
});

test('scheduling weight follows saved config precedence, never runtime/item priority markers', async () => {
  for (const key of ['mission_config', 'startConfig', 'config']) {
    const savedTask = { _id: 'id-a', name: task.name, status: 'running', isRemote: true,
      priority: 2, [key]: JSON.stringify({ priority: 9, rounds: { enabled: true } }) };
    const { main } = sections(await render({ task: savedTask,
      progress: { stale: false, priority_campaign: groupedEvent({ priority: 2 }) } }));
    assert.match(plainText(main), /调度优先级 9/);
    assert.doesNotMatch(plainText(main), /调度优先级 2/);
  }
  const html = await render({ task: { ...task, mission_config: undefined }, progress: {} });
  assert.match(plainText(html), /调度优先级 1/);
  const { state, props, nextTick } = await setupCard({ task: { ...task,
    mission_config: { priority: 4 }, startConfig: { priority: 99 } }, progress: {} });
  assert.equal(state.priorityConfig.value.priority, 4);
  props.task.mission_config = { priority: 12 };
  await nextTick();
  assert.equal(state.priorityConfig.value.priority, 12);
});

test('actual SFC SSR keeps paused in header and control line for active/blocked/stale/missing research metadata', async () => {
  for (const rounds of [true, false]) {
    for (const payload of [groupedEvent(), groupedEvent({ status: 'blocked', reason: 'feedback_incomplete_or_wait_timeout' }),
      groupedEvent({ stale: true, last_error: 'registry timeout' }),
      groupedEvent({ status: null, stage: null, stats_confirmed: false, counts: null }), null,
      event({ mode: 'legacy', status: 'active' })]) {
      const paused = { ...task, status: 'paused', stopConfirmedAt: Date.now(),
        stopConfirmedTaskId: 'id-a', stopConfirmedRemote: true, mission_config: { rounds: { enabled: rounds } } };
      const html = await render({ task: paused, taskRuntimeStatus: 'running',
        progress: { stale: false, success: 7, error: 2, total: 60, priority_campaign: payload } });
      const { main, details } = payload || rounds ? sections(html) : { main: html, details: '' };
      const text = plainText(main);
      assert.match(text, /pipeline__priority\s*paused/); // actual header slot, not just helper
      assert.match(text, /任务状态：paused/);
      assert.doesNotMatch(text, /任务状态：(active|blocked|running|等待root池状态确认)/);
      if (payload || rounds) assert.match(text, /研究状态：/);
      if (payload?.pool_id && payload.counts) {
        assert.match(text, /glb_spread · Roots 118 · R0 29 \/ R1 89 \/ R2 0 \/ R3 0/);
        assert.match(text, /asi_revision · Roots 26 · R0 7 \/ R1 3 \/ R2 8 \/ R3 8/);
        assert.match(text, /✓ 50/);
        assert.match(text, /✗ 0/);
        assert.match(text, /Σ 240/);
        assert.match(plainText(details), /每 root ≤3阶段 · ≤60\/阶段 · ≤180累计/);
        if (payload.stale) assert.match(text, /registry timeout.*保留上次实际统计/);
      }
      if (payload?.mode === 'legacy') assert.match(text, /旧 campaign 记录（只读）/);
    }
  }
});

test('late successful active/blocked/stale events cannot replace paused badge or control line; counts still update', async () => {
  const { hooks, state, props, nextTick } = await setupCard({ task: { ...task, status: 'running' },
    taskRuntimeStatus: 'running', progress: { stale: false, priority_campaign: groupedEvent() } });
  try {
    await hooks.mount();
    props.task.status = 'paused';
    props.stopOperation = { _id: 'id-a', name: task.name, isRemote: true, status: 'paused',
      receipt: { confirmed: true, taskId: 'id-a', taskName: task.name, isRemote: true, stopConfirmedAt: Date.now() } };
    await nextTick();
    for (const [index, extra] of [{ status: 'active' }, { status: 'blocked' },
      { status: 'active', stale: true, last_error: 'registry timeout' },
      { status: null, stage: null }, { status: 'completed', unresolved: 0 }].entries()) {
      hooks.handler({ payload: groupedEvent({ ...extra, updated_at: 101 + index,
        counts: { total: 240, success: 51 + index, error: 2 } }) });
      assert.equal(state.displayStatus.value, 'paused');
      assert.equal(state.displayRuntimeStatus.value, 'paused');
      assert.equal(state.displayProgress.value.success, 51 + index);
      assert.equal(state.displayProgress.value.error, 2);
      assert.equal(state.sourceGroups.value[0].root_count, 118);
    }
    props.progress = { stale: true, priority_campaign: null };
    await nextTick();
    assert.equal(state.displayStatus.value, 'paused');
    assert.equal(state.displayRuntimeStatus.value, 'paused');
    assert.equal(state.displayProgress.value.total, 240);
  } finally { hooks.unmount(); }
});

test('research status is explicitly not liveness; paused-only precedence preserves historical error/blocked behavior', async () => {
  const saved = groupedEvent();
  const html = await render({ task: { ...task, status: 'running' }, taskRuntimeStatus: 'error',
    progress: { stale: false, priority_campaign: saved } });
  assert.match(plainText(html), /研究状态：active/);
  assert.equal(card.priorityDisplayStatus(event(), true, 'error'), 'blocked');
  assert.equal(card.priorityDisplayStatus(saved, true, 'error'), 'active');
  assert.equal(card.priorityDisplayStatus(null, true, 'paused'), '暂停标记无有效停止凭据');
  assert.equal(card.priorityDisplayStatus(null, false, 'paused'), '暂停标记无有效停止凭据');
  assert.equal(card.priorityDisplayStatus(saved, true, 'paused'), '暂停标记无有效停止凭据');
  assert.equal(card.priorityDisplayStatus(saved, true, 'paused', 'paused'), 'paused');
});


test('actual SFC SSR requires stop proof, never mistakes legacy paused control or active research for stopped process', async () => {
  for (const payload of [groupedEvent(), groupedEvent({ status: 'blocked' }),
    groupedEvent({ stale: true, last_error: 'metadata timeout' }), null]) {
    for (const extra of [{}, { stopConfirmedAt: 'invalid', stopConfirmedTaskId: 'id-a', stopConfirmedRemote: true },
      { stopConfirmedAt: Date.now(), stopConfirmedTaskId: 'old-id', stopConfirmedRemote: true },
      { stopConfirmedAt: Date.now(), stopConfirmedTaskId: 'id-a', stopConfirmedRemote: false }]) {
      const html = await render({ task: { ...task, ...extra, status: 'paused' }, taskRuntimeStatus: 'running',
        progress: { stale: false, priority_campaign: payload } });
      const { main } = sections(html);
      const text = plainText(main);
      assert.match(text, /pipeline__priority\s*暂停标记无有效停止凭据/);
      assert.match(text, /任务状态：暂停标记无有效停止凭据/);
      assert.doesNotMatch(text, /任务状态：paused|任务状态：active/);
      if (payload?.counts) { assert.match(text, /✓ 50/); assert.match(text, /Σ 240/); }
    }
  }
});

test('real SFC SSR and late events keep stopping/unconfirmed above all research states, preserving real counts and groups', async () => {
  for (const [status, label] of [['stopping', '停止执行中'], ['stop_unconfirmed', '停止执行失败']]) {
    const stopOperation = { _id: 'id-a', name: task.name, isRemote: true, status };
    for (const payload of [groupedEvent(), groupedEvent({ stale: true, last_error: 'registry timeout' }), null]) {
      const html = await render({ task: { ...task, status: 'running' }, stopOperation,
        taskRuntimeStatus: 'running', progress: { stale: false, priority_campaign: payload } });
      const { main } = sections(html);
      assert.match(plainText(main), new RegExp(`pipeline__priority\\s*${label}`));
      assert.match(plainText(main), new RegExp(`任务状态：${label}`));
      assert.doesNotMatch(plainText(main), /任务状态：paused|任务状态：active/);
      if (payload?.counts) {
        assert.match(plainText(main), /✓ 50.*✗ 0.*Σ 240/);
        assert.match(plainText(main), /glb_spread · Roots 118 · R0 29/);
      }
    }
    const { hooks, state } = await setupCard({ task: { ...task, status: 'paused' }, stopOperation,
      progress: { stale: false, priority_campaign: groupedEvent() } });
    try {
      await hooks.mount();
      for (const [index, extra] of [{ status: 'active' }, { status: 'blocked' },
        { status: 'active', stale: true, last_error: 'timeout' }].entries()) {
        hooks.handler({ payload: groupedEvent({ ...extra, updated_at: 110 + index }) });
        assert.equal(state.displayStatus.value, label);
        assert.equal(state.displayRuntimeStatus.value, label);
        assert.equal(state.displayProgress.value.total, 240);
      }
    } finally { hooks.unmount(); }
  }
});

test('actual card operation prop is identity/host-fenced; persisted proof confirms remount only on current target', async () => {
  for (const extra of [{ _id: 'other' }, { name: 'other' }, { isRemote: false }]) {
    const html = await render({ task: { ...task, status: 'paused' }, stopOperation: {
      _id: 'id-a', name: task.name, isRemote: true, status: 'stopping', ...extra },
      progress: { stale: false, priority_campaign: groupedEvent() } });
    assert.match(plainText(html), /任务状态：暂停标记无有效停止凭据/);
    assert.doesNotMatch(plainText(html), /任务状态：stopping/);
  }
  const persisted = { ...task, status: 'paused', stopConfirmedAt: { $date: { $numberLong: String(Date.now()) } },
    stopConfirmedRemote: true, stopConfirmedTaskId: 'id-a' };
  const { state } = await setupCard({ task: persisted, progress: {} });
  assert.equal(state.displayStatus.value, 'paused');
  assert.equal(state.displayRuntimeStatus.value, 'paused');
  assert.equal(state.runtime.value, null);
});

test('research-only paused metadata cannot produce a confirmed paused badge without proof', async () => {
  const html = await render({ task: { ...task, status: 'running' }, taskRuntimeStatus: 'paused',
    progress: { stale: false, priority_campaign: groupedEvent({ status: 'paused' }) } });
  assert.match(plainText(html), /pipeline__priority\s*暂停标记无有效停止凭据/);
  assert.doesNotMatch(plainText(html), /pipeline__priority\s*paused/);
});

test('same-ID BSON object list reload retains real cached runtime counts while proof/operation display stays fenced', async () => {
  const saved = { ...task, _id: { $oid: 'id-a' }, status: 'paused', stopConfirmedAt: Date.now(),
    stopConfirmedTaskId: 'id-a', stopConfirmedRemote: true };
  const { state, props, nextTick } = await setupCard({ task: saved,
    progress: { stale: false, priority_campaign: groupedEvent() } });
  assert.equal(state.displayProgress.value.total, 240);
  props.task = { ...saved, _id: { $oid: 'id-a' } }; // new JSON object, same logical task
  await nextTick();
  assert.equal(state.displayProgress.value.total, 240);
  assert.equal(state.displayStatus.value, 'paused');
  props.task = { ...saved, _id: { $oid: 'id-b' } };
  await nextTick();
  assert.equal(state.runtime.value, null);
  assert.equal(state.displayStatus.value, '暂停标记无有效停止凭据');
});

test('actual Priority SFC receives structured consumer diagnostics and displays full stages/failure causes above research details', async () => {
  const current = { ...task, status: 'running' };
  const observation = extra => ({ taskId: 'id-a', name: task.name, isRemote: true, observedAt: Date.now(),
    state: 'unknown', stage: 'process', detail: '核验所有 pane 及消费者进程树', ...extra });
  for (const [runtime, expected, reason] of [[null, 'running', '未主动查询进程'],
    [observation({ state: 'checking' }), '运行检查进行中', '核验所有 pane 及消费者进程树'],
    [observation({ error: '[process] Permission denied: /proc/99/stat；exit=13，未发送停止命令' }), '运行检查执行失败', 'Permission denied: /proc/99/stat；exit=13，未发送停止命令'],
    [observation({ error: 'SSH 检查超时（15秒）；等待原连接返回，不重启任务' }), '运行检查超时', 'SSH 检查超时（15秒）；等待原连接返回，不重启任务'],
    [observation({ taskId: 'other' }), '运行检查身份校验失败', '身份校验失败'],
    [observation({ observedAt: Date.now() - 600000 }), '运行检查结果已过期', '运行检查快照已达到 600 秒有效期'],
  ]) {
    const html = await render({ task: current, taskRuntimeStatus: runtime,
      progress: { stale: false, priority_campaign: groupedEvent() } });
    const { main } = sections(html); const text = plainText(main);
    assert.ok(text.includes(`${runtime ? '消费者检查' : '业务控制状态'}：${expected}`)); assert.ok(text.includes(reason));
    if (runtime?.stage) assert.ok(text.includes(`[${runtime.stage}]`));
    assert.match(text, /研究状态：active/);
    assert.match(text, /✓ 50.*✗ 0.*Σ 240/);
    assert.match(text, /glb_spread · Roots 118/);
    assert.doesNotMatch(text, /消费者检查：(unknown|未知|运行状态未确认)/i);
  }
  const operation = { _id: 'id-a', name: task.name, isRemote: true, status: 'stop_unconfirmed',
    stage: 'control_write', failureSource: 'native', detail: '执行退出后回写原控制记录',
    error: '[control_write] 执行退出已核验，但 CAS matched_count=0，目标变化，未写入 paused（完整原因尾部）' };
  const html = await render({ task: current, stopOperation: operation,
    progress: { stale: false, priority_campaign: groupedEvent() } });
  const { main } = sections(html); const text = plainText(main);
  assert.match(text, /pipeline__priority\s*停止执行失败/);
  assert.ok(text.includes(operation.error)); assert.ok(text.includes('[control_write]'));
  assert.match(text, /执行退出已核验；control_write 控制记录写入失败/);
  assert.doesNotMatch(text, /pipeline__priority\s*paused|任务状态：paused/);
  assert.match(text, /✓ 50.*✗ 0.*Σ 240/);
});

// The expiry is a one-shot local presentation timer, never a native task poll.
test('Priority structured observations expire via one-shot timer and unmount disposes the timer', async (t) => {
  t.mock.timers.enable({ apis: ['Date', 'setTimeout'], now: 1800000000000 });
  const { hooks, state, nextTick } = await setupCard({ task: { ...task, status: 'running' },
    taskRuntimeStatus: { taskId: 'id-a', name: task.name, isRemote: true, observedAt: Date.now(), state: 'running' },
    progress: { stale: false, priority_campaign: groupedEvent() } });
  try {
    await hooks.mount();
    assert.equal(state.consumerStatus.value, '消费者存活（缺少有效派发快照）');
    t.mock.timers.tick(600000); await nextTick();
    assert.equal(state.consumerStatus.value, '运行检查结果已过期');
    assert.match(state.diagnosticDetails.value, /600 秒/);
    hooks.unmount(); t.mock.timers.tick(600000); await nextTick();
    assert.equal(state.consumerStatus.value, '运行检查结果已过期');
  } finally { hooks.unmount(); t.mock.timers.reset(); }
});

test('actual Priority SFC does not invent a CAS failure after control_write timeout/disposal', async () => {
  for (const failureSource of ['client_timeout', 'client_disposed', 'native']) {
    const operation = { _id: 'id-a', name: task.name, isRemote: true, status: 'stop_unconfirmed',
      stage: 'control_write', detail: '执行退出已核验；CAS 写入中', failureSource,
      failureKind: failureSource === 'client_disposed' ? 'error' : 'timeout',
      error: failureSource === 'client_disposed' ? '页面已离开，未收到最终回执' : '[control_write] 等待最终回执超时' };
    const { main } = sections(await render({ task: { ...task, status: 'running' }, stopOperation: operation,
      progress: { stale: false, priority_campaign: groupedEvent() } }));
    const text = plainText(main);
    assert.ok(text.includes(operation.error));
    assert.match(text, /control_write 最终写入结果待核对/);
    assert.doesNotMatch(text, /控制记录写入失败|pipeline__priority\s*paused/);
    assert.match(text, /✓ 50.*✗ 0.*Σ 240/);
  }
});

test('copy regression: Priority local/remote waiting, failed refresh and existing results remain factual', async () => {
  for (const isRemote of [false, true]) {
    const current = { ...task, status: 'running', isRemote };
    const snapshot = poolEvent({ is_remote: isRemote, status: 'waiting', reason: 'source_consumer_upgrade_required' });
    const input = { task: current, progress: { stale: false, priority_campaign: snapshot } };
    const before = JSON.stringify(input);
    const text = plainText(await render(input));
    assert.match(text, /研究状态：waiting/);
    assert.match(text, /等待来源任务 normal consumer 的 protocol3 支持/);
    assert.match(text, /✓ 50.*✗ 0.*Σ 240/);
    assert.match(text, /每 root ≤3阶段 · ≤60\/阶段 · ≤180累计/);
    assert.match(text, /待完成 190（含等待与待核验结果）/);
    assert.doesNotMatch(text, /不是|不代表|不能|不计为|不自动|standalone Consumer|无池全局预算/);
    assert.equal(JSON.stringify(input), before);
    const error = '读取失败：账号不匹配；host=selected；taskId=id-a';
    const failed = { ...snapshot, stats_confirmed: false, counts: null, stale: true, last_error: error };
    const failedText = plainText(await render({ task: current, progress: { stale: true, priority_campaign: failed } }));
    assert.ok(failedText.includes(error));
    assert.match(failedText, /✓ —.*✗ —.*Σ —（进度未确认）/);
    const { hooks, state } = await setupCard(input);
    try {
      await hooks.mount();
      hooks.handler({ payload: failed });
      assert.deepEqual(state.displayProgress.value, snapshot.counts);
      assert.equal(state.progressStale.value, true);
      assert.equal(state.progressError.value, error);
    } finally { hooks.unmount(); }
  }
});

test('v4 R3 actual Priority card retains details but fences known remote-only mutators', async () => {
  for (const controlSource of ['remote_only', 'local', undefined]) {
    const app = createSSRApp({ render: () => h(card.default, { task: { ...task, taskType: 'priority', status: 'pending', controlSource } }) });
    app.config.warnHandler = () => {};
    for (const name of ['n-card', 'n-tag', 'n-text', 'n-space', 'n-switch']) app.component(name,
      { setup: (_, { slots }) => () => h('div', [slots.header?.(), slots.default?.()]) });
    app.component('n-button', { props: ['disabled'], setup: (props, { slots }) => () =>
      h('button', { disabled: props.disabled }, [slots.icon?.(), slots.default?.()]) });
    const html = await renderToString(app);
    const buttons = [...html.matchAll(/<button([^>]*)>([\s\S]*?)<\/button>/g)];
    const byLabel = label => buttons.find(row => row[2].replace(/<[^>]*>/g, '').trim().endsWith(label));
    assert.equal(byLabel('查看任务详情')[1].includes('disabled'), false);
    for (const label of ['启动', '更新', '停止', '删除']) assert.equal(byLabel(label)[1].includes('disabled'), controlSource === 'remote_only');
    assert.doesNotMatch(html.replace(/<[^>]*>/g, ''), /本控端登记|执行端可为本地或远程|登记来源未提供/);
    if (controlSource === 'remote_only') assert.match(html.replace(/<[^>]*>/g, ''), /仅远端登记/);
  }
});
