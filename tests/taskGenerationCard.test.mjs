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
import { createTaskFetcher } from '../src/utils/taskProgress.js';

// Execute the original SFC, store and latest-only list fetch. Only the host
// renderer / visual primitives are substitutes; no native, DB or network I/O.
const require = createRequire(import.meta.url);
const vue = pathToFileURL(require.resolve('vue/dist/vue.runtime.esm-bundler.js')).href;
const source = await readFile(new URL('../src/components/TaskCards/RegularTaskCard.vue', import.meta.url), 'utf8');
const compiled = compileScript(parse(source).descriptor, { id: 'generation-card', inlineTemplate: true }).content
  .replaceAll("'../../utils/taskDiagnostics.js'", JSON.stringify(new URL('../src/utils/taskDiagnostics.js', import.meta.url).href))
  .replaceAll("'../../stores/taskStore.js'", JSON.stringify(new URL('../src/stores/taskStore.js', import.meta.url).href))
  .replaceAll("'../../utils/taskGeneration.js'", JSON.stringify(new URL('../src/utils/taskGeneration.js', import.meta.url).href))
  .replaceAll('from "vue"', `from ${JSON.stringify(vue)}`)
  .replaceAll("from 'vue'", `from ${JSON.stringify(vue)}`);
const card = (await import(`data:text/javascript;base64,${Buffer.from(compiled).toString('base64')}`)).default;
const task = (extra = {}) => ({ _id: { $oid: '0123456789abcdef01234567' }, name: 'generation-fixture',
  taskType: 'regular', controlSource: 'local', isRemote: false, status: 'pending', ...extra });
const failed = (extra = {}) => task({ generation: { id: 'attempt-original', phase: 'generation_failed',
  finishedAt: '2026-10-31T00:00:00Z' }, lastError: 'writer exited 17\noriginal cause: invalid template', ...extra });
const plain = html => html.replace(/<[^>]*>/g, ' ').replace(/\s+/g, ' ').trim();
function configure(app) {
  app.config.warnHandler = () => {};
  for (const name of ['n-card', 'n-tag', 'n-text', 'n-space', 'n-switch']) {
    app.component(name, { setup: (_, { slots }) => () => h('div', [slots.header?.(), slots.default?.()]) });
  }
  app.component('n-button', { inheritAttrs: false, setup: (_, { attrs, slots }) =>
    () => h('button', attrs, [slots.icon?.(), slots.default?.()]) });
  return app;
}
async function render(props) {
  return renderToString(configure(createSSRApp({ render: () => h(card, props) })));
}
function mount(render) {
  const node = (type, text = '') => ({ type, text, children: [], parent: null, props: {} });
  const renderer = createRenderer({
    createElement: type => node(type), createText: text => node('text', text), createComment: () => node('comment'),
    setText: (el, text) => { el.text = text; },
    setElementText: (el, text) => { el.text = text; el.children = []; },
    patchProp: (el, key, _old, value) => { el.props[key] = value; },
    parentNode: el => el.parent,
    nextSibling: el => el.parent?.children[el.parent.children.indexOf(el) + 1] ?? null,
    insert(el, parent, anchor = null) {
      if (el.parent) el.parent.children.splice(el.parent.children.indexOf(el), 1);
      el.parent = parent;
      const index = anchor ? parent.children.indexOf(anchor) : -1;
      parent.children.splice(index < 0 ? parent.children.length : index, 0, el);
    },
    remove(el) { if (el.parent) el.parent.children.splice(el.parent.children.indexOf(el), 1); el.parent = null; },
  });
  const root = node('root');
  const app = configure(renderer.createApp({ render }));
  app.mount(root);
  const text = el => [el.text, ...el.children.map(text)].join(' ').replace(/\s+/g, ' ').trim();
  const nodes = el => [el, ...el.children.flatMap(nodes)];
  const button = label => {
    const found = nodes(root).find(el => el.type === 'button' && text(el).includes(label));
    assert.ok(found, `missing button ${label}`);
    return found;
  };
  return { text: () => text(root), button, stop: () => app.unmount(),
    click(label) { const el = button(label); if (!el.props.disabled) el.props.onClick?.(); } };
}

for (const taskType of ['regular', 'super']) test(`${taskType}: actual card keeps pending badge and persistent separate original generation error`, async () => {
  const value = failed({ taskType });
  const before = JSON.stringify(value);
  for (let mount = 0; mount < 2; mount++) { // full remount, no prior local error/toast
    const html = await render({ task: JSON.parse(JSON.stringify(value)),
      progress: { stale: true, lastError: 'progress-only failure', total: 3, error: 0, success: 0 } });
    assert.match(plain(html), /generation-fixture pending/);
    assert.match(plain(html), /生成失败（generation_failed）/);
    assert.match(plain(html), /writer exited 17 original cause: invalid template/);
    assert.match(plain(html), /进度刷新失败：progress-only failure/);
    assert.match(plain(html), /✗ 0\+0/); // operation failure is not an item error
    assert.doesNotMatch(plain(html), /列表生成完成/);
  }
  assert.equal(JSON.stringify(value), before);
});

