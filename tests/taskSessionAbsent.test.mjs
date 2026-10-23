import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { pathToFileURL } from 'node:url';
import { parse, compileScript } from '@vue/compiler-sfc';
import { createSSRApp, h } from 'vue';
import { renderToString } from '@vue/server-renderer';
import { createPinia, setActivePinia } from 'pinia';
import { useTaskStore, validStopReceipt, taskStopDisplayStatus } from '../src/stores/taskStore.js';
import { TASK_STAGE_MARKER, validOperationStage, stopDiagnostic } from '../src/utils/taskDiagnostics.js';

const require = createRequire(import.meta.url);
const vue = pathToFileURL(require.resolve('vue/dist/vue.runtime.esm-bundler.js')).href;
const helper = new URL('../src/utils/taskDiagnostics.js', import.meta.url).href;
const managerSource = await readFile(new URL('../src/views/TaskManager.vue', import.meta.url), 'utf8');
const script = parse(managerSource).descriptor.script.content.replaceAll("'../utils/taskDiagnostics.js'", JSON.stringify(helper));
const manager = await import(`data:text/javascript;base64,${Buffer.from(script).toString('base64')}`);
const target = (remote = false) => ({ taskId: 'exact-id', name: 'exact_task', isRemote: remote, authProfile: 'user1' });
const task = (extra = {}) => ({ _id: 'exact-id', name: 'exact_task', isRemote: false, taskType: 'regular', status: 'running', auth_profile: 'user1', ...extra });
const absence = (remote = false, name = 'exact_task') => `❌ Session '${name}' 不存在 (${remote ? 'remote' : 'local'})`;
const stage = (remote = false, name = 'exact_task', phase = 'session_absent') => TASK_STAGE_MARKER + JSON.stringify({ operation: 'runtime', task_name: name, target_remote: remote, stage: phase, message: '已成功检查，精确会话不存在' });
const stamp = Date.now();
const stub = { setup: (_, { slots }) => () => h('div', [slots.header?.(), slots.default?.()]) };
const plain = html => html.replace(/<[^>]*>/g, ' ').replace(/\s+/g, ' ');
async function card(file, props, raw = false) {
  const source = await readFile(new URL(`../src/components/TaskCards/${file}.vue`, import.meta.url), 'utf8');
  let code = compileScript(parse(source).descriptor, { id: file, inlineTemplate: true }).content
    .replaceAll("'../../utils/taskDiagnostics.js'", JSON.stringify(helper))
    .replaceAll("'../../stores/taskStore.js'", JSON.stringify(new URL('../src/stores/taskStore.js', import.meta.url).href))
    .replaceAll("'../../utils/taskGeneration.js'", JSON.stringify(new URL('../src/utils/taskGeneration.js', import.meta.url).href))
    .replaceAll("'../../utils/taskConfig.js'", JSON.stringify(new URL('../src/utils/taskConfig.js', import.meta.url).href))
    .replaceAll('from "vue"', `from ${JSON.stringify(vue)}`)
    .replaceAll("from 'vue'", `from ${JSON.stringify(vue)}`)
    .replace(/import \{ listen \} from '@tauri-apps\/api\/event';/, 'const listen = async () => () => {};')
    .replace(/import \{ invoke \} from '@tauri-apps\/api\/core';/, 'const invoke = async () => {};');
  const module = await import(`data:text/javascript;base64,${Buffer.from(code).toString('base64')}`);
  const app = createSSRApp({ render: () => h(module.default, props) });
  app.config.warnHandler = () => {};
  for (const name of ['n-card', 'n-tag', 'n-text', 'n-space', 'n-switch', 'n-button', 'n-empty', 'n-collapse', 'n-collapse-item', 'n-grid', 'n-gi', 'n-tooltip']) app.component(name, stub);
  const html = await renderToString(app);
  return raw ? html : plain(html);
}

for (const remote of [false, true]) test(`exact successful session absence is paused, target remote=${remote}`, async () => {
  const output = `${stage(remote)}\n${absence(remote)}\n${stage(remote, 'exact_task', 'complete')}`;
  const observed = manager.normalizeTaskRuntime(target(remote), output, stamp);
  assert.equal(observed.state, 'session_absent');
  assert.equal(observed.error, null);
  assert.equal(validOperationStage({ operation: 'runtime', ...target(remote), requestId: 'request-1', stage: 'session_absent', message: '精确会话不存在' }), true);
  const text = await card('RegularTaskCard', { task: task({ isRemote: remote }), taskRuntimeStatus: observed });
  assert.match(text, /已暂停（tmux会话缺失）/);
  assert.doesNotMatch(text, /运行状态未确认|停止状态未确认|运行检查执行失败/);
});

