import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { pathToFileURL } from 'node:url';
import { parse, compileScript } from '@vue/compiler-sfc';
import { createSSRApp, createRenderer, h, reactive, nextTick } from 'vue';
import { renderToString } from '@vue/server-renderer';

// Execute the actual SFC normalizer exports and actual compiled Regular card,
// not copied badge logic or a stand-in component.
const managerSource = await readFile(new URL('../src/views/TaskManager.vue', import.meta.url), 'utf8');
const manager = await import(`data:text/javascript;base64,${Buffer.from(parse(managerSource).descriptor.script.content).toString('base64')}`);
const require = createRequire(import.meta.url);
const vueModule = pathToFileURL(require.resolve('vue/dist/vue.runtime.esm-bundler.js')).href;
const storeModule = new URL('../src/stores/taskStore.js', import.meta.url).href;
const cardSource = await readFile(new URL('../src/components/TaskCards/RegularTaskCard.vue', import.meta.url), 'utf8');
const compiled = compileScript(parse(cardSource).descriptor, { id: 'dispatch-card', inlineTemplate: true }).content
  .replaceAll("'../../stores/taskStore.js'", JSON.stringify(storeModule))
  .replaceAll('from "vue"', `from ${JSON.stringify(vueModule)}`)
  .replaceAll("from 'vue'", `from ${JSON.stringify(vueModule)}`);
const card = await import(`data:text/javascript;base64,${Buffer.from(compiled).toString('base64')}`);
const id = '650000000000000000000001';
const target = { taskId: id, name: 'regular-source', isRemote: true, authProfile: 'user1' };
const task = (patch = {}) => ({ _id: { $oid: id }, name: target.name, isRemote: true, taskType: 'regular',
  status: 'running', auth_profile: 'user1', mission_config: { max_concurrent: 8, max_multi_simulation_children: 10 }, ...patch });
const positive = '✅ Pane 0 (pid=21733) is running: python3.12';
const negative = '❌ Pane 0 (pid=21733) is running: bash';
const marker = 'QUANTNIGHT_DISPATCH_STATE ';
const wait = '请求结果待核验（派发暂停，消费者存活）';
const quotaWait = '等待配额恢复（派发暂停，消费者存活）';
const unknownDispatch = '消费者存活（派发状态未确认）';
const meta = (state = 'waiting_reconciliation', now = Date.now()) => ({ schemaVersion: 1, ...target,
  submissionState: state,
  submissionReason: state === 'ready' ? null : state === 'waiting_quota_reset'
    ? 'daily simulation safety stop reached: remaining=496, floor=500'
    : 'simulation_quota_unconfirmed: unknown response without trustworthy daily quota headers; reconcile budget without a POST probe',
  heartbeatAt: now, consumerRunId: '0123456789abcdef0123456789abcdef', pid: 21740,
  quota: { limit: 5000, remaining: state === 'waiting_quota_reset' ? 496 : 3965, stopFloor: 500, budgetConfirmed: state === 'waiting_quota_reset' },
  resetAt: now + 3600000 });
const wire = (m = meta(), process = positive) => `${process}\n${marker}${JSON.stringify(m)}`;
const observation = (m = meta(), process = positive, now = Date.now(), error = null) => manager.normalizeTaskRuntime(target, wire(m, process), now, error);
const plain = html => html.replace(/<[^>]*>/g, ' ').replace(/\s+/g, ' ').trim();
const stub = { setup: (_, { slots }) => () => h('div', [slots.header?.(), slots.default?.()]) };
const configure = app => {
  app.config.warnHandler = () => {};
  for (const name of ['n-card', 'n-tag', 'n-text', 'n-space', 'n-switch', 'n-button']) app.component(name, stub);
  return app;
};
const progress = { success: 11, priority_success: 4, error: 3, priority_error: 1, total: 100, priority_total: 20, stale: true, lastError: 'timeout' };
async function render(runtime, patch = {}) {
  return plain(await renderToString(configure(createSSRApp({ render: () => h(card.default,
    { task: task(), taskRuntimeStatus: runtime, progress, ...patch }) }))));
}
function preserved(text) {
  assert.match(text, /✓ 7\+4.*✗ 2\+1.*Σ 80\+20/);
  assert.match(text, /最大并发: 8.*子任务上限: 10/);
  assert.match(text, /进度刷新失败，保留上次统计并重试/);
}