test('real reactive card retries only on click, hides previous error during new attempt and after success', async () => {
  const props = reactive({ task: failed(), generationBusy: false });
  const events = [];
  const view = mount(() => h(card, { ...props, onGenerateList: () => events.push('generate') }));
  try {
    assert.match(view.text(), /生成失败（generation_failed）/);
    assert.equal(view.button('重新生成').props.disabled, false);
    assert.deepEqual(events, []);
    view.click('重新生成'); assert.deepEqual(events, ['generate']);
    props.generationBusy = true; await nextTick();
    assert.equal(view.button('生成中').props.disabled, true);
    assert.doesNotMatch(view.text(), /generation_failed|writer exited/);
    view.click('生成中'); assert.equal(events.length, 1);
    props.task = failed({ status: 'generating', generation: { id: 'attempt-new', phase: 'writing' } });
    props.generationBusy = false; await nextTick();
    assert.equal(view.button('生成中').props.disabled, true);
    assert.doesNotMatch(view.text(), /generation_failed|writer exited/);
    props.task = failed({ status: 'ready', generation: { id: 'attempt-new', phase: 'complete' } });
    await nextTick();
    assert.match(view.text(), /generation-fixture ready/);
    assert.doesNotMatch(view.text(), /generation_failed|writer exited/);
    assert.equal(view.button('生成list').props.disabled, false);
    assert.equal(events.length, 1);
  } finally { view.stop(); }
});

test('legacy failure stays visible, unknown remains distinct/disabled and no arbitrary lastError creates failure', async () => {
  const props = reactive({ task: task({ status: 'generation_failed', lastError: 'legacy cause' }) });
  const events = [];
  const view = mount(() => h(card, { ...props, onGenerateList: () => events.push('generate') }));
  try {
    assert.match(view.text(), /生成失败（generation_failed）.*legacy cause/);
    assert.equal(view.button('重新生成').props.disabled, false);
    props.task = failed({ status: 'generation_unknown', generation: { id: 'uncertain', phase: 'generation_unknown' } });
    await nextTick();
    assert.match(view.text(), /生成结果核验缺少最终回执/);
    assert.doesNotMatch(view.text(), /生成失败（generation_failed）/);
    assert.equal(view.button('生成状态待核验').props.disabled, true);
    view.click('生成状态待核验'); assert.deepEqual(events, []);
    for (const status of ['pending', 'ready']) {
      props.task = task({ status, lastError: 'old unrelated cause' }); await nextTick();
      assert.doesNotMatch(view.text(), /generation_failed|old unrelated cause/);
    }
    props.task = failed({ controlSource: 'remote_only' }); await nextTick();
    assert.equal(view.button('重新生成').props.disabled, true); // original control-source protection
  } finally { view.stop(); }
});

test('original latest-only fetch -> real Pinia store -> mounted card preserves refreshed failure and rejects stale list', async () => {
  setActivePinia(createPinia());
  const store = useTaskStore(); store.replaceTasks([task()]);
  const requests = [], errors = [], events = [];
  const fetch = createTaskFetcher({ load: () => new Promise((resolve, reject) => requests.push({ resolve, reject })),
    replace: rows => store.replaceTasks(rows), refresh: () => {}, isActive: () => true,
    onError: err => errors.push(err), onRefreshError: assert.fail });
  const view = mount(() => h(card, { task: store.tasks[0], onGenerateList: () => events.push('generate') }));
  try {
    const stale = fetch(), current = fetch();
    requests[1].resolve([failed()]); await current; await nextTick();
    assert.equal(store.tasks[0].status, 'pending');
    assert.match(view.text(), /生成失败（generation_failed）/);
    requests[0].resolve([task({ status: 'generating' })]); await stale; await nextTick();
    assert.match(view.text(), /original cause: invalid template/);
    const failingRead = fetch(); requests[2].reject(new Error('list read unavailable'));
    await failingRead; await nextTick();
    assert.equal(errors.length, 1);
    assert.equal(store.tasks[0].generation.phase, 'generation_failed');
    assert.equal(view.button('重新生成').props.disabled, false);
    assert.deepEqual(events, []); // refresh/failure never invokes generate
    const replacement = fetch(); requests[3].resolve([task({ _id: 'replacement-id' })]);
    await replacement; await nextTick();
    assert.doesNotMatch(view.text(), /generation_failed|writer exited/);
  } finally { view.stop(); }
});

test('actual Vue text interpolation escapes generation error markup without rewriting its text', async () => {
  const html = await render({ task: failed({ lastError: '<img src=x onerror="bad()"> & original' }) });
  assert.match(html, /&lt;img src=x onerror=&quot;bad\(\)&quot;&gt; &amp; original/);
  assert.doesNotMatch(html, /<img/);
});


