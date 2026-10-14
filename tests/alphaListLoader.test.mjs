import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { parse, compileScript, compileTemplate } from '@vue/compiler-sfc';
import * as Vue from 'vue';
import {
  createAlphaListLoader, alphaCountKey, provisionalItemCount,
} from '../src/utils/alphaListLoader.js';

const deferred = () => {
  let resolve, reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
};
const flush = () => new Promise((resolve) => setImmediate(resolve));
const params = (overrides = {}) => ({
  collection: 'alpha_results', alpha_type: 'REGULAR', region: 'USA',
  universe: 'TOP3000', page: 1, page_size: 20, embedding: 'first',
  sort_field: null, sort_order: null, ...overrides,
});
const pageResult = (page = 1, length = 20, total = 0, pageSize = 20) => ({
  data: Array.from({ length }, (_, n) => ({ id: `p${page}-${n}` })),
  total, page, page_size: pageSize,
});
function clock() {
  let time = 0, nextId = 1;
  const timers = new Map();
  return {
    now: () => time,
    setTimer: (fn, ms) => { const id = nextId++; timers.set(id, { fn, at: time + ms }); return id; },
    clearTimer: (id) => timers.delete(id),
    advance(ms) {
      time += ms;
      for (const [id, entry] of [...timers]) {
        if (entry.at <= time) { timers.delete(id); entry.fn(); }
      }
    },
    get size() { return timers.size; },
  };
}
function harness(t, options = {}) {
  const calls = [], states = [], time = clock();
  const invoke = (command, args) => {
    const d = deferred();
    calls.push({ command, args, ...d });
    return d.promise;
  };
  const loader = createAlphaListLoader({ invoke, onState: (state) => states.push(state), ...time, ...options });
  t.after(() => loader.dispose());
  return {
    loader, calls, states, time, invoke,
    rows: () => calls.filter((c) => c.command === 'get_alpha_results'),
    counts: () => calls.filter((c) => c.command === 'get_alpha_count'),
    latest: () => states.at(-1),
  };
}

test('non-RA rows arrive before exact count; fake total is ignored and next page works', async (t) => {
  const h = harness(t);
  h.loader.schedule(params(), { immediate: true });
  await flush();
  assert.equal(h.rows()[0].args.params.include_total, false);
  h.rows()[0].resolve(pageResult());
  await flush();
  assert.equal(h.latest().rows.length, 20);
  assert.equal(h.latest().loading, false);
  assert.equal(h.latest().total, null);
  assert.equal(h.latest().countStatus, 'pending');
  assert.equal(provisionalItemCount(1, 20, h.latest().rows), 21);
  h.counts()[0].resolve(103);
  await flush();
  assert.equal(h.latest().total, 103);
  assert.equal(h.latest().countStatus, 'exact');
  assert.equal(h.latest().rows.length, 20);
  assert.equal(provisionalItemCount(2, 20, [1, 2]), 22);
});

test('count coalesces across pagination, sorting and embedding; cached counts are reused', async (t) => {
  const h = harness(t);
  for (const p of [params(), params({ page: 2 }), params({ page: 3, sort_field: 'Sharpe', sort_order: -1 }), params({ embedding: 'second' })]) {
    h.loader.schedule(p, { immediate: true });
    await flush();
  }
  assert.equal(h.rows().length, 4);
  assert.equal(h.counts().length, 1);
  assert.equal('embedding' in h.counts()[0].args.params, false);
  assert.equal('page' in h.counts()[0].args.params, false);
  h.counts()[0].resolve(95);
  await flush();
  assert.equal(h.latest().total, 95);
  h.loader.schedule(params({ page: 4 }), { immediate: true });
  await flush();
  assert.equal(h.latest().total, 95);
  assert.equal(h.counts().length, 1);
});

test('rapid pages do not cancel filter-scoped total and stale pages cannot overwrite newest', async (t) => {
  const h = harness(t);
  for (let page = 1; page <= 3; page++) {
    h.loader.schedule(params({ page }), { immediate: true });
    await flush();
  }
  h.counts()[0].resolve(200);
  h.rows()[2].resolve(pageResult(3));
  await flush();
  h.rows()[1].reject(new Error('old page error'));
  h.rows()[0].resolve(pageResult(1));
  await flush();
  assert.equal(h.latest().page, 3);
  assert.equal(h.latest().rows[0].id, 'p3-0');
  assert.equal(h.latest().total, 200);
  assert.equal(h.latest().error, null);
});