test('actual native-string normalization keeps process truth and separates reconciliation, quota reset and ready', async () => {
  for (const [state, label] of [['waiting_reconciliation', wait], ['waiting_quota_reset', quotaWait], ['ready', '消费者存活（可派发）']]) {
    const runtime = observation(meta(state));
    assert.equal(runtime.raw, positive); // legacy Priority consumers never see suffix
    assert.equal(runtime.state, 'running');
    assert.equal(runtime.dispatch.submissionState, state);
    const before = JSON.stringify(runtime);
    const html = await render(runtime);
    assert.ok(html.includes(label)); preserved(html);
    assert.doesNotMatch(html, /regular-source running|配额耗尽|quota exhausted|paused|stopping/);
    assert.equal(JSON.stringify(runtime), before);
    assert.match(html, /重置时间（UTC）: \d{4}-\d\d-\d\dT/);
    if (state === 'waiting_reconciliation') {
      assert.match(html, /上次配额（未核验）: 3965\/5000/);
      assert.ok(!html.includes(quotaWait));
    }
  }
  const legacy = manager.normalizeTaskRuntime(target, positive, Date.now());
  assert.equal(legacy.state, 'running'); assert.equal(legacy.dispatch, null);
  assert.ok((await render(legacy)).includes(unknownDispatch));
});

test('valid wait metadata never masks absent/unknown process, empty output, timeout or permission/startup errors', async () => {
  for (const [process, error, label] of [[negative, null, '未检测到运行消费者'], ['', null, '运行状态未确认'],
    [positive + '\nPermission denied', null, '运行状态未确认'], ['SSH timeout', null, '运行状态未确认'],
    [positive.replace('pid=21733', 'pid=0'), null, '运行状态未确认'],
    [positive, 'Native timed out', '运行状态未确认'], [positive, 'script_config failed', '运行状态未确认']]) {
    const html = await render(observation(meta(), process, Date.now(), error));
    assert.ok(html.includes(label)); preserved(html);
    assert.ok(!html.includes(wait)); assert.ok(!html.includes(quotaWait));
    assert.doesNotMatch(html, /消费者存活|重置时间|上次配额|配额耗尽|paused/);
  }
});

test('empty, malformed, stale/future, fake identity/profile/PID/run/quota never acquire a dispatch badge', async () => {
  const now = Date.now();
  const patches = [{ schemaVersion: 2 }, { taskId: 'old' }, { name: 'other' }, { isRemote: false }, { isRemote: 'true' },
    { authProfile: 'fake' }, { authProfile: null }, { consumerRunId: 'fake' }, { consumerRunId: null },
    { pid: 0 }, { pid: 1 }, { pid: -1 }, { pid: '21740' }, { pid: 4294967296 }, { pid: null },
    { heartbeatAt: now - manager.DISPATCH_MAX_AGE }, { heartbeatAt: now + 5001 }, { heartbeatAt: String(now) },
    { submissionState: 'waiting_quota' }, { submissionState: 'UNKNOWN' }, { submissionReason: null },
    { submissionReason: 'line\nforgery' }, { quota: null }, { quota: { ...meta().quota, remaining: '3965' } },
    { quota: { ...meta().quota, remaining: 6000 } }, { quota: { ...meta().quota, budgetConfirmed: 'false' } },
    { resetAt: '2026-10-16T02:13:20Z' }, { resetAt: -1 }, { resetAt: 8640000000000001 }];
  for (const patch of patches) {
    const runtime = observation({ ...meta('waiting_reconciliation', now), ...patch }, positive, now);
    assert.equal(runtime.state, 'running'); assert.equal(runtime.dispatch, null, JSON.stringify(patch));
    assert.equal(card.regularDisplayStatus(task(), runtime, null, now), unknownDispatch);
    const exited = observation({ ...meta('waiting_reconciliation', now), ...patch }, negative, now);
    assert.equal(card.regularDisplayStatus(task(), exited, null, now), '未检测到运行消费者');
  }
  for (const raw of [`${positive}\n${marker}{broken`, `${positive}\n${marker}{}`, `${positive}\n${marker}null`,
    wire() + '\n' + marker + JSON.stringify(meta()), `${marker}${JSON.stringify(meta())}\n${positive}`]) {
    const runtime = manager.normalizeTaskRuntime(target, raw, now);
    assert.equal(runtime.dispatch, null); assert.equal(runtime.raw, positive); assert.equal(runtime.state, 'running');
  }
});