test('actual TaskManager generate handler refreshes persisted failure, retries once per click, never rewrites status', async () => {
  const managerSource = await readFile(new URL('../src/views/TaskManager.vue', import.meta.url), 'utf8');
  const code = compileScript(parse(managerSource).descriptor, { id: 'generation-manager' }).content
    .replace('ref, onMounted, onUnmounted, watch, computed', 'ref, watch, computed')
    .replaceAll('from "vue"', `from ${JSON.stringify(vue)}`)
    .replaceAll('from "pinia"', `from ${JSON.stringify(import.meta.resolve('pinia'))}`)
    .replace('"../stores/taskStore"', JSON.stringify(new URL('../src/stores/taskStore.js', import.meta.url).href))
    .replace('"../utils/taskProgress.js"', JSON.stringify(new URL('../src/utils/taskProgress.js', import.meta.url).href))
    .replaceAll("'../utils/taskDiagnostics.js'", JSON.stringify(new URL('../src/utils/taskDiagnostics.js', import.meta.url).href))
    .replace("'../utils/taskGeneration.js'", JSON.stringify(new URL('../src/utils/taskGeneration.js', import.meta.url).href))
    .replace(/import \{ invoke \} from "@tauri-apps\/api\/core";/, 'const invoke = (...args) => hooks.invoke(...args);')
    .replace(/import \{ listen \} from "@tauri-apps\/api\/event";/, 'const listen = async () => () => {};')
    .replace(/import \{ useRoute, useRouter \} from "vue-router";/, 'const useRoute = () => ({ fullPath: "/tasks", query: {} }); const useRouter = () => ({});')
    .replace(/import \{ useMessage, useDialog \} from "naive-ui";/,
      'const useMessage = () => ({ error: value => hooks.errors.push(value), success: value => hooks.success.push(value), warning: value => hooks.warnings.push(value) }); const useDialog = () => ({});')
    .replace(/import (\w+) from "[^"\n]+\.vue";/g, 'const $1 = {};');
  const hooksCode = 'export const hooks = { invoke: null, errors: [], success: [], warnings: [] }; const onMounted = () => {}; const onUnmounted = fn => { hooks.unmount = fn; };';
  const module = await import(`data:text/javascript;base64,${Buffer.from(hooksCode + code).toString('base64')}`);
  setActivePinia(createPinia());
  const store = useTaskStore(); store.replaceTasks([task()]);
  const calls = [], commands = [], reads = [];
  module.hooks.invoke = (command, args) => {
    calls.push({ command, args });
    if (command === 'generate_list') return new Promise((resolve, reject) => commands.push({ resolve, reject }));
    if (command === 'get_all_tasks') return new Promise(resolve => reads.push(resolve));
    if (command === 'refresh_task_progress') return Promise.resolve();
    assert.fail('unexpected native operation: ' + command);
  };
  const state = module.default.setup({}, { expose() {} });
  const id = task()._id.$oid;
  const view = mount(() => h(card, { task: store.tasks[0], generationBusy: !!state.generationRequests.value[id] }));
  const flush = async () => { for (let i = 0; i < 12; i++) await Promise.resolve(); await nextTick(); };
  try {
    const first = state.generateList(task()._id);
    await state.generateList(id); // in-flight duplicate ignored by original handler
    assert.equal(commands.length, 1);
    await nextTick();
    assert.equal(view.button('生成中').props.disabled, true);
    assert.equal(store.tasks[0].status, 'pending'); // no optimistic status write
    commands[0].reject(new Error('original writer exited 17'));
    await flush();
    assert.equal(reads.length, 1);
    assert.equal(store.tasks[0].status, 'pending');
    reads[0]([failed()]); await first; await nextTick();
    assert.equal(store.tasks[0].status, 'pending');
    assert.match(view.text(), /生成失败（generation_failed）.*original cause: invalid template/);
    assert.equal(view.button('重新生成').props.disabled, false);
    assert.match(module.hooks.errors[0], /original writer exited 17/);
    assert.equal(module.hooks.success.length, 0);
    assert.equal(commands.length, 1);
    const second = state.generateList(id); await nextTick();
    assert.equal(commands.length, 2);
    assert.doesNotMatch(view.text(), /generation_failed|writer exited/);
    commands[1].resolve(); await flush();
    reads[1]([failed({ status: 'ready', generation: { id: 'new', phase: 'complete' } })]);
    await second; await nextTick();
    assert.equal(store.tasks[0].status, 'ready');
    assert.doesNotMatch(view.text(), /generation_failed|writer exited/);
    assert.equal(module.hooks.success.length, 1);
    store.replaceTasks([failed({ status: 'generation_unknown', generation: { id: 'unknown', phase: 'generation_unknown' } })]);
    await state.generateList(id);
    assert.equal(commands.length, 2);
    assert.equal(calls.filter(c => c.command === 'get_all_tasks').length, 2);
    assert.equal(calls.filter(c => c.command === 'refresh_task_progress').length, 2);
  } finally { view.stop(); module.hooks.unmount(); }
});