test('count TTL starts at completion, expires at 30s, with bounded LRU cache', async (t) => {
  const h = harness(t, { maxCountEntries: 2 });
  h.loader.schedule(params(), { immediate: true });
  await flush();
  h.time.advance(5000);
  h.counts()[0].resolve(40);
  h.rows()[0].resolve(pageResult());
  await flush();
  h.time.advance(29999);
  h.loader.schedule(params({ page: 2 }), { immediate: true });
  await flush();
  assert.equal(h.counts().length, 1);
  assert.equal(h.latest().total, 40);
  h.time.advance(1);
  h.loader.schedule(params({ page: 3 }), { immediate: true });
  await flush();
  assert.equal(h.counts().length, 2);
  assert.equal(h.latest().total, null);
  h.counts()[1].resolve(41);
  await flush();
  for (const region of ['EUR', 'ASI']) {
    h.loader.schedule(params({ region }), { immediate: true });
    await flush();
    h.counts().at(-1).resolve(7);
    await flush();
  }
  h.loader.schedule(params({ page: 4 }), { immediate: true });
  await flush();
  assert.equal(h.counts().length, 5, 'USA entry was evicted, not persisted');
});

test('cached count expiring during debounce is not displayed as exact when rows start', async (t) => {
  const h = harness(t);
  h.loader.schedule(params(), { immediate: true });
  await flush();
  h.counts()[0].resolve(9);
  h.rows()[0].resolve(pageResult(1, 9));
  await flush();
  h.time.advance(29900);
  h.loader.schedule(params({ embedding: 'second' }));
  h.time.advance(300);
  await flush();
  assert.equal(h.counts().length, 2);
  assert.equal(h.latest().countStatus, 'pending');
  assert.equal(h.latest().total, null);
});

test('count failure leaves usable rows, never zero, and retries on the next request', async (t) => {
  const h = harness(t);
  h.loader.schedule(params(), { immediate: true });
  await flush();
  h.rows()[0].resolve(pageResult());
  h.counts()[0].reject(new Error('count unavailable'));
  await flush();
  assert.equal(h.latest().rows.length, 20);
  assert.equal(h.latest().total, null);
  assert.equal(h.latest().countStatus, 'unavailable');
  h.loader.schedule(params({ page: 2 }), { immediate: true });
  await flush();
  assert.equal(h.counts().length, 2);
  h.counts()[1].resolve(0); // Real zero is still a valid exact result.
  await flush();
  assert.equal(h.latest().total, 0);
  assert.equal(h.latest().countStatus, 'exact');
});

test('all semantic filters (including future ones) enter count key; only presentation fields are excluded', () => {
  const base = params({ query: 'abc', messages_in: ['PASS'] });
  const excluded = { page: 9, page_size: 100, sort_field: 'Fitness', sort_order: 1, embedding: 'different', include_total: false };
  assert.equal(alphaCountKey(base), alphaCountKey({ ...base, ...excluded }));
  for (const [key, value] of Object.entries({
    collection: 'other', alpha_type: 'SUPER', region: 'EUR', universe: 'TOP1000',
    query: 'xyz', exclude_query: 'volume', id: 'abc', messages_in: ['FAIL'],
    messages_nin: ['FAIL'], classifications_in: ['POWER_POOL'], delay: 0, days_within: 7,
    min_turnover: 0.1, max_turnover: 0.5, min_margin: 0.002, min_returns: 0.1,
    self_categories: ['MODEL'], future_filter: { min: 5, max: 10 },
  })) assert.notEqual(alphaCountKey(base), alphaCountKey({ ...base, [key]: value }), key);
  assert.equal(alphaCountKey({ region: 'USA', custom: { b: 2, a: 1 } }), alphaCountKey({ custom: { a: 1, b: 2 }, region: 'USA' }));
});

test('different collection/type/time/region scopes issue distinct exact counts', async (t) => {
  const h = harness(t);
  for (const p of [params(), params({ collection: 'other' }), params({ alpha_type: 'SUPER' }), params({ days_within: 7 }), params({ region: 'ASI' })]) {
    h.loader.schedule(p, { immediate: true });
    await flush();
  }
  assert.equal(h.counts().length, 5);
  h.counts()[0].resolve(999);
  await flush();
  assert.equal(h.latest().total, null);
  h.counts()[4].resolve(2);
  await flush();
  assert.equal(h.latest().total, 2);
});

