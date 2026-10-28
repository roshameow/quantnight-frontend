import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { pathToFileURL } from 'node:url';
import { parse, compileScript } from '@vue/compiler-sfc';
import { createSSRApp, createRenderer, h, reactive, nextTick } from 'vue';
import { renderToString } from '@vue/server-renderer';
import { createPinia, setActivePinia } from 'pinia';
import { useTaskStore } from '../src/stores/taskStore.js';

const require = createRequire(import.meta.url);
const vueModule = pathToFileURL(require.resolve('vue/dist/vue.runtime.esm-bundler.js')).href;
const storeModule = new URL('../src/stores/taskStore.js', import.meta.url).href;
const source = await readFile(new URL('../src/components/TaskCards/RegularTaskCard.vue', import.meta.url), 'utf8');
const { descriptor } = parse(source);
const compiled = compileScript(descriptor, { id: 'regular-runtime-test', inlineTemplate: true }).content
  .replaceAll("'../../utils/taskDiagnostics.js'", JSON.stringify(new URL('../src/utils/taskDiagnostics.js', import.meta.url).href))
  .replaceAll("'../../stores/taskStore.js'", JSON.stringify(storeModule))
  .replaceAll("'../../utils/taskGeneration.js'", JSON.stringify(new URL('../src/utils/taskGeneration.js', import.meta.url).href))
  .replaceAll('from "vue"', `from ${JSON.stringify(vueModule)}`)
  .replaceAll("from 'vue'", `from ${JSON.stringify(vueModule)}`);
const cardURL = `data:text/javascript;base64,${Buffer.from(compiled).toString('base64')}`;
const card = await import(cardURL);
const task = (extra = {}) => ({ _id: { $oid: 'regular-id' }, name: 'glb_spread', taskType: 'regular',
  status: 'running', isRemote: false, auth_profile: 'user1',
  mission_config: { max_concurrent: 3, max_multi_simulation_children: 10 }, ...extra });
const observation = (extra = {}) => ({ taskId: 'regular-id', name: 'glb_spread', isRemote: false,
  observedAt: Date.now(), state: 'running', raw: '✅ Pane 0 (pid=21733) is running: python3.12', ...extra });
const progress = { success: 11, error: 3, total: 100, priority_success: 4, priority_error: 1, priority_total: 20,
  stale: true, lastError: 'progress timeout' };
const plain = html => html.replace(/<[^>]*>/g, ' ').replace(/\s+/g, ' ').trim();
const stub = { setup: (_, { slots }) => () => h('div', [slots.header?.(), slots.default?.()]) };
const uiNames = ['n-card', 'n-tag', 'n-text', 'n-space', 'n-switch', 'n-button', 'n-empty',
  'n-collapse', 'n-collapse-item', 'n-grid', 'n-gi'];
function configure(app) {
  app.config.warnHandler = () => {};
  for (const name of uiNames) app.component(name, stub);
  return app;
}
async function render(props) {
  return renderToString(configure(createSSRApp({ render: () => h(card.default, props) })));
}
function assertPreserved(html) {
  const text = plain(html);
  assert.match(text, /✓ 7\+4.*✗ 2\+1.*Σ 80\+20/);
  assert.match(text, /进度刷新失败：/);
  assert.match(text, /最大并发: 3.*子任务上限: 10/);
  assert.match(text, /glb_spread/);
  assert.match(text, /user1/);
}

test('real regular SFC SSR: initial business running needs no PID observation; fresh positive, bash-only and unknown are distinct', async () => {
  for (const [runtime, label] of [[null, 'running'], [observation(), '消费者存活（缺少有效派发快照）'],
    [observation({ state: 'not_running', raw: '❌ Pane 0 (pid=21733) is running: bash' }), '未检测到运行消费者'],
    [observation({ state: 'unknown' }), '运行检查输出校验失败'], [observation({ state: 'checking' }), '运行检查进行中'],
    [observation({ state: 'paused' }), '运行检查输出校验失败']]) {
    const input = { task: task(), progress, taskRuntimeStatus: runtime };
    const before = JSON.stringify(input);
    const html = await render(input);
    assert.match(plain(html), new RegExp(`glb_spread ${label}`));
    assertPreserved(html);
    assert.equal(JSON.stringify(input), before);
    if (label !== '消费者存活（缺少有效派发快照）') assert.doesNotMatch(plain(html), /glb_spread 消费者存活（缺少有效派发快照）/);
    if (runtime) assert.doesNotMatch(plain(html), /glb_spread running/);
    assert.doesNotMatch(plain(html), /paused|任务完成|启动成功/);
  }
});

