// Counts describe semantic filters, not a page, ordering, or embedding join.
const COUNT_EXCLUDED = new Set([
  'page', 'page_size', 'sort_field', 'sort_order', 'embedding', 'include_total',
]);
const snapshot = (value) => JSON.parse(JSON.stringify(value));
const canonical = (value) => {
  if (Array.isArray(value)) return value.map(canonical);
  if (value && typeof value === 'object') {
    return Object.fromEntries(Object.keys(value).sort().map((key) => [key, canonical(value[key])]));
  }
  return value;
};
const keyOf = (value) => JSON.stringify(canonical(value));
const countParams = (params) => Object.fromEntries(
  Object.entries(params).filter(([key]) => !COUNT_EXCLUDED.has(key)),
);
export const alphaCountKey = (params) => keyOf(countParams(params));
export const isRaQuery = (params) => ['RA', 'RA_PARENT'].includes(params.alpha_type);

// This is a navigation bound, NEVER an exact total. A full page allows one next page.
export function provisionalItemCount(page, pageSize, rows, hasRows = true) {
  const offset = (page - 1) * pageSize;
  return offset + (hasRows ? rows.length + (rows.length === pageSize ? 1 : 0) : 1);
}

/** Per-panel, memory-only request coordinator. All dependencies are injectable. */
export function createAlphaListLoader({
  invoke,
  onState,
  now = Date.now,
  setTimer = setTimeout,
  clearTimer = clearTimeout,
  debounceMs = 300,
  countTtlMs = 30_000,
  maxCountEntries = 64,
}) {
  let active = true;
  let generation = 0;
  let current = null;
  let timer = null;
  let pendingKey = null;
  let state = null;
  const pagesInFlight = new Map();
  // One count slot across all filter keys. `current` is the only waiting request;
  // obsolete filters are never queued for later replay.
  let countInFlight = null;
  const countCache = new Map();

  function call(command, params) {
    return Promise.resolve().then(() => {
      if (!active) throw new Error('Alpha list loader disposed');
      return invoke(command, { params: snapshot(params) });
    });
  }
  function emit() {
    if (active) onState({ ...state, params: snapshot(current.params), generation });
  }
  function cachedCount(key) {
    const entry = countCache.get(key);
    if (!entry) return null;
    if (now() - entry.completedAt >= countTtlMs) {
      countCache.delete(key);
      return null;
    }
    // LRU, bounded even when filters change continuously.
    countCache.delete(key);
    countCache.set(key, entry);
    return entry;
  }
  function acceptsCount(key) {
    return active && current && !current.ra && current.countKey === key;
  }
  function ensureCount(request) {
    const { countKey, params } = request;
    if (!active || request !== current || request.ra || !request.countReady) return;
    // A shared count may have failed while this search page was still loading.
    // Its later row success must not silently retry; a new schedule resets status.
    if (state.countStatus === 'unavailable' || cachedCount(countKey) || countInFlight) return;
    if (acceptsCount(countKey)) {
      state.total = null;
      state.countStatus = 'pending';
      state.countError = null;
      emit();
    }
    const promise = call('get_alpha_count', countParams(params));
    countInFlight = promise;
    promise.then((total) => {
      if (!Number.isSafeInteger(total) || total < 0) throw new Error('Invalid alpha count');
      if (!active) return;
      countCache.set(countKey, { total, completedAt: now() });
      while (countCache.size > maxCountEntries) countCache.delete(countCache.keys().next().value);
      // Filter-scoped, not page-generation-scoped: rapid pagination must not lose this total.
      if (acceptsCount(countKey)) {
        state.total = total;
        state.countStatus = 'exact';
        state.countError = null;
        emit();
      }
    }).catch((error) => {
      // Failures are not cached (and are never interpreted as zero).
      if (acceptsCount(countKey)) {
        state.total = null;
        state.countStatus = 'unavailable';
        state.countError = error;
        emit();
      }
    }).finally(() => {
      countInFlight = null;
      // Retry failures only on a new schedule, not in a completion loop. A
      // different current filter may proceed, but only once its rows are ready
      // (or its non-search request has started).
      if (active && current?.countKey !== countKey) ensureCount(current);
    });
  }
  function cancelTimer() {
    if (timer !== null) clearTimer(timer);
    timer = null;
    pendingKey = null;
  }
  function start(request, version) {
    if (!active || version !== generation) return;
    cancelTimer();
    request.countReady = !request.search;
    ensureCount(request);
    let promise = pagesInFlight.get(request.pageKey);
    if (!promise) {
      promise = call('get_alpha_results', request.params);
      pagesInFlight.set(request.pageKey, promise);
      const cleanup = () => pagesInFlight.delete(request.pageKey);
      promise.then(cleanup, cleanup);
    }
    promise.then((result) => {
      if (!active || version !== generation) return;
      state.rows = result.data;
      state.hasRows = true;
      state.page = result.page;
      state.pageSize = result.page_size;
      state.loading = false;
      if (request.ra) {
        state.total = result.total;
        state.countStatus = 'exact';
      }
      // Non-RA result.total is a placeholder; only the separate count is authoritative.
      emit();
      if (request.search) {
        request.countReady = true;
        ensureCount(request);
      }
    }).catch((error) => {
      if (!active || version !== generation) return;
      state.loading = false;
      state.error = error;
      emit();
    });
  }

  function schedule(params, { immediate = false } = {}) {
    if (!active) return;
    const frozen = snapshot(params);
    const ra = isRaQuery(frozen);
    if (ra) delete frozen.include_total; // Preserve the existing RA facet path in full.
    else frozen.include_total = false;
    const pageKey = keyOf(frozen);
    if (pendingKey === pageKey && !immediate) return;
    if (current?.pageKey === pageKey && pagesInFlight.has(pageKey) && pendingKey === null
        && state.countStatus !== 'unavailable') return;
    cancelTimer();
    // Invalidate immediately, before the debounce timer fires (including stale errors).
    const version = ++generation;
    current = {
      params: frozen, ra, pageKey, countKey: alphaCountKey(frozen),
      // Scheduling only: do not normalize or otherwise change matching inputs.
      search: Boolean(frozen.query || frozen.exclude_query), countReady: false,
    };
    const entry = ra ? null : cachedCount(current.countKey);
    state = {
      rows: [], hasRows: false, loading: true, error: null,
      page: frozen.page || 1, pageSize: frozen.page_size || 20,
      total: entry?.total ?? null,
      countStatus: entry ? 'exact' : 'pending', countError: null,
    };
    emit();
    if (immediate) start(current, version);
    else {
      const request = current;
      pendingKey = pageKey;
      timer = setTimer(() => start(request, version), debounceMs);
    }
  }
  function dispose() {
    active = false;
    generation++;
    cancelTimer();
    pagesInFlight.clear();
    countInFlight = null;
    countCache.clear();
  }
  return { schedule, dispose, getGeneration: () => generation };
}
