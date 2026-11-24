/**
 * Pure grouping helper & race-free request coordinator for SubmissionStatsPanel.
 */

export function groupSubmissionStats(docs) {
  const list = Array.isArray(docs) ? docs : [];
  const monthGroups = {};

  for (const doc of list) {
    const id = doc?._id || {};
    const month = id.month || 'Unknown';
    if (!monthGroups[month]) {
      monthGroups[month] = {
        month,
        count: 0,
        super_count: 0,
        avg_sharpe: 0,
        avg_fitness: 0,
        avg_turnover: 0,
        avg_returns: 0,
        avg_margin: 0,
        children: [],
      };
    }

    const region = id.region;
    const delay = id.delay;
    const docCount = doc?.count || 0;
    const regionData = {
      key: `${month}-${region}-${delay}`,
      month,
      region,
      delay,
      count: docCount,
      super_count: doc?.super_count || 0,
      avg_sharpe: doc?.avg_sharpe,
      avg_fitness: doc?.avg_fitness,
      avg_turnover: doc?.avg_turnover,
      avg_returns: doc?.avg_returns,
      avg_margin: doc?.avg_margin,
    };

    monthGroups[month].children.push(regionData);
    monthGroups[month].count += docCount;
    monthGroups[month].super_count += doc?.super_count || 0;
    monthGroups[month].avg_sharpe += (doc?.avg_sharpe || 0) * docCount;
    monthGroups[month].avg_fitness += (doc?.avg_fitness || 0) * docCount;
    monthGroups[month].avg_turnover += (doc?.avg_turnover || 0) * docCount;
    monthGroups[month].avg_returns += (doc?.avg_returns || 0) * docCount;
    monthGroups[month].avg_margin += (doc?.avg_margin || 0) * docCount;
  }

  return Object.values(monthGroups)
    .map((m) => {
      if (m.count > 0) {
        m.avg_sharpe /= m.count;
        m.avg_fitness /= m.count;
        m.avg_turnover /= m.count;
        m.avg_returns /= m.count;
        m.avg_margin /= m.count;
      }
      m.children.sort((a, b) => {
        const regionComp = String(a.region || '').localeCompare(String(b.region || ''));
        if (regionComp !== 0) return regionComp;
        return (a.delay ?? 0) - (b.delay ?? 0);
      });
      return {
        ...m,
        key: m.month,
      };
    })
    .sort((a, b) => String(b.month).localeCompare(String(a.month)));
}

function normalizeParams({ collection, query } = {}) {
  const normalizedCollection = collection || 'alpha_results';
  const trimmed = typeof query === 'string' ? query.trim() : '';
  return {
    collection: normalizedCollection,
    query: trimmed || null,
  };
}

const keyOf = (params) => JSON.stringify([params.collection, params.query]);

export function createSubmissionStatsLoader({ invoke, onState }) {
  let active = true;
  let generation = 0;
  let current = null;
  let committedKey = null;
  let state = {
    rawResult: [],
    statsData: [],
    loading: false,
    error: null,
    params: null,
    loadedParams: null,
    generation: 0,
  };
  const inFlight = new Map();

  function emit() {
    if (!active) return;
    onState({
      ...state,
      params: state.params ? { ...state.params } : null,
      loadedParams: state.loadedParams ? { ...state.loadedParams } : null,
    });
  }

  function load(rawParams, { force = false } = {}) {
    if (!active) return null;
    const params = normalizeParams(rawParams);
    const requestKey = keyOf(params);

    if (!force && current?.requestKey === requestKey && inFlight.has(requestKey)) {
      return inFlight.get(requestKey);
    }

    const version = ++generation;
    current = { params, requestKey };
    const scopeChanged = committedKey !== requestKey;

    state = {
      rawResult: scopeChanged ? [] : state.rawResult,
      statsData: scopeChanged ? [] : state.statsData,
      loading: true,
      error: null,
      params: { ...params },
      loadedParams: scopeChanged ? null : state.loadedParams,
      generation: version,
    };
    emit();

    let promise = !force ? inFlight.get(requestKey) : null;
    if (!promise) {
      promise = Promise.resolve().then(() => {
        if (!active) throw new Error('Submission stats loader disposed');
        return invoke('get_submission_stats', {
          collection: params.collection,
          query: params.query,
        });
      });
      inFlight.set(requestKey, promise);
      const cleanup = () => {
        if (inFlight.get(requestKey) === promise) inFlight.delete(requestKey);
      };
      promise.then(cleanup, cleanup);
    }

    return promise
      .then((result) => {
        if (!active || version !== generation) return;
        const rawResult = Array.isArray(result) ? result : [];
        committedKey = requestKey;
        state = {
          rawResult,
          statsData: groupSubmissionStats(rawResult),
          loading: false,
          error: null,
          params: { ...params },
          loadedParams: { ...params },
          generation: version,
        };
        emit();
      })
      .catch((error) => {
        if (!active || version !== generation) return;
        state = {
          ...state,
          loading: false,
          error,
          generation: version,
        };
        emit();
      });
  }

  function dispose() {
    active = false;
    generation++;
    current = null;
    inFlight.clear();
  }

  return {
    load,
    dispose,
    getGeneration: () => generation,
  };
}