test('freshness and identity boundary rejects stale, future, invalid and wrong task/host observations', async () => {
  const now = Date.now();
  for (const patch of [{ taskId: 'old-id' }, { name: 'other' }, { isRemote: true }, { isRemote: 'false' },
    { observedAt: now - card.REGULAR_RUNTIME_MAX_AGE }, { observedAt: now + 60000 },
    { observedAt: -1 }, { observedAt: null }, { observedAt: NaN }, { observedAt: '123' }]) {
    const runtime = observation(patch);
    const expected = ['taskId', 'name', 'isRemote'].some(key => key in patch) ? '运行检查身份校验失败'
      : typeof patch.observedAt === 'number' && patch.observedAt > 0 && patch.observedAt <= now ? '运行检查结果已过期' : '运行检查时间无效';
    assert.equal(card.regularDisplayStatus(task(), runtime, null, now), expected);
    const html = await render({ task: task(), progress, taskRuntimeStatus: runtime });
    assert.ok(plain(html).includes(`glb_spread ${expected}`));
    assertPreserved(html);
  }
  assert.equal(card.regularDisplayStatus(task(), observation({ observedAt: now - card.REGULAR_RUNTIME_MAX_AGE + 1 }), null, now), '消费者存活（缺少有效派发快照）');
});

test('actual regular SFC SSR honors stop lifecycle and persisted pause proof above positive or missing consumer evidence', async () => {
  for (const runtime of [null, observation(), observation({ state: 'not_running' }), observation({ state: 'unknown' })]) {
    for (const [status, label] of [['stopping', '停止执行中'], ['stop_unconfirmed', '停止执行失败'], ['paused', 'paused']]) {
      const stopOperation = { _id: 'regular-id', name: 'glb_spread', isRemote: false, status,
        receipt: { confirmed: true, taskId: 'regular-id', taskName: 'glb_spread', isRemote: false, stopConfirmedAt: Date.now() } };
      const html = await render({ task: task(), taskRuntimeStatus: runtime, progress, stopOperation });
      assert.match(plain(html), new RegExp(`glb_spread ${label}`));
      assertPreserved(html);
    }
    const paused = task({ status: 'paused', stopConfirmedAt: { $date: { $numberLong: String(Date.now()) } },
      stopConfirmedTaskId: 'regular-id', stopConfirmedRemote: false });
    assert.match(plain(await render({ task: paused, taskRuntimeStatus: runtime })), /glb_spread paused/);
    for (const patch of [{}, { stopConfirmedAt: 'invalid' }, { stopConfirmedTaskId: 'wrong' }, { stopConfirmedRemote: true }]) {
      const input = Object.keys(patch).length ? { ...paused, ...patch } : task({ status: 'paused' });
      assert.match(plain(await render({ task: input, taskRuntimeStatus: runtime })), /glb_spread 暂停标记无有效停止凭据/);
    }
  }
});

test('static ready state and mismatched stop operations cannot imply stopped or running', async () => {
  assert.match(plain(await render({ task: task({ status: 'ready' }), taskRuntimeStatus: observation() })), /glb_spread ready/);
  for (const patch of [{ _id: 'other' }, { name: 'other' }, { isRemote: true }]) {
    const html = await render({ task: task(), taskRuntimeStatus: observation({ state: 'not_running' }),
      stopOperation: { _id: 'regular-id', name: 'glb_spread', isRemote: false, status: 'stopping', ...patch } });
    assert.match(plain(html), /glb_spread 未检测到运行消费者/);
    assert.doesNotMatch(plain(html), /stopping|paused/);
  }
});