test('RA and RA_PARENT retain facet totals/corrected pages; never omit or call separate count', async (t) => {
  const h = harness(t);
  for (const alpha_type of ['RA', 'RA_PARENT']) {
    h.loader.schedule(params({ alpha_type, page: 8, include_total: false }), { immediate: true });
    await flush();
    assert.equal('include_total' in h.rows().at(-1).args.params, false);
    h.rows().at(-1).resolve(pageResult(2, 1, 21));
    await flush();
    assert.equal(h.latest().total, 21);
    assert.equal(h.latest().page, 2);
    assert.equal(h.latest().countStatus, 'exact');
  }
  assert.equal(h.counts().length, 0);
});

test('new filter invalidates old rows/errors/counts immediately, before debounce fires', async (t) => {
  const h = harness(t);
  h.loader.schedule(params(), { immediate: true });
  await flush();
  h.loader.schedule(params({ region: 'EUR' }));
  const emissions = h.states.length;
  h.rows()[0].resolve(pageResult());
  h.counts()[0].reject(new Error('old filter count error'));
  await flush();
  assert.equal(h.states.length, emissions);
  assert.equal(h.latest().rows.length, 0);
  assert.equal(h.latest().total, null);
  h.time.advance(300);
  await flush();
  h.loader.schedule(params({ region: 'ASI' }));
  const nextEmissions = h.states.length;
  h.rows()[1].reject(new Error('old filter row error'));
  h.counts()[1].resolve(400);
  await flush();
  assert.equal(h.states.length, nextEmissions);
  assert.equal(h.latest().error, null);
  assert.equal(h.latest().total, null);
});

test('snapshot is immutable from caller mutations, including nested filter arrays', async (t) => {
  const h = harness(t);
  const p = params({ messages_in: ['PASS'], future: { time: 7 } });
  h.loader.schedule(p);
  p.messages_in.push('FAIL');
  p.future.time = 8;
  p.region = 'EUR';
  h.time.advance(300);
  await flush();
  assert.deepEqual(h.rows()[0].args.params.messages_in, ['PASS']);
  assert.deepEqual(h.counts()[0].args.params.future, { time: 7 });
  assert.equal(h.rows()[0].args.params.region, 'USA');
});

test('duplicate schedules and same-key in-flight pages coalesce; immediate page cancels pending timer', async (t) => {
  const h = harness(t);
  h.loader.schedule(params());
  h.loader.schedule(params());
  assert.equal(h.time.size, 1);
  h.time.advance(299);
  await flush();
  assert.equal(h.calls.length, 0);
  h.time.advance(1);
  await flush();
  h.loader.schedule(params());
  h.loader.schedule(params(), { immediate: true });
  assert.equal(h.time.size, 0);
  await flush();
  assert.equal(h.rows().length, 1);
  h.loader.schedule(params({ region: 'EUR' }));
  h.loader.schedule(params({ region: 'EUR', page: 2 }), { immediate: true });
  assert.equal(h.time.size, 0);
  await flush();
  h.time.advance(300);
  await flush();
  assert.equal(h.rows().length, 2);
  assert.equal(h.rows()[1].args.params.page, 2);
});

test('returning to same in-flight page adopts its response under newest generation', async (t) => {
  const h = harness(t);
  h.loader.schedule(params(), { immediate: true });
  await flush();
  h.loader.schedule(params({ page: 2 }), { immediate: true });
  await flush();
  h.loader.schedule(params(), { immediate: true });
  await flush();
  assert.equal(h.rows().length, 2);
  h.rows()[0].resolve(pageResult());
  h.counts()[0].resolve(80);
  await flush();
  assert.equal(h.latest().rows.length, 20);
  assert.equal(h.latest().page, 1);
  assert.equal(h.latest().total, 80);
});

test('dispose cancels timer and rejects in-flight rows, count and errors without emissions', async (t) => {
  const h = harness(t);
  h.loader.schedule(params(), { immediate: true });
  await flush();
  h.loader.schedule(params({ region: 'EUR' }));
  h.loader.dispose();
  assert.equal(h.time.size, 0);
  const emissions = h.states.length;
  h.rows()[0].reject(new Error('after unmount'));
  h.counts()[0].resolve(100);
  h.time.advance(300);
  h.loader.schedule(params(), { immediate: true });
  await flush();
  assert.equal(h.states.length, emissions);
  assert.equal(h.calls.length, 2);
});

