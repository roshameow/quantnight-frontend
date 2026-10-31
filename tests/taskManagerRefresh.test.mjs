import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { pathToFileURL } from 'node:url';
import { parse, compileScript } from '@vue/compiler-sfc';
import { createRenderer, h, reactive, nextTick } from 'vue';
import { createPinia } from 'pinia';
import { useTaskStore } from '../src/stores/taskStore.js';

// Actual parent, toolbar, regular card, fetcher and Pinia. Only native I/O,
// routing and visual primitives are fake; no browser, DB, process or timers.
const require = createRequire(import.meta.url);
const vue = pathToFileURL(require.resolve('vue/dist/vue.runtime.esm-bundler.js')).href;
const pinia = import.meta.resolve('pinia');
const url = path => new URL(path, import.meta.url).href;
const moduleUrl = code => `data:text/javascript;base64,${Buffer.from(code).toString('base64')}`;
async function compiled(path) {
  return compileScript(parse(await readFile(new URL(path, import.meta.url), 'utf8')).descriptor,
    { id: path, inlineTemplate: true }).content
    .replaceAll('from "vue"', `from ${JSON.stringify(vue)}`)
    .replaceAll("from 'vue'", `from ${JSON.stringify(vue)}`);
}
const toolbarUrl = moduleUrl(await compiled('../src/components/TaskCards/TaskManagerToolbar.vue'));
const cardUrl = moduleUrl((await compiled('../src/components/TaskCards/RegularTaskCard.vue'))
  .replaceAll("'../../utils/taskDiagnostics.js'", JSON.stringify(url('../src/utils/taskDiagnostics.js')))
  .replaceAll("'../../stores/taskStore.js'", JSON.stringify(url('../src/stores/taskStore.js')))
  .replaceAll("'../../utils/taskGeneration.js'", JSON.stringify(url('../src/utils/taskGeneration.js'))));
const managerSource = await compiled('../src/views/TaskManager.vue');
const deferred = () => { let resolve, reject; const promise = new Promise((ok, fail) => { resolve = ok; reject = fail; }); return { promise, resolve, reject }; };
const tick = async () => { for (let i = 0; i < 12; i++) await Promise.resolve(); await nextTick(); };
const task = (extra = {}) => ({ _id: { $oid: '0123456789abcdef01234567' }, name: 'refresh-fixture',
  status: 'pending', taskType: 'regular', controlSource: 'local', isRemote: false, ...extra });