// Mount the actual compiled SFC into an in-memory Vue renderer (no browser/app).
// This exercises prop/computed updates, not a copied status helper.
function memoryMount(props) {
  const node = (type, text = '') => ({ type, text, children: [], parent: null });
  const renderer = createRenderer({
    createElement: type => node(type), createText: text => node('text', text), createComment: () => node('comment'),
    setText: (el, text) => { el.text = text; }, setElementText: (el, text) => { el.text = text; el.children = []; },
    parentNode: el => el.parent, nextSibling: el => el.parent?.children[el.parent.children.indexOf(el) + 1] || null,
    patchProp() {},
    insert(el, parent, anchor = null) {
      if (el.parent) el.parent.children.splice(el.parent.children.indexOf(el), 1);
      el.parent = parent;
      const index = anchor ? parent.children.indexOf(anchor) : -1;
      parent.children.splice(index < 0 ? parent.children.length : index, 0, el);
    },
    remove(el) { if (el.parent) el.parent.children.splice(el.parent.children.indexOf(el), 1); el.parent = null; },
  });
  const root = node('root');
  const app = configure(renderer.createApp({ render: () => h(card.default, props) }));
  app.mount(root);
  const text = el => [el.text, ...el.children.map(text)].join(' ').replace(/\s+/g, ' ').trim();
  return { text: () => text(root), stop: () => app.unmount() };
}

test('mounted real card updates evidence/status without losing counters/config and fences reused props', async () => {
  const props = reactive({ task: task(), progress: { ...progress }, taskRuntimeStatus: null, stopOperation: null });
  const mounted = memoryMount(props);
  try {
    assert.match(mounted.text(), /glb_spread running/);
    for (const [runtime, label] of [[observation(), '消费者存活（缺少有效派发快照）'],
      [observation({ state: 'not_running' }), '未检测到运行消费者'],
      [observation({ state: 'unknown' }), '运行检查输出校验失败'], [observation(), '消费者存活（缺少有效派发快照）']]) {
      props.taskRuntimeStatus = runtime;
      await nextTick();
      assert.match(mounted.text(), new RegExp(`glb_spread ${label}`));
      assertPreserved(mounted.text());
    }
    props.task = task({ _id: 'replacement' }); await nextTick();
    assert.match(mounted.text(), /glb_spread 运行检查身份校验失败/);
    props.task = task({ isRemote: true }); await nextTick();
    assert.match(mounted.text(), /glb_spread 运行检查身份校验失败/);
    props.task = task();
    props.stopOperation = { _id: 'regular-id', name: 'glb_spread', isRemote: false, status: 'stop_unconfirmed' };
    await nextTick();
    props.taskRuntimeStatus = observation(); await nextTick();
    assert.match(mounted.text(), /glb_spread 停止执行失败/);
  } finally { mounted.stop(); }
});

test('actual TaskManager SFC SSR wires regular stop operation and pending runtime while preserving store progress', async () => {
  setActivePinia(createPinia());
  const store = useTaskStore();
  store.replaceTasks([task()]);
  store.updateProgress({ ...progress, collection: 'glb_spread', task_id: 'regular-id', is_remote: false, updated_at: 100 });
  const managerSource = await readFile(new URL('../src/views/TaskManager.vue', import.meta.url), 'utf8');
  const manager = compileScript(parse(managerSource).descriptor, { id: 'manager-runtime-ssr', inlineTemplate: true }).content
    .replaceAll('from "vue"', `from ${JSON.stringify(vueModule)}`)
    .replaceAll('from "pinia"', `from ${JSON.stringify(import.meta.resolve('pinia'))}`)
    .replace('"../stores/taskStore"', JSON.stringify(storeModule))
    .replace('"../utils/taskProgress.js"', JSON.stringify(new URL('../src/utils/taskProgress.js', import.meta.url).href))
    .replaceAll("'../utils/taskDiagnostics.js'", JSON.stringify(new URL('../src/utils/taskDiagnostics.js', import.meta.url).href))
    .replaceAll("'../utils/taskGeneration.js'", JSON.stringify(new URL('../src/utils/taskGeneration.js', import.meta.url).href))
    .replace(/import \{ invoke \} from "@tauri-apps\/api\/core";/, 'const invoke = () => { throw new Error("SSR must not invoke native"); };')
    .replace(/import \{ listen \} from "@tauri-apps\/api\/event";/, 'const listen = async () => () => {};')
    .replace(/import \{ useRoute, useRouter \} from "vue-router";/, 'const useRoute = () => ({ fullPath: "/tasks" }); const useRouter = () => ({});')
    .replace(/import \{ useMessage, useDialog \} from "naive-ui";/, 'const useMessage = () => ({}); const useDialog = () => ({});')
    .replace(/import RegularTaskCard from "[^"\n]+\.vue";/, `import RegularTaskCard from ${JSON.stringify(cardURL)};`)
    .replace(/import (\w+) from "[^"\n]+\.vue";/g, 'const $1 = { render: () => null };');
  const module = await import(`data:text/javascript;base64,${Buffer.from(manager).toString('base64')}`);
  const renderManager = () => renderToString(configure(createSSRApp(module.default)));
  assert.match(plain(await renderManager()), /glb_spread running/);
  const operation = store.beginTaskStop(task());
  assert.match(plain(await renderManager()), /glb_spread 停止执行中/);
  store.markTaskStopUnconfirmed(task(), operation, 'native timeout');
  const html = await renderManager();
  assert.match(plain(html), /glb_spread 停止确认超时/);
  assert.match(plain(html), /✓ 7\+4.*✗ 2\+1.*Σ 80\+20/);
});