// Compile/mount the actual script with Vue's in-memory renderer, no DOM or IPC service.
function mountPanel(t, embeddingOptions = [{ value: 'first' }]) {
  const h = harness(t);
  const source = readFileSync(new URL('../src/views/AlphaTablePanel.vue', import.meta.url), 'utf8');
  const { descriptor, errors } = parse(source);
  assert.deepEqual(errors, []);
  const template = compileTemplate({ source: descriptor.template.content, filename: 'AlphaTablePanel.vue', id: 'panel-test' });
  assert.deepEqual(template.errors, []);
  const script = compileScript(descriptor, { id: 'panel-test' }).content
    .replace(/^import .*;\n/gm, '').replace('export default', 'return');
  const config = Vue.reactive({ embeddingOptions });
  const dependencies = {
    ref: Vue.ref, watch: Vue.watch, onMounted: Vue.onMounted,
    onBeforeUnmount: Vue.onBeforeUnmount, reactive: Vue.reactive,
    nextTick: Vue.nextTick, computed: Vue.computed,
    useMessage: () => ({ info() {}, success() {} }), invoke: h.invoke,
    DataFilters: {}, useAlphaTableColumns: () => ({ columns: [] }),
    createAlphaListLoader: (args) => createAlphaListLoader({ ...args, ...h.time }),
    provisionalItemCount, useConfigStore: () => config, useAnalysisStore: () => ({}),
  };
  const component = new Function(...Object.keys(dependencies), script)(...Object.values(dependencies));
  let state;
  const setup = component.setup;
  component.setup = (props, context) => { state = setup(props, context); return state; };
  component.render = () => null;
  const renderer = Vue.createRenderer({
    createComment: () => ({}), insert() {}, remove() {}, parentNode: () => null,
    nextSibling: () => null, createElement: () => ({}), createText: () => ({}),
    setText() {}, setElementText() {}, patchProp() {},
  });
  const app = renderer.createApp(component);
  app.mount({});
  t.after(() => app.unmount());
  return { ...h, state, config, unmount: () => app.unmount() };
}

test('actual panel: one initial path with ready embedding, 300ms debounce, RA page reset, unmount disposal', async (t) => {
  const h = mountPanel(t);
  assert.equal(h.time.size, 1);
  h.time.advance(300);
  await flush();
  assert.equal(h.rows().length, 1);
  assert.equal(h.counts().length, 1);
  assert.equal(h.rows()[0].args.params.embedding, 'first');
  h.state.pagination.value.page = 7;
  h.state.filters.type = 'RA';
  assert.equal(h.state.pagination.value.page, 1);
  h.rows()[0].resolve(pageResult());
  h.counts()[0].resolve(200);
  await flush();
  assert.equal(h.state.data.value.length, 0);
  assert.equal(h.state.totalLabel.value, '总数统计中');
  h.unmount();
  h.time.advance(300);
  await flush();
  assert.equal(h.rows().length, 1);
});

test('actual panel: delayed embedding coalesces mount timer rather than explicit duplicate fetch', async (t) => {
  const h = mountPanel(t, []);
  h.config.embeddingOptions = [{ value: 'late' }];
  await Vue.nextTick();
  assert.equal(h.time.size, 1);
  h.time.advance(300);
  await flush();
  assert.equal(h.rows().length, 1);
  assert.equal(h.rows()[0].args.params.embedding, 'late');
});

test('actual panel: count updates do not overwrite enriched rows; stale correlation/PNL cannot mutate scope', async (t) => {
  const h = mountPanel(t);
  h.time.advance(300);
  await flush();
  h.rows()[0].resolve(pageResult(1, 1));
  await flush();
  const corr = h.state.calculateCorr();
  h.calls.find((c) => c.command === 'compute_correlation').resolve({ 'p1-0': { ppac_correlation: '0.2', os_correlation: '0.3' } });
  await corr;
  assert.equal(h.state.data.value[0].corr_os, '0.3');
  assert.equal(h.state.corrLoading.value, false);
  h.counts()[0].resolve(1);
  await flush();
  assert.equal(h.state.data.value[0].corr_os, '0.3');
  assert.equal(h.state.totalLabel.value, '共 1 条');
  const oldCorr = h.state.calculateCorr();
  const oldPNL = h.state.loadPNL('p1-0');
  h.state.filters.region = 'EUR';
  h.calls.filter((c) => c.command === 'compute_correlation').at(-1).resolve({ 'p1-0': { os_correlation: '0.9' } });
  h.calls.find((c) => c.command === 'get_pnl_by_id').resolve({ pnl_series: [9] });
  await Promise.all([oldCorr, oldPNL]);
  assert.deepEqual(h.state.data.value, []);
  assert.deepEqual(h.state.pnlDataMap.value, {});
  assert.equal(h.state.loadingSet.value.size, 0);
  h.state.onFiltersUpdate({ ...h.state.filters, collection: 'other', messages: ['FAIL'] });
  assert.deepEqual(h.state.filters.messages, []);
});