let serial = 0;
async function harness(rows = []) {
  const code = managerSource
    .replace('createTaskProgressLifecycle,', 'createTaskProgressLifecycle as realTaskProgressLifecycle,')
    .replaceAll('from "pinia"', `from ${JSON.stringify(pinia)}`)
    .replace('"../stores/taskStore"', JSON.stringify(url('../src/stores/taskStore.js')))
    .replace('"../utils/taskProgress.js"', JSON.stringify(url('../src/utils/taskProgress.js')))
    .replaceAll("'../utils/taskDiagnostics.js'", JSON.stringify(url('../src/utils/taskDiagnostics.js')))
    .replace("'../utils/taskGeneration.js'", JSON.stringify(url('../src/utils/taskGeneration.js')))
    .replace('import { invoke } from "@tauri-apps/api/core";', 'const invoke = (...args) => hooks.invoke(...args);')
    .replace('import { listen } from "@tauri-apps/api/event";', 'const listen = (...args) => hooks.listen(...args);')
    .replace('import { useRoute, useRouter } from "vue-router";', 'const useRoute = () => hooks.route; const useRouter = () => ({ push() {}, replace() {} });')
    .replace('import { useMessage, useDialog } from "naive-ui";', 'const useMessage = () => ({ error: error => hooks.errors.push(error), success: text => hooks.notices.push(text), warning: text => hooks.notices.push(text) }); const useDialog = () => ({});')
    .replace('"../components/TaskCards/TaskManagerToolbar.vue"', JSON.stringify(toolbarUrl))
    .replace('"../components/TaskCards/RegularTaskCard.vue"', JSON.stringify(cardUrl))
    .replace(/import (\w+) from "[^"\n]+\.vue";/g, 'const $1 = { render: () => null };')
    .replace('return (_ctx, _cache) => {', 'hooks.state = { fetchTasks, handleAddTask, showAddModal };\nreturn (_ctx, _cache) => {');
  const mod = await import(moduleUrl(`export const hooks = { errors: [], notices: [] };
    const setInterval = () => { throw new Error('unexpected interval'); };
    const setTimeout = () => { throw new Error('unexpected timer'); };
    const createTaskProgressLifecycle = options => realTaskProgressLifecycle({ ...options, setIntervalFn: setInterval });
    ${code}\n// fixture ${serial++}`));
  const hooks = mod.hooks, calls = [], reads = [], handlers = new Map(), cleanups = [], create = deferred();
  hooks.route = reactive({ fullPath: '/tasks', query: {} });
  hooks.listen = async (event, handler) => { handlers.set(event, handler); return () => { handlers.delete(event); cleanups.push(event); }; };
  hooks.invoke = (name, args) => {
    calls.push({ name, args });
    if (name === 'get_all_tasks') { const read = deferred(); reads.push(read); return read.promise; }
    if (name === 'frontend_ready') return Promise.resolve();
    if (name === 'refresh_task_progress') return hooks.progressFailure ? Promise.reject(hooks.progressFailure) : Promise.resolve();
    if (name === 'create_task') return create.promise;
    assert.fail(`unexpected native operation ${name}`);
  };
  const node = (type, text = '') => ({ type, text, children: [], parent: null, props: {} });
  const renderer = createRenderer({
    createElement: type => node(type), createText: text => node('text', text), createComment: () => node('comment'),
    setText: (el, text) => { el.text = text; }, setElementText: (el, text) => { el.text = text; el.children = []; },
    patchProp: (el, key, _old, value) => { el.props[key] = value; }, parentNode: el => el.parent,
    nextSibling: el => el.parent?.children[el.parent.children.indexOf(el) + 1] ?? null,
    insert(el, parent, anchor = null) {
      if (el.parent) el.parent.children.splice(el.parent.children.indexOf(el), 1);
      el.parent = parent; const index = anchor ? parent.children.indexOf(anchor) : -1;
      parent.children.splice(index < 0 ? parent.children.length : index, 0, el);
    },
    remove(el) { if (el.parent) el.parent.children.splice(el.parent.children.indexOf(el), 1); el.parent = null; },
  });
  const root = node('root'), app = renderer.createApp(mod.default), piniaInstance = createPinia();
  app.use(piniaInstance); const store = useTaskStore(piniaInstance); store.replaceTasks(rows);
  app.config.warnHandler = () => {};
  for (const name of ['n-page-header', 'n-collapse', 'n-collapse-item', 'n-grid', 'n-gi', 'n-card', 'n-tag', 'n-text', 'n-space', 'n-switch']) {
    app.component(name, { setup: (_, { slots }) => () => h('div', [slots.title?.(), slots.header?.(), slots.extra?.(), slots.default?.()]) });
  }
  app.component('n-modal', { render: () => null });
  app.component('n-empty', { setup: (_, { attrs }) => () => h('p', String(attrs.description)) });
  app.component('n-button', { inheritAttrs: false, setup: (_, { attrs, slots }) => () => h('button', attrs, slots.default?.()) });
  app.mount(root); await tick();
  const text = el => [el.text, ...el.children.map(text)].join(' ').replace(/\s+/g, ' ').trim();
  const nodes = el => [el, ...el.children.flatMap(nodes)];
  const button = label => {
    const found = nodes(root).find(el => el.type === 'button' &&
      [text(el), el.props['aria-label'], el.props.title].some(value => typeof value === 'string' && value.includes(label)));
    assert.ok(found, `missing ${label}`); return found;
  };
  return { hooks, calls, reads, handlers, cleanups, create, store, button, root,
    text: () => text(root), click(label) { const el = button(label); if (!el.props.disabled) el.props.onClick?.(); },
    async resolve(index, value) { reads[index].resolve(value); await tick(); }, stop() { app.unmount(); },
  };
}

test('actual initial/manual toolbar refresh shares fetcher, shows loading and leaves add controls usable', async () => {
  const h = await harness();
  try {
    assert.equal(h.reads.length, 1);
    assert.equal(h.button('刷新中').props.disabled, true);
    assert.doesNotMatch(h.text(), /暂无任务|暂无 Priority/);
    h.click('添加任务'); assert.equal(h.hooks.state.showAddModal.value, true);
    h.click('刷新中'); assert.equal(h.reads.length, 1);
    await h.resolve(0, [task()]);
    assert.equal(h.button('刷新任务').props.disabled, false);
    const original = h.hooks.state.fetchTasks;
    h.click('刷新任务'); assert.equal(h.reads.length, 2);
    assert.equal(h.hooks.state.fetchTasks, original);
    assert.match(h.text(), /refresh-fixture/);
    await h.resolve(1, [task({ status: 'ready' })]);
    assert.match(h.text(), /refresh-fixture ready/);
    assert.equal(h.calls.filter(c => c.name === 'refresh_task_progress').length, 2);
    assert.deepEqual([...new Set(h.calls.map(c => c.name))], ['frontend_ready', 'get_all_tasks', 'refresh_task_progress']);
  } finally { h.stop(); }
});