test('mounted positive badge expires without any new props or native polling and cleans up its one-shot timer', async (t) => {
  t.mock.timers.enable({ apis: ['Date', 'setTimeout'], now: 1800000000000 });
  const props = reactive({ task: task(), taskRuntimeStatus: observation(), progress });
  const mounted = memoryMount(props);
  try {
    assert.match(mounted.text(), /glb_spread 消费者存活（缺少有效派发快照）/);
    t.mock.timers.tick(card.REGULAR_RUNTIME_MAX_AGE - 1); await nextTick();
    assert.match(mounted.text(), /glb_spread 消费者存活（缺少有效派发快照）/);
    t.mock.timers.tick(1); await nextTick();
    assert.match(mounted.text(), /glb_spread 运行检查结果已过期/);
    assertPreserved(mounted.text());
    props.taskRuntimeStatus = observation(); await nextTick();
    assert.match(mounted.text(), /glb_spread 消费者存活（缺少有效派发快照）/);
  } finally { mounted.stop(); t.mock.timers.reset(); }
});

test('actual Regular/Super SFC body shows full stage and failure reasons, not merely tooltip/toast or renamed unknown', async () => {
  for (const taskType of ['regular', 'super']) {
    for (const [runtime, label, cause] of [
      [null, 'running', '运行诊断（只读，可选）'],
      [observation({ state: 'checking', stage: 'ssh', detail: 'SSH 正在握手' }), '运行检查进行中', 'SSH 正在握手'],
      [observation({ state: 'unknown', stage: 'process', detail: '枚举后代进程', error: '[process] /proc/341/status Permission denied；退出码=13' }), '运行检查执行失败', '/proc/341/status Permission denied；退出码=13'],
      [observation({ state: 'unknown', stage: 'tmux', detail: '正在枚举精确会话', error: '15 秒检查超时：tmux 未返回；保留原回执' }), '运行检查超时', '15 秒检查超时：tmux 未返回；保留原回执'],
      [observation({ taskId: 'old-id', stage: 'identity', detail: '核对身份' }), '运行检查身份校验失败', '身份校验失败'],
      [observation({ observedAt: Date.now() - card.REGULAR_RUNTIME_MAX_AGE, stage: 'complete', detail: '旧进程快照' }), '运行检查结果已过期', '旧进程快照'],
      [observation({ state: 'unknown', stage: 'complete', detail: '枚举返回', raw: 'SSH banner\nPermission denied: actual output' }), '运行检查输出校验失败', 'Permission denied: actual output'],
    ]) {
      const html = await render({ task: task({ taskType }), progress, taskRuntimeStatus: runtime });
      const body = html.replace(/<[^>]*>/g, ' ');
      assert.ok(body.includes(label)); assert.ok(body.includes(cause));
      if (runtime?.stage) assert.ok(body.includes(`[${runtime.stage}]`));
      assert.doesNotMatch(body, /glb_spread\s*(unknown|未知|运行状态未确认|运行状态待确认)/i);
      assertPreserved(html);
    }
    const operation = { _id: 'regular-id', name: 'glb_spread', isRemote: false, status: 'stop_unconfirmed',
      stage: 'control_write', failureSource: 'native', detail: '原任务 ID/名称/主机 CAS 回写',
      error: '[control_write] 执行退出已核验，但 matched_count=0；身份变化，不能写入 paused；无自动重试' };
    const html = await render({ task: task({ taskType }), progress, taskRuntimeStatus: observation(), stopOperation: operation });
    assert.match(plain(html), /glb_spread 停止执行失败/);
    assert.ok(html.includes('[control_write]')); assert.ok(html.includes(operation.error));
    assert.match(plain(html), /执行退出已核验；control_write 控制记录写入失败/);
    assert.doesNotMatch(plain(html), /glb_spread paused/);
    assertPreserved(html);
  }
});

