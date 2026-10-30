import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { pathToFileURL } from 'node:url';
import { parse, compileScript } from '@vue/compiler-sfc';
import { createSSRApp, h } from 'vue';
import { renderToString } from '@vue/server-renderer';
import { createPinia, setActivePinia } from 'pinia';
import { useTaskStore } from '../src/stores/taskStore.js';

// Same fixture is evaluated by real core status and watcher aggregation/DTO tests.
// Node consumes the expected wire shape, not a second status classifier. Native
// execution is required separately; this test does not pretend to run Rust.
const cases = JSON.parse(await readFile(new URL('./fixtures/item-status-counts.json', import.meta.url)));
const require = createRequire(import.meta.url);
const vue = pathToFileURL(require.resolve('vue/dist/vue.runtime.esm-bundler.js')).href;
const source = await readFile(process.env.QUANTNIGHT_COUNT_CARD_SOURCE ?? new URL('../src/components/TaskCards/RegularTaskCard.vue', import.meta.url), 'utf8');
const code = compileScript(parse(source).descriptor, { id: 'item-count-contract', inlineTemplate: true }).content
  .replaceAll("'../../utils/taskDiagnostics.js'", JSON.stringify(new URL('../src/utils/taskDiagnostics.js', import.meta.url).href))
  .replaceAll("'../../stores/taskStore.js'", JSON.stringify(new URL('../src/stores/taskStore.js', import.meta.url).href))
  .replaceAll("'../../utils/taskGeneration.js'", JSON.stringify(new URL('../src/utils/taskGeneration.js', import.meta.url).href))
  .replaceAll('from "vue"', `from ${JSON.stringify(vue)}`)
  .replaceAll("from 'vue'", `from ${JSON.stringify(vue)}`);
const card = (await import(`data:text/javascript;base64,${Buffer.from(code).toString('base64')}`)).default;
const task = (taskType = 'regular', isRemote = false) => ({ _id: 'id', name: 'counts', taskType, isRemote, status: 'running' });
const storeFor = value => {
  setActivePinia(createPinia());
  const store = useTaskStore();
  store.replaceTasks([value]);
  return store;
};
const event = (extra = {}) => ({ collection: 'counts', task_id: 'id', is_remote: false,
  success: 0, error: 0, cancelled: 0, total: 1,
  priority_success: 0, priority_error: 0, priority_cancelled: 0, priority_total: 0,
  updated_at: 1, ...extra });
const stub = { setup: (_, { slots }) => () => h('div', [slots.header?.(), slots.default?.()]) };
async function render(task, progress) {
  const app = createSSRApp({ render: () => h(card, { task, progress }) });
  app.config.warnHandler = () => {};
  for (const name of ['n-card', 'n-tag', 'n-text', 'n-space', 'n-switch', 'n-button']) app.component(name, stub);
  return renderToString(app);
}