test('no-session progress alone, wrong name/host, mixed pane and transport failure never fake paused', () => {
  for (const [output, error] of [[stage(), null], [absence(false, 'other'), null], [absence(true), null],
    [`${absence()}\n✅ Pane 0 (pid=123) is running: python3`, null], [absence(), 'SSH permission denied'],
    [`${stage(false, 'other')}\n${absence()}`, null], ['Permission denied', null], ['', null]]) {
    assert.notEqual(manager.normalizeTaskRuntime(target(), output, stamp, error).state, 'session_absent');
  }
});

test('Priority root snapshot cannot override an actual successful absent session observation', async () => {
  const observed = manager.normalizeTaskRuntime(target(), `${stage()}\n${absence()}`, stamp);
  const text = await card('PriorityTaskCard', { task: task({ taskType: 'priority', mission_config: { rounds: { enabled: true } } }), taskRuntimeStatus: observed });
  assert.match(text, /已暂停（tmux会话缺失）/);
  assert.doesNotMatch(text, /尚未取得 root 池状态/);
});

test('absence-basis stop receipt is scoped, persisted, and never described as orphan PID exit', () => {
  setActivePinia(createPinia());
  const store = useTaskStore(); store.replaceTasks([task()]);
  const op = store.beginTaskStop(task());
  const receipt = { confirmed: true, taskId: 'exact-id', taskName: 'exact_task', isRemote: false, stopConfirmedAt: stamp, confirmationBasis: 'session_absent' };
  assert.equal(validStopReceipt(task(), receipt), true);
  for (const patch of [{ taskId: 'other' }, { isRemote: true }, { confirmationBasis: 'unknown' }, { confirmationBasis: true }]) assert.equal(validStopReceipt(task(), { ...receipt, ...patch }), false);
  assert.equal(store.confirmTaskPaused(task(), receipt, op), true);
  assert.equal(store.tasks[0].stopConfirmationBasis, 'session_absent');
  assert.equal(taskStopDisplayStatus(store.tasks[0], store.taskStopOperationMap.exact_task), 'paused');
  const detail = stopDiagnostic(store.tasks[0], store.taskStopOperationMap.exact_task, 'paused').detail;
  assert.match(detail, /tmux会话缺失/); assert.match(detail, /已核验目标主机的精确会话缺失、任务身份、执行目标及暂停记录回写/);
  assert.doesNotMatch(detail, /原记录执行进程退出及控制记录写入均匹配/);
  store.clearTaskStop(store.tasks[0]); assert.equal(store.tasks[0].stopConfirmationBasis, undefined);
});

test('legacy native optional null basis does not invalidate identity-bound persisted stop proof', () => {
  const legacy = task({ status: 'paused', stopConfirmedAt: stamp, stopConfirmedTaskId: 'exact-id', stopConfirmedRemote: false, stopConfirmationBasis: null });
  assert.equal(taskStopDisplayStatus(legacy, null), 'paused');
  for (const basis of ['bogus', true, 0, {}]) assert.notEqual(taskStopDisplayStatus({ ...legacy, stopConfirmationBasis: basis }, null), 'paused');
});


for (const file of ['RegularTaskCard', 'PriorityTaskCard']) test(`${file}: diagnosis defaults collapsed, short title and complete expandable reason remain`, async () => {
  const observation = { ...target(), observedAt: Date.now(), state: 'unknown', stage: 'ssh',
    detail: '等待原配置 SSH 连接', error: '[ssh] Permission denied：具体连接错误' };
  const html = await card(file, { task: task({ taskType: file === 'PriorityTaskCard' ? 'priority' : 'regular' }), taskRuntimeStatus: observation }, true);
  const match = html.match(/<details\b([^>]*class="operation-diagnostics"[^>]*)>([\s\S]*?)<\/details>/);
  assert.ok(match, 'diagnosis must live in native expandable details');
  assert.doesNotMatch(match[1], /(?:^|\s)open(?:\s|=|$)/, 'must not default open');
  assert.equal(match[2].match(/<summary[^>]*>(.*?)<\/summary>/s)[1], '运行 / 停止详情');
  assert.match(match[2].slice(match[2].indexOf('</summary>')), /ssh/);
  assert.match(match[2], /Permission denied：具体连接错误/, 'expanded body must preserve original cause');
  assert.match(html.slice(0, html.indexOf('<details')), /运行检查执行失败/, 'status badge stays visible outside collapsed content');
});