test('actual TaskManager -> actual Priority SFC passes structured runtime diagnosis, not raw string, and stop failure retains counts', async () => {
  setActivePinia(createPinia()); const store = useTaskStore();
  store.replaceTasks([task({ taskType: 'priority', mission_config: { rounds: { enabled: false } } })]);
  store.updateProgress({ ...progress, collection: 'glb_spread', task_id: 'regular-id', is_remote: false, updated_at: 100 });
  const prioritySource = await readFile(new URL('../src/components/TaskCards/PriorityTaskCard.vue', import.meta.url), 'utf8');
  const priorityCode = compileScript(parse(prioritySource).descriptor, { id: 'manager-priority-card', inlineTemplate: true }).content
    .replaceAll("'../../utils/taskDiagnostics.js'", JSON.stringify(new URL('../src/utils/taskDiagnostics.js', import.meta.url).href))
    .replaceAll("'../../stores/taskStore.js'", JSON.stringify(storeModule))
    .replaceAll("'../../utils/taskConfig.js'", JSON.stringify(new URL('../src/utils/taskConfig.js', import.meta.url).href))
    .replaceAll('from "vue"', `from ${JSON.stringify(vueModule)}`)
    .replaceAll("from 'vue'", `from ${JSON.stringify(vueModule)}`)
    .replace(/import \{ listen \} from '@tauri-apps\/api\/event';/, 'const listen = async () => () => {};')
    .replace(/import \{ invoke \} from '@tauri-apps\/api\/core';/, 'const invoke = async () => {};');
  const priorityURL = `data:text/javascript;base64,${Buffer.from(priorityCode).toString('base64')}`;
  const runtime = observation({ state: 'unknown', stage: 'ssh', detail: '原 SSH 入口连接阶段',
    error: '[ssh] Permission denied: selected host；未触达消费者（完整错误尾部）' });
  const managerSource = await readFile(new URL('../src/views/TaskManager.vue', import.meta.url), 'utf8');
  const code = compileScript(parse(managerSource).descriptor, { id: 'manager-priority-diagnostic', inlineTemplate: true }).content
    .replace('const taskRuntimeStatus = ref({});', `const taskRuntimeStatus = ref(${JSON.stringify({ glb_spread: runtime })});`)
    .replaceAll('from "vue"', `from ${JSON.stringify(vueModule)}`)
    .replaceAll('from "pinia"', `from ${JSON.stringify(import.meta.resolve('pinia'))}`)
    .replace('"../stores/taskStore"', JSON.stringify(storeModule))
    .replace('"../utils/taskProgress.js"', JSON.stringify(new URL('../src/utils/taskProgress.js', import.meta.url).href))
    .replaceAll("'../utils/taskDiagnostics.js'", JSON.stringify(new URL('../src/utils/taskDiagnostics.js', import.meta.url).href))
    .replaceAll("'../utils/taskGeneration.js'", JSON.stringify(new URL('../src/utils/taskGeneration.js', import.meta.url).href))
    .replace(/import \{ invoke \} from "@tauri-apps\/api\/core";/, 'const invoke = () => { throw new Error("SSR must not invoke native"); };')
    .replace(/import \{ listen \} from "@tauri-apps\/api\/event";/, 'const listen = async () => () => {};')
    .replace(/import \{ useRoute, useRouter \} from "vue-router";/, 'const useRoute = () => ({ fullPath: "/tasks" }); const useRouter = () => ({});')
    .replace(/import \{ useMessage, useDialog \} from "naive-ui";/, 'const useMessage = () => ({}); const useDialog = () => ({});')
    .replace(/import PriorityTaskCard from "[^"\n]+\.vue";/, `import PriorityTaskCard from ${JSON.stringify(priorityURL)};`)
    .replace(/import (\w+) from "[^"\n]+\.vue";/g, 'const $1 = { render: () => null };');
  const manager = await import(`data:text/javascript;base64,${Buffer.from(code).toString('base64')}`);
  const renderManager = () => renderToString(configure(createSSRApp(manager.default)));
  let text = plain(await renderManager());
  assert.match(text, /消费者检查：运行检查执行失败/);
  assert.ok(text.includes(runtime.error)); assert.ok(text.includes('[ssh]'));
  const operation = store.beginTaskStop(store.tasks[0]);
  store.markTaskStopUnconfirmed(store.tasks[0], operation, '[control_write] 执行退出已核验但写入失败：matched_count=0（CAS完整原因）');
  text = plain(await renderManager());
  assert.match(text, /glb_spread 停止执行失败/);
  assert.match(text, /\[control_write\].*matched_count=0（CAS完整原因）/);
  assert.match(text, /✓ 11.*✗ 3.*Σ 100/);
  assert.doesNotMatch(text, /glb_spread paused/);
});