test('stop lifecycle wins over dispatch pause and metadata cannot serve as stop proof', async () => {
  const runtime = observation();
  for (const [status, label] of [['stopping', 'stopping'], ['stop_unconfirmed', '停止状态未确认'], ['paused', 'paused']]) {
    const stopOperation = { _id: id, name: target.name, isRemote: true, status,
      receipt: { confirmed: true, taskId: id, taskName: target.name, isRemote: true, stopConfirmedAt: Date.now() } };
    const html = await render(runtime, { stopOperation });
    assert.ok(html.includes(label)); assert.ok(!html.includes(wait));
    assert.doesNotMatch(html, /消费者存活|重置时间|上次配额/); preserved(html);
  }
  for (const [input, label] of [[task({ status: 'paused' }), '暂停（运行状态未确认）'],
    [task({ status: 'paused', stopConfirmedAt: Date.now(), stopConfirmedTaskId: id, stopConfirmedRemote: true }), 'paused']]) {
    const html = await render(runtime, { task: input });
    assert.ok(html.includes(label)); assert.ok(!html.includes(wait));
  }
});

test('ID/host/profile replacement and old observation expiry fences still win over fresh capability', () => {
  const now = Date.now(); const runtime = observation(meta('waiting_reconciliation', now), positive, now);
  for (const patch of [{ _id: 'other' }, { name: 'other' }, { isRemote: false }, { auth_profile: 'user2' }]) {
    assert.equal(card.regularDisplayStatus(task(patch), runtime, null, now), '运行状态未确认');
  }
  const old = { ...runtime, observedAt: now - card.REGULAR_RUNTIME_MAX_AGE };
  assert.equal(card.regularDisplayStatus(task(), old, null, now), '运行状态未确认');
  assert.equal(card.regularDisplayStatus(task(), runtime, null, now + manager.DISPATCH_MAX_AGE), unknownDispatch);
  assert.equal(card.regularDisplayStatus(task({ taskType: 'super' }), runtime, null, now), 'running');
});

function mount(props) {
  const node = (type, text = '') => ({ type, text, children: [], parent: null });
  const renderer = createRenderer({ createElement: type => node(type), createText: text => node('text', text), createComment: () => node('comment'),
    setText: (el, text) => { el.text = text; }, setElementText: (el, text) => { el.text = text; el.children = []; },
    parentNode: el => el.parent, nextSibling: el => el.parent?.children[el.parent.children.indexOf(el) + 1] || null, patchProp() {},
    insert(el, parent, anchor = null) { if (el.parent) el.parent.children.splice(el.parent.children.indexOf(el), 1); el.parent = parent;
      const i = anchor ? parent.children.indexOf(anchor) : -1; parent.children.splice(i < 0 ? parent.children.length : i, 0, el); },
    remove(el) { if (el.parent) el.parent.children.splice(el.parent.children.indexOf(el), 1); el.parent = null; } });
  const root = node('root'); const app = configure(renderer.createApp({ render: () => h(card.default, props) })); app.mount(root);
  const text = el => [el.text, ...el.children.map(text)].join(' ').replace(/\s+/g, ' ').trim();
  return { text: () => text(root), stop: () => app.unmount() };
}

test('mounted real SFC expires dispatch heartbeat at 120s then process cache at 600s, without polling', async t => {
  t.mock.timers.enable({ apis: ['Date', 'setTimeout'], now: 1800000000000 });
  const props = reactive({ task: task(), taskRuntimeStatus: observation(), progress }); const mounted = mount(props);
  try {
    assert.ok(mounted.text().includes(wait));
    t.mock.timers.tick(manager.DISPATCH_MAX_AGE - 1); await nextTick(); assert.ok(mounted.text().includes(wait));
    t.mock.timers.tick(1); await nextTick(); assert.ok(mounted.text().includes(unknownDispatch));
    assert.doesNotMatch(mounted.text(), /重置时间|上次配额/);
    t.mock.timers.tick(card.REGULAR_RUNTIME_MAX_AGE - manager.DISPATCH_MAX_AGE); await nextTick();
    assert.match(mounted.text(), /运行状态未确认/); assert.doesNotMatch(mounted.text(), /消费者存活/); preserved(mounted.text());
    for (const [state, label] of [['waiting_reconciliation', wait], ['waiting_quota_reset', quotaWait], ['ready', '消费者存活（可派发）']]) {
      props.taskRuntimeStatus = observation(meta(state)); await nextTick(); assert.ok(mounted.text().includes(label)); preserved(mounted.text());
    }
    props.taskRuntimeStatus = observation(meta(), negative); await nextTick(); assert.match(mounted.text(), /未检测到运行消费者/);
  } finally { mounted.stop(); t.mock.timers.reset(); }
});
