import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { parse, compileScript, compileTemplate } from '@vue/compiler-sfc';
import * as Vue from 'vue';
import {
  createSubmissionStatsLoader,
  groupSubmissionStats,
} from '../src/utils/submissionStatsLoader.js';

const deferred = () => {
  let resolve;
  let reject;
  const promise = new Promise((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
};

const flush = () => new Promise((resolve) => setImmediate(resolve));

function makeDocs(prefix, months = ['2026-10', '2026-09'], countBase = 10) {
  const docs = [];
  months.forEach((month, idx) => {
    docs.push({
      _id: { month, region: 'USA', delay: 1 },
      count: countBase + idx,
      super_count: 1,
      avg_sharpe: 1.5 + idx * 0.1,
      avg_fitness: 1.1,
      avg_turnover: 0.2,
      avg_returns: 0.12,
      avg_margin: 0.0015,
      _label: `${prefix}-${month}`,
    });
  });
  return docs;
}

function createHarness(t) {
  const calls = [];
  const states = [];
  const invoke = (command, args) => {
    const d = deferred();
    calls.push({ command, args, ...d });
    return d.promise;
  };
  const loader = createSubmissionStatsLoader({
    invoke,
    onState: (state) => states.push(state),
  });
  t.after(() => loader.dispose());
  return {
    loader,
    calls,
    states,
    statsCalls: () => calls.filter((c) => c.command === 'get_submission_stats'),
    latest: () => states.at(-1),
  };
}

test('out-of-order collection switch: slower A cannot overwrite faster B', async (t) => {
  const h = createHarness(t);

  const pA = h.loader.load({ collection: 'alpha_results' });
  const pB = h.loader.load({ collection: 'alpha_submitted' });
  await flush();

  assert.equal(h.statsCalls().length, 2);
  assert.equal(h.statsCalls()[0].args.collection, 'alpha_results');
  assert.equal(h.statsCalls()[1].args.collection, 'alpha_submitted');

  const docsB = makeDocs('submitted', ['2026-11']);
  const docsA = makeDocs('results', ['2026-08', '2026-07'], 99);

  h.statsCalls()[1].resolve(docsB);
  await pB;
  assert.equal(h.latest().loading, false);
  assert.equal(h.latest().loadedParams.collection, 'alpha_submitted');
  assert.deepEqual(h.latest().rawResult, docsB);
  assert.equal(h.latest().statsData[0].month, '2026-11');

  h.statsCalls()[0].resolve(docsA);
  await pA;
  assert.equal(h.latest().loading, false);
  assert.equal(h.latest().loadedParams.collection, 'alpha_submitted');
  assert.deepEqual(h.latest().rawResult, docsB);
  assert.equal(h.latest().statsData.length, 1);
  assert.equal(h.latest().statsData[0].month, '2026-11');
});

test('rapid A -> B -> C: stale rejection and older resolution keep C loading until C settles', async (t) => {
  const h = createHarness(t);

  const pA = h.loader.load({ collection: 'alpha_results' });
  const pB = h.loader.load({ collection: 'alpha_submitted' });
  const pC = h.loader.load({ collection: 'alpha_submitted_zzz' });
  await flush();

  assert.equal(h.latest().loading, true);

  h.statsCalls()[0].reject(new Error('timeout A'));
  await pA;
  assert.equal(h.latest().loading, true);
  assert.equal(h.latest().error, null);

  h.statsCalls()[1].resolve(makeDocs('B', ['2026-09']));
  await pB;
  assert.equal(h.latest().loading, true);
  assert.deepEqual(h.latest().rawResult, []);

  const docsC = makeDocs('C', ['2026-11']);
  h.statsCalls()[2].resolve(docsC);
  await pC;
  assert.equal(h.latest().loading, false);
  assert.equal(h.latest().error, null);
  assert.equal(h.latest().loadedParams.collection, 'alpha_submitted_zzz');
  assert.deepEqual(h.latest().rawResult, docsC);
});

test('switching collection clears previous data immediately, while force refresh keeps rows visible', async (t) => {
  const h = createHarness(t);
  const docsA = makeDocs('A', ['2026-10']);

  const p1 = h.loader.load({ collection: 'alpha_results' });
  await flush();
  h.statsCalls()[0].resolve(docsA);
  await p1;
  assert.deepEqual(h.latest().rawResult, docsA);

  // Forced refresh on same collection preserves visible rows while loading=true.
  const pRefresh = h.loader.load({ collection: 'alpha_results' }, { force: true });
  await flush();
  assert.equal(h.statsCalls().length, 2);
  assert.equal(h.latest().loading, true);
  assert.deepEqual(h.latest().rawResult, docsA);

  // Switching to another collection immediately clears stale A rows before response.
  const pSwitch = h.loader.load({ collection: 'alpha_submitted' });
  await flush();
  assert.equal(h.latest().loading, true);
  assert.deepEqual(h.latest().rawResult, []);
  assert.deepEqual(h.latest().statsData, []);
  assert.equal(h.latest().loadedParams, null);

  const docsB = makeDocs('B', ['2026-11']);
  h.statsCalls()[2].resolve(docsB);
  await pSwitch;
  h.statsCalls()[1].resolve(docsA);
  await pRefresh;

  assert.equal(h.latest().loadedParams.collection, 'alpha_submitted');
  assert.deepEqual(h.latest().rawResult, docsB);
});

test('rapid toggle A -> B -> A coalesces in-flight A request and adopts it on latest generation', async (t) => {
  const h = createHarness(t);

  const pA1 = h.loader.load({ collection: 'alpha_results' }, { force: false });
  const pB = h.loader.load({ collection: 'alpha_submitted' }, { force: false });
  const pA2 = h.loader.load({ collection: 'alpha_results' }, { force: false });
  await flush();

  assert.equal(h.statsCalls().length, 2);

  h.statsCalls()[1].resolve(makeDocs('B', ['2026-01']));
  await pB;
  assert.equal(h.latest().loading, true);

  const docsA = makeDocs('A', ['2026-11']);
  h.statsCalls()[0].resolve(docsA);
  await Promise.all([pA1, pA2]);

  assert.equal(h.latest().loading, false);
  assert.equal(h.latest().loadedParams.collection, 'alpha_results');
  assert.deepEqual(h.latest().rawResult, docsA);
});

test('failure does not poison cache and subsequent load retries cleanly', async (t) => {
  const h = createHarness(t);

  const p1 = h.loader.load({ collection: 'alpha_results' });
  await flush();
  h.statsCalls()[0].reject(new Error('network error'));
  await p1;

  assert.equal(h.latest().loading, false);
  assert.match(String(h.latest().error), /network error/);

  const p2 = h.loader.load({ collection: 'alpha_results' }, { force: false });
  await flush();
  assert.equal(h.statsCalls().length, 2);
  const docs = makeDocs('A', ['2026-11']);
  h.statsCalls()[1].resolve(docs);
  await p2;

  assert.equal(h.latest().loading, false);
  assert.equal(h.latest().error, null);
  assert.deepEqual(h.latest().rawResult, docs);
});

test('dispose prevents late resolution and rejection from emitting state', async (t) => {
  const h = createHarness(t);

  const p1 = h.loader.load({ collection: 'alpha_results' });
  await flush();
  const countBeforeDispose = h.states.length;

  h.loader.dispose();
  h.statsCalls()[0].resolve(makeDocs('A'));
  await p1;
  assert.equal(h.states.length, countBeforeDispose);

  const pAfter = h.loader.load({ collection: 'alpha_submitted' });
  assert.equal(pAfter, null);
  assert.equal(h.statsCalls().length, 1);
});

test('groupSubmissionStats groups, averages weighted metrics, and sorts cleanly', () => {
  const grouped = groupSubmissionStats([
    {
      _id: { month: '2026-09', region: 'EUR', delay: 1 },
      count: 2,
      super_count: 1,
      avg_sharpe: 2.0,
      avg_fitness: 1.0,
      avg_turnover: 0.1,
      avg_returns: 0.2,
      avg_margin: 0.002,
    },
    {
      _id: { month: '2026-10', region: 'USA', delay: 1 },
      count: 1,
      super_count: 0,
      avg_sharpe: 1.5,
      avg_fitness: 1.2,
      avg_turnover: 0.3,
      avg_returns: 0.1,
      avg_margin: 0.001,
    },
    {
      _id: { month: '2026-10', region: 'USA', delay: 0 },
      count: 3,
      super_count: 2,
      avg_sharpe: 2.5,
      avg_fitness: 1.6,
      avg_turnover: 0.1,
      avg_returns: 0.3,
      avg_margin: 0.003,
    },
  ]);

  assert.equal(grouped.length, 2);
  assert.equal(grouped[0].month, '2026-10');
  assert.equal(grouped[0].count, 4);
  assert.equal(grouped[0].super_count, 2);
  assert.ok(Math.abs(grouped[0].avg_sharpe - (1.5 + 7.5) / 4) < 1e-9);
  assert.deepEqual(
    grouped[0].children.map((c) => `${c.region}-${c.delay}`),
    ['USA-0', 'USA-1'],
  );
  assert.equal(grouped[1].month, '2026-09');
});

function mountStatsPanel(t) {
  const calls = [];
  const invoke = (command, args) => {
    const d = deferred();
    calls.push({ command, args, ...d });
    return d.promise;
  };

  const source = readFileSync(
    new URL('../src/views/SubmissionStatsPanel.vue', import.meta.url),
    'utf8',
  );
  const { descriptor, errors } = parse(source);
  assert.deepEqual(errors, []);
  const template = compileTemplate({
    source: descriptor.template.content,
    filename: 'SubmissionStatsPanel.vue',
    id: 'stats-panel-test',
  });
  assert.deepEqual(template.errors, []);

  const script = compileScript(descriptor, { id: 'stats-panel-test' })
    .content.replace(/^import .*;\n/gm, '')
    .replace('export default', 'return');

  const configStore = Vue.reactive({
    dataFilterOptions: [
      { label: 'alpha_results', value: 'alpha_results' },
      { label: 'alpha_submitted', value: 'alpha_submitted' },
      { label: 'alpha_submitted_zzz', value: 'alpha_submitted_zzz' },
    ],
  });
  const addedQueries = [];
  const queryStore = {
    getQueriesByType: () => [],
    addQuery: (type, q) => addedQueries.push({ type, q }),
    removeQuery: () => {},
  };

  let capturedLoadPNL = null;
  const dependencies = {
    ref: Vue.ref,
    onMounted: Vue.onMounted,
    onBeforeUnmount: Vue.onBeforeUnmount,
    watch: Vue.watch,
    h: Vue.h,
    computed: Vue.computed,
    invoke,
    NButton: {},
    NSpace: {},
    NSelect: {},
    NDataTable: {},
    NModal: {},
    NSlider: {},
    NText: {},
    NTag: {},
    NAutoComplete: {},
    useConfigStore: () => configStore,
    useQueryStore: () => queryStore,
    useAlphaTableColumns: ({ loadPNL }) => {
      capturedLoadPNL = loadPNL;
      return { columns: [] };
    },
    createSubmissionStatsLoader,
  };

  const component = new Function(...Object.keys(dependencies), script)(
    ...Object.values(dependencies),
  );
  let state;
  const origSetup = component.setup;
  component.setup = (props, context) => {
    state = origSetup(props, context);
    return state;
  };
  component.render = () => null;

  const renderer = Vue.createRenderer({
    createComment: () => ({}),
    insert() {},
    remove() {},
    parentNode: () => null,
    nextSibling: () => null,
    createElement: () => ({}),
    createText: () => ({}),
    setText() {},
    setElementText() {},
    patchProp() {},
  });

  const app = renderer.createApp(component);
  app.mount({});
  t.after(() => app.unmount());

  return {
    calls,
    state,
    addedQueries,
    loadPNL: (...args) => capturedLoadPNL(...args),
    statsCalls: () => calls.filter((c) => c.command === 'get_submission_stats'),
    alphaCalls: () => calls.filter((c) => c.command === 'get_alpha_results'),
    pnlCalls: () => calls.filter((c) => c.command === 'get_pnl_by_id'),
    unmount: () => app.unmount(),
  };
}

test('actual SFC: switching collection during initial slow load ignores stale response', async (t) => {
  const h = mountStatsPanel(t);
  await flush();

  assert.equal(h.statsCalls().length, 1);
  assert.equal(h.statsCalls()[0].args.collection, 'alpha_results');
  assert.equal(h.state.loading.value, true);

  const pSwitch = h.state.handleCollectionChange('alpha_submitted');
  await flush();
  assert.equal(h.statsCalls().length, 2);
  assert.equal(h.statsCalls()[1].args.collection, 'alpha_submitted');

  const submittedDocs = makeDocs('sub', ['2026-11', '2026-10'], 3);
  h.statsCalls()[1].resolve(submittedDocs);
  await pSwitch;

  assert.equal(h.state.loading.value, false);
  assert.equal(h.state.loadedParams.value.collection, 'alpha_submitted');
  assert.equal(h.state.statsData.value.length, 2);
  assert.equal(h.state.totalStatsRow.value.count, 7);

  // Stale initial request finally finishes — must not touch current state.
  h.statsCalls()[0].resolve(makeDocs('old', ['2026-05'], 500));
  await flush();

  assert.equal(h.state.loading.value, false);
  assert.equal(h.state.loadedParams.value.collection, 'alpha_submitted');
  assert.deepEqual(h.state.rawStatsResult.value, submittedDocs);
  assert.equal(h.state.totalStatsRow.value.count, 7);
});

test('actual SFC: switching collection resets windowStartIndex to 0 for shorter datasets', async (t) => {
  const h = mountStatsPanel(t);
  await flush();

  const tenMonths = Array.from({ length: 10 }, (_, i) => `2026-${String(10 - i).padStart(2, '0')}`);
  h.statsCalls()[0].resolve(makeDocs('long', tenMonths, 5));
  await flush();

  h.state.windowStartIndex.value = 5;
  assert.equal(h.state.selectedMonthsNames.value.length, 3);

  const pSwitch = h.state.handleCollectionChange('alpha_submitted');
  await flush();
  assert.equal(h.state.windowStartIndex.value, 0);

  h.statsCalls()[1].resolve(makeDocs('short', ['2026-11', '2026-10', '2026-09'], 2));
  await pSwitch;

  assert.equal(h.state.windowStartIndex.value, 0);
  assert.deepEqual(h.state.selectedMonthsNames.value, ['2026-11', '2026-10', '2026-09']);
  assert.equal(h.state.windowStatsRow.value.count, 9);
});

test('actual SFC: drill-down viewAlphaList & loadPNL guard against modal switch and collection switch races', async (t) => {
  const h = mountStatsPanel(t);
  await flush();
  h.statsCalls()[0].resolve(makeDocs('res', ['2026-10', '2026-09'], 5));
  await flush();

  // Open modal for 2026-10, then immediately open modal for 2026-09 before first resolves.
  const pModal1 = h.state.viewAlphaList('2026-10', 'USA', 1, 'REGULAR');
  const pModal2 = h.state.viewAlphaList('2026-09', 'USA', 1, 'REGULAR');
  await flush();

  assert.equal(h.alphaCalls().length, 2);
  assert.equal(h.state.alphaLoading.value, true);

  h.alphaCalls()[0].resolve({ data: [{ id: 'stale-alpha' }] });
  await pModal1;
  assert.equal(h.state.alphaLoading.value, true);
  assert.deepEqual(h.state.alphaListData.value, []);

  h.alphaCalls()[1].resolve({ data: [{ id: 'shared-alpha-id' }] });
  await pModal2;
  assert.equal(h.state.alphaLoading.value, false);
  assert.deepEqual(h.state.alphaListData.value, [{ id: 'shared-alpha-id' }]);

  // Start loading PnL in alpha_results, then switch collection to alpha_submitted.
  const pPnlOld = h.loadPNL('shared-alpha-id');
  await flush();
  assert.equal(h.pnlCalls().length, 1);
  assert.equal(h.pnlCalls()[0].args.query.collection, 'alpha_results');

  const pSwitch = h.state.handleCollectionChange('alpha_submitted');
  await Vue.nextTick();
  await flush();

  // Collection switch closed modal and invalidated old PnL request.
  assert.equal(h.state.showAlphaList.value, false);
  assert.deepEqual(h.state.pnlDataMap.value, {});
  assert.equal(h.state.loadingSet.value.size, 0);

  h.pnlCalls()[0].resolve({ pnl_series: [1, 2, 3] });
  await pPnlOld;
  assert.deepEqual(h.state.pnlDataMap.value, {});

  h.statsCalls()[1].resolve(makeDocs('sub', ['2026-11'], 2));
  await pSwitch;

  // Opening modal in alpha_submitted allows fresh PnL load for the same alpha ID.
  const pModalSub = h.state.viewAlphaList('2026-11', 'USA', 1, 'REGULAR');
  await flush();
  h.alphaCalls()[2].resolve({ data: [{ id: 'shared-alpha-id' }] });
  await pModalSub;
  assert.equal(h.alphaCalls()[2].args.params.collection, 'alpha_submitted');

  const pPnlNew = h.loadPNL('shared-alpha-id');
  await flush();
  assert.equal(h.pnlCalls().length, 2);
  assert.equal(h.pnlCalls()[1].args.query.collection, 'alpha_submitted');
  h.pnlCalls()[1].resolve({ pnl_series: [9, 10] });
  await pPnlNew;
  assert.deepEqual(h.state.pnlDataMap.value['shared-alpha-id'], [9, 10]);
});

test('actual SFC: unmount discards late stats, drill-down, and PnL responses without mutating state', async (t) => {
  const h = mountStatsPanel(t);
  await flush();

  const pModal = h.state.viewAlphaList('2026-10', 'USA', 1, 'REGULAR');
  const pPnl = h.loadPNL('alpha-1');
  await flush();

  h.unmount();

  h.statsCalls()[0].resolve(makeDocs('after-unmount', ['2026-11'], 99));
  h.alphaCalls()[0].resolve({ data: [{ id: 'alpha-1' }] });
  h.pnlCalls()[0].resolve({ pnl_series: [42] });
  await Promise.all([pModal, pPnl]);

  assert.deepEqual(h.state.rawStatsResult.value, []);
  assert.deepEqual(h.state.alphaListData.value, []);
  assert.deepEqual(h.state.pnlDataMap.value, {});
});