test('disposal before queued IPC starts prevents any external calls', async (t) => {
  const h = harness(t);
  h.loader.schedule(params(), { immediate: true });
  h.loader.dispose();
  await flush();
  assert.equal(h.calls.length, 0);
});

test('actual panel: loading/error/count-unavailable labels, provisional navigation, delay zero and immediate actions', async (t) => {
  const h = mountPanel(t);
  h.state.filters.delay = 0;
  h.time.advance(300);
  await flush();
  assert.equal(h.rows()[0].args.params.delay, 0);
  assert.equal(h.counts()[0].args.params.delay, 0);
  assert.equal(h.state.rowsLoading.value, true);
  h.rows()[0].reject(new Error('row service unavailable'));
  h.counts()[0].reject(new Error('count service unavailable'));
  await flush();
  assert.equal(h.state.rowsLoading.value, false);
  assert.match(h.state.rowError.value, /row service unavailable/);
  assert.equal(h.state.totalLabel.value, '总数暂不可用');
  h.state.fetchData();
  await flush();
  assert.equal(h.state.rowError.value, '');
  h.rows()[1].resolve(pageResult());
  await flush();
  assert.equal(h.state.pagination.value.itemCount, 21);
  assert.equal(h.state.totalLabel.value, '总数统计中');
  h.state.filters.embedding = 'second';
  h.state.pagination.value.page = 2;
  h.state.fetchData();
  assert.equal(h.time.size, 0);
  await flush();
  assert.equal(h.rows().length, 3);
  assert.equal(h.rows()[2].args.params.page, 2);
  assert.equal(h.counts().length, 2);
  h.state.handlePageSizeChange(50);
  await flush();
  assert.equal(h.rows()[3].args.params.page_size, 50);
  assert.equal(h.rows()[3].args.params.page, 1);
  assert.equal(h.counts().length, 2);
});

test('actual panel: unmounted correlation/PNL successes cannot mutate rows/maps', async (t) => {
  const h = mountPanel(t);
  h.time.advance(300);
  await flush();
  h.rows()[0].resolve(pageResult(1, 1));
  await flush();
  const corr = h.state.calculateCorr();
  const pnl = h.state.loadPNL('p1-0');
  h.unmount();
  h.calls.find((c) => c.command === 'compute_correlation').resolve({ 'p1-0': { os_correlation: '0.9' } });
  h.calls.find((c) => c.command === 'get_pnl_by_id').resolve({ pnl_series: [9] });
  await Promise.all([corr, pnl]);
  assert.equal(h.state.data.value[0].corr_os, undefined);
  assert.deepEqual(h.state.pnlDataMap.value, {});
});

test('actual panel: multi-field Object.assign advances sync generations but emits only one final debounced query', async (t) => {
  const h = mountPanel(t);
  const generation = h.state.listLoader.getGeneration();
  h.state.pagination.value.page = 6;
  h.state.onFiltersUpdate({
    ...h.state.filters, collection: 'other', embedding: 'second', region: 'EUR',
    universe: 'TOP1000', type: 'SUPER', delay: 0, days: 7,
    searchQuery: '  close  ', messagesIn: ['PASS'], minTurnover: 10,
  });
  assert.ok(h.state.listLoader.getGeneration() > generation + 1);
  assert.equal(h.state.pagination.value.page, 1);
  assert.equal(h.time.size, 1);
  h.time.advance(299);
  await flush();
  assert.equal(h.calls.length, 0);
  h.time.advance(1);
  await flush();
  assert.equal(h.rows().length, 1);
  assert.equal(h.counts().length, 1);
  const rowParams = h.rows()[0].args.params;
  const expected = {
    collection: 'other', embedding: 'second', region: 'EUR', universe: 'TOP1000',
    alpha_type: 'SUPER', delay: 0, days_within: 7, query: 'close',
    messages_in: ['PASS'], min_turnover: 0.1, page: 1,
  };
  for (const [key, value] of Object.entries(expected)) assert.deepEqual(rowParams[key], value, key);
  assert.equal(alphaCountKey(rowParams), alphaCountKey(h.counts()[0].args.params));
});