for (const kind of ['regular', 'super']) for (const remote of [false, true]) {
  test(`${kind} remote=${remote}: shared status table -> original store -> actual card`, async () => {
    const target = task(kind, remote);
    const store = storeFor(target);
    let version = 0;
    for (const row of cases) for (const priority of [false, true]) {
      const payload = event({ is_remote: remote, updated_at: ++version,
        success: row.success, error: row.error, cancelled: row.cancelled,
        priority_total: Number(priority), priority_success: priority ? row.success : 0,
        priority_error: priority ? row.error : 0, priority_cancelled: priority ? row.cancelled : 0 });
      assert.equal(store.updateProgress(payload), true);
      const progress = store.taskProgressMap.counts;
      for (const key of ['success', 'error', 'cancelled', 'priority_cancelled']) assert.equal(progress[key], payload[key], `${row.status}: ${key}`);
      const html = await render(target, progress);
      const text = html.replace(/<[^>]*>/g, ' ').replace(/\s+/g, ' ');
      assert.ok(text.includes(`✗ ${priority ? 0 : row.error}+${priority ? row.error : 0}`), row.status);
      if (row.cancelled) {
        assert.ok(text.includes(`已取消 ${priority ? 0 : 1}+${priority ? 1 : 0}`));
        assert.match(html, /#b0bec5/); // neutral cancellation segment, not red error
        assert.match(text, /✗ 0\+0/);
      } else assert.doesNotMatch(text, /已取消/);
      assert.equal(store.tasks[0].status, 'running'); // counts never rewrite task lifecycle
    }
  });
}

test('additive cancellation projection preserves legacy DTOs, stale snapshots and host resets', () => {
  const store = storeFor(task());
  assert.equal(store.updateProgress(event({ cancelled: 1, priority_cancelled: 1 })), true);
  assert.equal(store.updateProgressError({ collection: 'counts', task_id: 'id', is_remote: false, error: 'offline', updated_at: 2 }), true);
  assert.equal(store.taskProgressMap.counts.cancelled, 1);
  assert.equal(store.taskProgressMap.counts.error, 0);
  assert.equal(store.taskProgressMap.counts.stale, true);
  assert.equal(store.updateProgress(event({ updated_at: 1, cancelled: 0 })), false);
  const legacy = event({ updated_at: 3 });
  delete legacy.cancelled; delete legacy.priority_cancelled;
  assert.equal(store.updateProgress(legacy), true);
  assert.equal(store.taskProgressMap.counts.cancelled, 0);
  assert.equal(store.taskProgressMap.counts.priority_cancelled, 0);
  store.updateProgress(event({ updated_at: 4, cancelled: 1 }));
  store.setTaskSource('counts', true);
  assert.equal(store.taskProgressMap.counts.cancelled, 0);
  assert.equal(store.taskProgressMap.counts.stale, true);
});

test('invalid optional counts do not corrupt the existing progress snapshot', () => {
  const store = storeFor(task());
  store.updateProgress(event({ cancelled: 1 }));
  for (const value of [-1, NaN, '1', Infinity]) for (const key of ['cancelled', 'priority_cancelled']) {
    assert.equal(store.updateProgress(event({ [key]: value, updated_at: 2 })), false);
    assert.equal(store.taskProgressMap.counts.cancelled, 1);
  }
});

test('mixed error, timeout and cancelled items remain separate in the rendered bar', async () => {
  const store = storeFor(task('super'));
  const totals = cases.reduce((sum, row) => ({ success: sum.success + row.success,
    error: sum.error + row.error, cancelled: sum.cancelled + row.cancelled }), { success: 0, error: 0, cancelled: 0 });
  store.updateProgress(event({ ...totals, total: cases.length }));
  const html = await render(store.tasks[0], store.taskProgressMap.counts);
  const text = html.replace(/<[^>]*>/g, ' ').replace(/\s+/g, ' ');
  assert.match(text, /✓ 1\+0.*✗ 4\+0.*已取消 1\+0.*Σ 14\+0/);
  assert.match(html, /不代表平台取消或表达式失败/);
  assert.equal(store.taskProgressMap.counts.error + store.taskProgressMap.counts.cancelled, 5);
});

// Structural guard complements (does not replace) the native execution tests:
// an independently copied table would pass value parity until the next drift.
test('single core semantic definition is called, never copied by the GUI', async () => {
  const read = path => readFile(new URL(path, import.meta.url), 'utf8');
  const core = await read('../crates/task-core/src/task_store.rs');
  const watcher = await read('../src-tauri/src/watcher.rs');
  const inspection = await read('../crates/task-core/src/inspection.rs');
  const statsBody = watcher.match(/fn stats_pipeline\(\) -> Vec<Document> \{([\s\S]*?)\n\}/)?.[1];
  assert.equal(statsBody?.trim(), 'quantnight_task_core::task_store::item_progress_pipeline()');
  const countsBody = core.match(/pub fn item_progress_counts\([\s\S]*?\n\}/)?.[0];
  const pipelineBody = core.match(/pub fn item_progress_pipeline\([\s\S]*?\n\}/)?.[0];
  assert.match(countsBody, /item_progress_counts_with_terminal\(statuses\.iter\(\)/);
  const rowsBody = core.match(/pub fn item_progress_counts_with_terminal[\s\S]*?\n\}/)?.[0];
  const expressionBody = core.match(/fn item_progress_bucket_expression\([\s\S]*?\n\}/)?.[0];
  assert.match(rowsBody, /ITEM_PROGRESS_RULES\.iter\(\)/);
  assert.match(expressionBody, /ITEM_PROGRESS_RULES\.iter\(\)/);
  assert.match(pipelineBody, /item_progress_bucket_expression\(\)/);
  for (const body of [rowsBody, expressionBody]) assert.doesNotMatch(body, /"CANCELLED"|"permanent_error"/);
  assert.match(pipelineBody, /in ITEM_PROGRESS_BUCKETS/);
  for (const body of [countsBody, pipelineBody]) assert.doesNotMatch(body, /"permanent_error"|"timeout_error"|"fail"/);
  const reconciliation = inspection.slice(inspection.indexOf('let reconciliation: Result<(), Error>'));
  assert.match(reconciliation, /let counts = crate::task_store::item_progress_counts\(&execution\.statuses\);/);
  assert.match(reconciliation, /let failures = counts\["error"\] \+ counts\["cancelled"\];/);
  assert.doesNotMatch(reconciliation, /"permanent_error"|"timeout_error"/);
});

const receiptCases = JSON.parse(await readFile(new URL('./fixtures/item-receipt-counts.json', import.meta.url)));
for (const kind of ['regular', 'super']) test(`${kind}: verified writer rows and historical projection -> store/card`, async () => {
  const target = task(kind);
  const store = storeFor(target);
  let version = 0;
  for (const item of receiptCases) {
    const before = JSON.stringify(item.row);
    // Expected projection values are checked against actual core/BSON execution
    // in native tests; this stage verifies their unchanged DTO consumer.
    store.updateProgress(event({ ...item.counts, updated_at: ++version }));
    const progress = store.taskProgressMap.counts;
    const html = await render(target, progress);
    const text = html.replace(/<[^>]*>/g, ' ').replace(/\s+/g, ' ');
    assert.ok(text.includes(`✓ ${item.counts.success}+0`), item.name);
    assert.ok(text.includes(`✗ ${item.counts.error}+0`), item.name);
    if (item.counts.cancelled) assert.match(text, /已取消 1\+0/);
    else assert.doesNotMatch(text, /已取消/);
    assert.equal(JSON.stringify(item.row), before);
    assert.equal(store.tasks[0].status, 'running');
  }
});