test('list failure preserves old cards and useful error until accepted replacement; no false empty initial result', async () => {
  const h = await harness([task()]);
  try {
    h.reads[0].reject(new Error('local fixture unavailable')); await tick();
    assert.match(h.text(), /刷新任务失败.*local fixture unavailable/);
    assert.match(h.text(), /refresh-fixture/);
    assert.equal(h.button('刷新任务').props.disabled, false);
    assert.match(String(h.hooks.errors[0]), /local fixture unavailable/);
    h.click('刷新任务'); assert.match(h.text(), /local fixture unavailable/);
    await h.resolve(1, []); assert.doesNotMatch(h.text(), /local fixture unavailable/);
    assert.match(h.text(), /暂无任务/);
  } finally { h.stop(); }
  const empty = await harness();
  try {
    empty.reads[0].reject('registry unavailable'); await tick();
    assert.match(empty.text(), /registry unavailable/);
    assert.doesNotMatch(empty.text(), /暂无任务|暂无 Priority/);
  } finally { empty.stop(); }
});

test('registry and route refresh supersede in-flight manual response, never suppress real events', async () => {
  const h = await harness();
  try {
    await h.resolve(0, [task()]); h.click('刷新任务');
    h.handlers.get('task-registry-changed')({ payload: { remote: false } });
    assert.equal(h.reads.length, 3);
    await h.resolve(1, [task({ status: 'ready' })]);
    assert.equal(h.button('刷新中').props.disabled, true);
    assert.equal(h.store.tasks[0].status, 'pending');
    await h.resolve(2, [task({ status: 'generating', generation: { id: 'g', phase: 'writing' } })]);
    assert.equal(h.store.tasks[0].status, 'generating');
    h.hooks.route.fullPath = '/tasks?taskId=fixture'; await tick();
    assert.equal(h.reads.length, 4);
    await h.resolve(3, [task({ status: 'pending', generation: { id: 'g', phase: 'generation_failed' }, lastError: 'original failure' })]);
    assert.match(h.text(), /original failure/);
    assert.equal(h.reads.length, 4); // replacement/progress ACK do not recurse
  } finally { h.stop(); }
});

test('mutation completion obtains a fresh list and invalidate releases loading without reusing obsolete promise', async () => {
  const h = await harness();
  try {
    await h.resolve(0, [task()]); h.click('刷新任务');
    h.hooks.state.fetchTasks.invalidate();
    await tick(); assert.equal(h.button('刷新任务').props.disabled, false);
    const creation = h.hooks.state.handleAddTask({ name: 'new-task' });
    h.create.resolve({}); await creation; await tick();
    assert.equal(h.reads.length, 3);
    await h.resolve(1, []); assert.equal(h.store.tasks.length, 1);
    await h.resolve(2, [task(), task({ _id: 'new-id', name: 'new-task' })]);
    assert.equal(h.store.tasks.length, 2);
  } finally { h.stop(); }
});

test('transient generation diagnostic is escaped text, not persisted outcome or an automatic retry', async () => {
  const h = await harness();
  try {
    const row = task({ status: 'generating', generation: { id: 'original', phase: 'writing' }, lastError: 'old error',
      generationRefreshError: '<img src=x onerror=unsafe> probe unavailable' });
    await h.resolve(0, [row]);
    assert.match(h.text(), /生成状态核验未完成.*probe unavailable/);
    assert.equal(h.store.tasks[0].lastError, 'old error');
    assert.deepEqual(h.store.tasks[0].generation, row.generation);
    const hasImage = node => node.type === 'img' || node.children.some(hasImage);
    assert.equal(hasImage(h.root), false);
    h.click('刷新任务'); await h.resolve(1, [task({ status: 'generation_unknown' })]);
    assert.doesNotMatch(h.text(), /probe unavailable/);
    assert.ok(!h.calls.some(c => /generate|start|check_task_status/.test(c.name)));
  } finally { h.stop(); }
});

test('late fetch after unmount cannot replace cards, toast, or enqueue progress', async () => {
  const h = await harness([task()]);
  h.stop();
  assert.ok(h.cleanups.includes('task-registry-changed'));
  const before = h.calls.length;
  await h.resolve(0, []);
  assert.equal(h.store.tasks.length, 1);
  assert.equal(h.calls.length, before);
  assert.deepEqual(h.hooks.errors, []);
});


test('progress refresh failure never undoes accepted list or leaves refresh button loading', async () => {
  const h = await harness();
  try {
    h.hooks.progressFailure = new Error('fixture background progress unavailable');
    await h.resolve(0, [task({ status: 'ready' })]);
    assert.equal(h.button('刷新任务').props.disabled, false);
    assert.equal(h.store.tasks[0].status, 'ready');
    assert.doesNotMatch(h.text(), /刷新任务失败/);
    assert.deepEqual(h.hooks.errors, []);
    assert.equal(h.calls.filter(call => call.name === 'get_all_tasks').length, 1);
  } finally { h.stop(); }
});