test('legacy paused body identifies the actual missing, invalid or mismatched proof field', async () => {
  const valid = task({ status: 'paused', stopConfirmedAt: Date.now(), stopConfirmedTaskId: 'regular-id', stopConfirmedRemote: false });
  for (const [patch, reason] of [[{ stopConfirmedAt: null }, '缺少 stopConfirmedAt'],
    [{ stopConfirmedAt: 'invalid' }, 'stopConfirmedAt 停止时间凭据格式无效'],
    [{ stopConfirmedTaskId: null }, '缺少 stopConfirmedTaskId'],
    [{ stopConfirmedTaskId: 'old-id' }, '身份校验失败：停止凭据 stopConfirmedTaskId 的任务 ID'],
    [{ stopConfirmedRemote: null }, '缺少有效 stopConfirmedRemote'],
    [{ stopConfirmedRemote: true }, '身份校验失败：停止凭据 stopConfirmedRemote 的执行主机与当前本地/远程目标存在差异']]) {
    const text = plain(await render({ task: { ...valid, ...patch } }));
    assert.match(text, /glb_spread 暂停标记无有效停止凭据/);
    assert.ok(text.includes(reason)); assert.doesNotMatch(text, /glb_spread paused/);
  }
});

test('actual Regular SFC shows control_write timeout/disposal as missing final evidence, not a definitive CAS failure', async () => {
  for (const failureSource of ['client_timeout', 'client_disposed', 'native']) {
    const operation = { _id: 'regular-id', name: 'glb_spread', isRemote: false, status: 'stop_unconfirmed',
      stage: 'control_write', detail: '执行退出已核验；CAS 写入中', failureSource,
      failureKind: failureSource === 'client_disposed' ? 'error' : 'timeout',
      error: failureSource === 'client_disposed' ? '页面已离开，最终停止回执尚未收到' : '[control_write] 等待最终停止回执超时；写入结果尚未观测' };
    const text = plain(await render({ task: task(), stopOperation: operation, progress }));
    assert.ok(text.includes(operation.error));
    assert.match(text, /执行退出已核验，已进入 control_write；control_write 最终写入结果待核对/);
    assert.doesNotMatch(text, /控制记录写入失败|glb_spread paused/);
    assertPreserved(text);
  }
});

