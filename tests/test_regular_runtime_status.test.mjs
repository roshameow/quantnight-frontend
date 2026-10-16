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
  .replaceAll("'../../stores/taskStore.js'", JSON.stringify(storeModule))
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
  assert.match(text, /进度刷新失败，保留上次统计并重试/);
  assert.match(text, /最大并发: 3.*子任务上限: 10/);
  assert.match(text, /glb_spread/);
  assert.match(text, /user1/);
}

test('real regular SFC SSR: initial running control is pending; fresh positive, bash-only and unknown are distinct', async () => {
  for (const [runtime, label] of [[null, '运行状态待确认'], [observation(), '消费者存活（派发状态未确认）'],
    [observation({ state: 'not_running', raw: '❌ Pane 0 (pid=21733) is running: bash' }), '未检测到运行消费者'],
    [observation({ state: 'unknown' }), '运行状态未确认'], [observation({ state: 'checking' }), '运行状态未确认'],
    [observation({ state: 'paused' }), '运行状态未确认']]) {
    const input = { task: task(), progress, taskRuntimeStatus: runtime };
    const before = JSON.stringify(input);
    const html = await render(input);
    assert.match(plain(html), new RegExp(`glb_spread ${label}`));
    assertPreserved(html);
    assert.equal(JSON.stringify(input), before);
    if (label !== '消费者存活（派发状态未确认）') assert.doesNotMatch(plain(html), /glb_spread 消费者存活（派发状态未确认）/);
    assert.doesNotMatch(plain(html), /glb_spread running/);
    assert.doesNotMatch(plain(html), /paused|任务完成|启动成功/);
  }
});

test('freshness and identity boundary rejects stale, future, invalid and wrong task/host observations', async () => {
  const now = Date.now();
  for (const patch of [{ taskId: 'old-id' }, { name: 'other' }, { isRemote: true }, { isRemote: 'false' },
    { observedAt: now - card.REGULAR_RUNTIME_MAX_AGE }, { observedAt: now + 60000 },
    { observedAt: -1 }, { observedAt: null }, { observedAt: NaN }, { observedAt: '123' }]) {
    const runtime = observation(patch);
    assert.equal(card.regularDisplayStatus(task(), runtime, null, now), '运行状态未确认');
    const html = await render({ task: task(), progress, taskRuntimeStatus: runtime });
    assert.match(plain(html), /glb_spread 运行状态未确认/);
    assertPreserved(html);
  }
  assert.equal(card.regularDisplayStatus(task(), observation({ observedAt: now - card.REGULAR_RUNTIME_MAX_AGE + 1 }), null, now), '消费者存活（派发状态未确认）');
});

test('actual regular SFC SSR honors stop lifecycle and persisted pause proof above positive or missing consumer evidence', async () => {
  for (const runtime of [null, observation(), observation({ state: 'not_running' }), observation({ state: 'unknown' })]) {
    for (const [status, label] of [['stopping', 'stopping'], ['stop_unconfirmed', '停止状态未确认'], ['paused', 'paused']]) {
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
      assert.match(plain(await render({ task: input, taskRuntimeStatus: runtime })), /glb_spread 暂停（运行状态未确认）/);
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
    assert.match(mounted.text(), /glb_spread 运行状态待确认/);
    for (const [runtime, label] of [[observation(), '消费者存活（派发状态未确认）'],
      [observation({ state: 'not_running' }), '未检测到运行消费者'],
      [observation({ state: 'unknown' }), '运行状态未确认'], [observation(), '消费者存活（派发状态未确认）']]) {
      props.taskRuntimeStatus = runtime;
      await nextTick();
      assert.match(mounted.text(), new RegExp(`glb_spread ${label}`));
      assertPreserved(mounted.text());
    }
    props.task = task({ _id: 'replacement' }); await nextTick();
    assert.match(mounted.text(), /glb_spread 运行状态未确认/);
    props.task = task({ isRemote: true }); await nextTick();
    assert.match(mounted.text(), /glb_spread 运行状态未确认/);
    props.task = task();
    props.stopOperation = { _id: 'regular-id', name: 'glb_spread', isRemote: false, status: 'stop_unconfirmed' };
    await nextTick();
    props.taskRuntimeStatus = observation(); await nextTick();
    assert.match(mounted.text(), /glb_spread 停止状态未确认/);
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
    .replace(/import \{ invoke \} from "@tauri-apps\/api\/core";/, 'const invoke = () => { throw new Error("SSR must not invoke native"); };')
    .replace(/import \{ listen \} from "@tauri-apps\/api\/event";/, 'const listen = async () => () => {};')
    .replace(/import \{ useRoute, useRouter \} from "vue-router";/, 'const useRoute = () => ({ fullPath: "/tasks" }); const useRouter = () => ({});')
    .replace(/import \{ useMessage, useDialog \} from "naive-ui";/, 'const useMessage = () => ({}); const useDialog = () => ({});')
    .replace(/import RegularTaskCard from "[^"\n]+\.vue";/, `import RegularTaskCard from ${JSON.stringify(cardURL)};`)
    .replace(/import (\w+) from "[^"\n]+\.vue";/g, 'const $1 = { render: () => null };');
  const module = await import(`data:text/javascript;base64,${Buffer.from(manager).toString('base64')}`);
  const renderManager = () => renderToString(configure(createSSRApp(module.default)));
  assert.match(plain(await renderManager()), /glb_spread 运行状态待确认/);
  const operation = store.beginTaskStop(task());
  assert.match(plain(await renderManager()), /glb_spread stopping/);
  store.markTaskStopUnconfirmed(task(), operation, 'native timeout');
  const html = await renderManager();
  assert.match(plain(html), /glb_spread 停止状态未确认/);
  assert.match(plain(html), /✓ 7\+4.*✗ 2\+1.*Σ 80\+20/);
});


test('mounted positive badge expires without any new props or native polling and cleans up its one-shot timer', async (t) => {
  t.mock.timers.enable({ apis: ['Date', 'setTimeout'], now: 1800000000000 });
  const props = reactive({ task: task(), taskRuntimeStatus: observation(), progress });
  const mounted = memoryMount(props);
  try {
    assert.match(mounted.text(), /glb_spread 消费者存活（派发状态未确认）/);
    t.mock.timers.tick(card.REGULAR_RUNTIME_MAX_AGE - 1); await nextTick();
    assert.match(mounted.text(), /glb_spread 消费者存活（派发状态未确认）/);
    t.mock.timers.tick(1); await nextTick();
    assert.match(mounted.text(), /glb_spread 运行状态未确认/);
    assertPreserved(mounted.text());
    props.taskRuntimeStatus = observation(); await nextTick();
    assert.match(mounted.text(), /glb_spread 消费者存活（派发状态未确认）/);
  } finally { mounted.stop(); t.mock.timers.reset(); }
});