test('copy regression: local/remote checks preserve facts, raw failures, existing results and badge colors', async () => {
  for (const isRemote of [false, true]) {
    const current = task({ isRemote });
    const snapshot = patch => observation({ isRemote, stage: 'process', detail: '核验进程', ...patch });
    for (const [runtime, label, detail, color] of [
      [snapshot({ state: 'checking' }), '运行检查进行中', '正等待本次检查结果。', 'warning'],
      [snapshot({}), '消费者存活（缺少有效派发快照）', '已识别 Python/uv 消费者进程。', 'warning'],
      [snapshot({ state: 'session_absent' }), '已暂停（tmux会话缺失）', '已核验目标主机的精确 tmux 会话缺失。', 'warning'],
      [snapshot({ state: 'unknown', raw: '' }), '运行检查输出校验失败', '检查输出缺少可校验的 Python/uv 或 shell/sleep 进程行。', 'error'],
      [snapshot({ authProfile: 'other' }), '运行检查身份校验失败', '身份校验失败：检查结果的任务 ID、名称、执行主机、账号或启动批次与当前任务存在差异', 'error'],
    ]) {
      const input = { task: current, taskRuntimeStatus: runtime, progress };
      const before = JSON.stringify(input);
      const html = await render(input);
      assert.ok(plain(html).includes(label)); assert.ok(plain(html).includes(detail));
      assert.match(html, new RegExp(`class="runtime-badge" type="${color}"`));
      assert.doesNotMatch(plain(html), /不是|不代表|不能|不得|不匹配|不符合/);
      assertPreserved(html);
      assert.equal(JSON.stringify(input), before);
    }
    const error = '[ssh] 身份校验失败：auth_profile 不匹配；taskId=regular-id；不代表进程退出';
    const html = await render({ task: current, taskRuntimeStatus: snapshot({ state: 'unknown', error }), progress });
    assert.ok(plain(html).includes(error), 'upstream error remains verbatim, including negative wording');
    assert.match(html, /class="runtime-badge" type="error"/);
    assert.match(plain(html), /glb_spread 运行检查执行失败/);
    assertPreserved(html);
  }
});

test('actual Regular card (including SUPER reuse) guards only view-owned pending work; PID snapshots do not disable normal start', async () => {
  async function buttons(props) {
    const app = configure(createSSRApp({ render: () => h(card.default, props) }));
    app.component('n-button', { props: ['disabled'], setup: (props, { slots }) => () =>
      h('button', { disabled: props.disabled }, [slots.icon?.(), slots.default?.()]) });
    const html = await renderToString(app);
    const rows = [...html.matchAll(/<button([^>]*)>([\s\S]*?)<\/button>/g)];
    return { html, byLabel: label => rows.find(row => plain(row[2]).endsWith(label)) };
  }
  for (const taskType of ['regular', 'super']) {
    for (const [status, runtime, busy, blocked] of [
      ['running', null, false, false], ['launch_unknown', null, false, false],
      ['paused', null, false, false], // historical proof is left to the original core operation
      ['completed', observation(), false, false], ['completed_with_errors', observation(), false, false],
      ['completed', observation({ state: 'unknown' }), false, false],
      ['ready', observation({ state: 'session_absent' }), false, false],
      ['ready', observation({ state: 'session_absent' }), true, true],
    ]) {
      const result = await buttons({ task: task({ status, taskType }), taskRuntimeStatus: runtime, operationBusy: busy });
      assert.equal(result.byLabel('启动')[1].includes('disabled'), blocked, `${taskType}:${status}:${runtime?.state}`);
      assert.ok(!result.byLabel('查看')[1].includes('disabled'));
      assert.ok(!result.byLabel('暂停')[1].includes('disabled'), 'original explicit pause remains available');
      assert.ok(result.byLabel('运行诊断（只读，可选）'));
      if (status.startsWith('completed') && runtime?.state === 'running') {
        assert.match(plain(result.html), /控制记录已完成，但原消费者仍存活/);
        assert.match(plain(result.html), /继续查看原任务/);
      }
    }
  }
});

test('v4 R3 real Regular/SUPER card keeps view but disables remote-only mutators, not local-controlled remote execution', async () => {
  for (const controlSource of ['remote_only', 'local', undefined]) {
    const app = configure(createSSRApp({ render: () => h(card.default, { task: task({ status: 'ready', isRemote: true, controlSource }) }) }));
    app.component('n-button', { props: ['disabled'], setup: (props, { slots }) => () => h('button', { disabled: props.disabled }, [slots.icon?.(), slots.default?.()]) });
    const html = await renderToString(app);
    const buttons = [...html.matchAll(/<button([^>]*)>([\s\S]*?)<\/button>/g)];
    const byLabel = label => buttons.find(row => plain(row[2]).endsWith(label));
    assert.equal(byLabel('查看')[1].includes('disabled'), false);
    for (const label of ['生成list', '发送到远程', '启动', '暂停', '删除']) {
      assert.equal(byLabel(label)[1].includes('disabled'), controlSource === 'remote_only', label);
    }
    if (controlSource === 'remote_only') assert.match(plain(html), /仅远端登记.*原 owner/);
    assert.doesNotMatch(plain(html), /本控端登记|执行端可为本地或远程|登记来源未提供/);
  }
});
