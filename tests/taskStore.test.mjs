import test from 'node:test'
import assert from 'node:assert/strict'
import { createPinia, setActivePinia } from 'pinia'
import { watch } from 'vue'
import { useTaskStore } from '../src/stores/taskStore.js'
import {
  createTaskFetcher, createTaskProgressLifecycle,
  syncTaskToRemote, mutateTaskSource,
} from '../src/utils/taskProgress.js'

const task = (overrides = {}) => ({ _id: 'id-a', name: 'alpha', isRemote: false, status: 'running', ...overrides })
const progress = (overrides = {}) => ({ collection: 'alpha', is_remote: false, success: 7, error: 2,
  total: 10, priority_success: 1, priority_error: 1, priority_total: 3, ...overrides })
const failure = (overrides = {}) => ({ collection: 'alpha', is_remote: false, error: 'offline', updated_at: 200, ...overrides })
const freshStore = (tasks = [task()]) => {
  setActivePinia(createPinia())
  const store = useTaskStore()
  store.replaceTasks(tasks)
  return store
}
const deferred = () => {
  let resolve, reject
  const promise = new Promise((res, rej) => { resolve = res; reject = rej })
  return { promise, resolve, reject }
}
const tick = async () => { for (let i = 0; i < 6; i++) await Promise.resolve() }

// These exercise the actual Pinia/Vue store, not a replacement implementation.
test('initialization follows each task source and starts stale', () => {
  const store = freshStore([task(), task({ name: 'remote', isRemote: true })])
  assert.equal(store.taskProgressMap.alpha.is_remote, false)
  assert.equal(store.taskProgressMap.remote.is_remote, true)
  assert.equal(store.taskProgressMap.remote.stale, true)
  store.initializeProgressForTasks(store.tasks)
  assert.equal(store.taskProgressMap.remote.is_remote, true)
})

test('backing snapshot is registered as actual Pinia state', () => {
  const store = freshStore()
  assert.equal(store.$state.state.tasks[0].name, 'alpha')
  store.updateProgress(progress())
  assert.equal(store.$state.state.progress.alpha.success, 7)
})

test('success requires a current task, matching boolean source and valid counts', () => {
  const store = freshStore()
  for (const event of [null, {}, progress({ collection: 'deleted' }), progress({ is_remote: true }),
    progress({ is_remote: null }), progress({ is_remote: undefined }), progress({ success: NaN }),
    progress({ total: -1 }), progress({ priority_error: '1' })]) {
    assert.equal(store.updateProgress(event), false)
  }
  assert.equal(store.updateProgress(progress()), true)
  assert.equal(store.taskProgressMap.alpha.success, 7)
  assert.equal(store.taskProgressMap.alpha.stale, false)
})

test('failures preserve all last successful counts and mark stale; newer success recovers', () => {
  const store = freshStore()
  store.updateProgress(progress({ updated_at: 100 }))
  const old = { ...store.taskProgressMap.alpha }
  assert.equal(store.updateProgressError(failure()), true)
  for (const key of ['success', 'error', 'total', 'priority_success', 'priority_error', 'priority_total']) {
    assert.equal(store.taskProgressMap.alpha[key], old[key])
  }
  assert.equal(store.taskProgressMap.alpha.stale, true)
  assert.equal(store.taskProgressMap.alpha.lastError, 'offline')
  assert.equal(store.updateProgress(progress({ updated_at: 201, success: 8 })), true)
  assert.equal(store.taskProgressMap.alpha.stale, false)
  assert.equal(store.taskProgressMap.alpha.lastError, null)
})

test('both success and failure reject older timestamps across either event type', () => {
  const store = freshStore()
  store.updateProgress(progress({ updated_at: 300 }))
  assert.equal(store.updateProgress(progress({ updated_at: 299, success: 0 })), false)
  assert.equal(store.updateProgressError(failure({ updated_at: 299 })), false)
  assert.equal(store.updateProgressError(failure({ updated_at: 400 })), true)
  assert.equal(store.updateProgress(progress({ updated_at: 399 })), false)
  assert.equal(store.updateProgressError(failure({ updated_at: 399 })), false)
  assert.equal(store.taskProgressMap.alpha.updated_at, 400)
  assert.equal(store.taskProgressMap.alpha.stale, true)
})

test('legacy success accepted until versioned event; invalid timestamps rejected', () => {
  const store = freshStore()
  assert.equal(store.updateProgress(progress()), true)
  for (const timestamp of [null, '100', 1.5, -1, NaN, Infinity, Number.MAX_SAFE_INTEGER + 1]) {
    assert.equal(store.updateProgress(progress({ updated_at: timestamp })), false)
  }
  assert.equal(store.updateProgress(progress({ updated_at: 0 })), true)
  assert.equal(store.updateProgress(progress()), false)
  assert.equal(store.updateProgressError(failure({ updated_at: undefined })), false)
})

test('unknown-source failure accepted, wrong-source failure rejected without mutation', () => {
  const store = freshStore([task({ isRemote: true })])
  assert.equal(store.updateProgressError(failure()), false)
  assert.equal(store.updateProgressError(failure({ is_remote: null })), true)
  assert.equal(store.taskProgressMap.alpha.is_remote, true)
  assert.equal(store.taskProgressMap.alpha.success, 0)
  assert.equal(store.taskProgressMap.alpha.lastError, 'offline')
  assert.equal(store.updateProgressError(failure({ is_remote: 'true' })), false)
  assert.equal(store.updateProgressError(failure({ is_remote: null, error: 9 })), false)
})

test('source changes invalidate counts and retain ordering; unchanged snapshots preserve progress', () => {
  const store = freshStore()
  store.updateProgress(progress({ updated_at: 300 }))
  store.replaceTasks([task({ status: 'waiting' })])
  assert.equal(store.taskProgressMap.alpha.success, 7)
  store.setTaskSource('alpha', true)
  assert.equal(store.taskProgressMap.alpha.success, 0)
  assert.equal(store.taskProgressMap.alpha.is_remote, true)
  assert.equal(store.taskProgressMap.alpha.stale, true)
  assert.equal(store.updateProgress(progress({ updated_at: 400 })), false)
  assert.equal(store.updateProgress(progress({ is_remote: true, updated_at: 299 })), false)
  assert.equal(store.updateProgress(progress({ is_remote: true, updated_at: 301 })), true)
  store.setTaskSource('alpha', false)
  assert.equal(store.updateProgress(progress({ updated_at: 300 })), false)
})

test('same-name new task does not inherit completed progress from the old identity', () => {
  const store = freshStore([task({ _id: { $oid: 'old-id' } })])
  store.updateProgress(progress({ success: 10, updated_at: 100 }))
  store.replaceTasks([task({ _id: { $oid: 'old-id' } })])
  assert.equal(store.taskProgressMap.alpha.success, 10)
  store.replaceTasks([task({ _id: { $oid: 'new-id' } })])
  assert.equal(store.taskProgressMap.alpha.success, 0)
  assert.equal(store.taskProgressMap.alpha.stale, true)
  assert.equal(store.tasks[0].status, 'running') // counts are not lifecycle authority
})

test('atomic replacement never publishes tasks with the wrong progress source', () => {
  const store = freshStore()
  const snapshots = []
  const stop = watch(() => [store.tasks, store.taskProgressMap], () => {
    snapshots.push(store.tasks.map((item) => [item.isRemote, store.taskProgressMap[item.name].is_remote]))
  }, { flush: 'sync' })
  store.replaceTasks([task({ isRemote: true })])
  stop()
  assert.deepEqual(snapshots, [[[true, true]]])
})

test('deletion and clear prune cache; late deleted events cannot recreate entries', () => {
  const store = freshStore([task(), task({ name: 'other' })])
  store.updateProgress(progress({ updated_at: 100 }))
  store.removeTask('alpha')
  assert.equal(store.taskProgressMap.alpha, undefined)
  assert.equal(store.updateProgress(progress({ updated_at: 500 })), false)
  assert.equal(store.updateProgressError(failure()), false)
  store.clearTasks()
  assert.deepEqual(store.tasks, [])
  assert.deepEqual(store.taskProgressMap, {})
  store.addTask(task({ isRemote: true }))
  assert.equal(store.taskProgressMap.alpha.is_remote, true)
})

function fetchHarness() {
  const requests = [], replacements = [], errors = [], refreshErrors = []
  let active = true, refreshCount = 0
  const refreshResult = deferred()
  const fetch = createTaskFetcher({
    load: () => { const request = deferred(); requests.push(request); return request.promise },
    replace: (tasks) => replacements.push(tasks),
    refresh: () => { refreshCount++; return refreshResult.promise },
    isActive: () => active, onError: (error) => errors.push(error),
    onRefreshError: (error) => refreshErrors.push(error),
  })
  return { fetch, requests, replacements, errors, refreshErrors, refreshResult,
    deactivate: () => { active = false }, get refreshCount() { return refreshCount } }
}

test('only latest fetch replaces atomically; successful fetch fires one nonblocking refresh', async () => {
  const h = fetchHarness()
  const older = h.fetch(), latest = h.fetch()
  h.requests[1].resolve([task({ isRemote: true }), task({ name: 'hidden', status: 'deactive' })])
  assert.equal(await latest, true) // refreshResult remains pending
  h.requests[0].resolve([task()])
  assert.equal(await older, false)
  assert.equal(h.replacements.length, 1)
  assert.equal(h.replacements[0].length, 1)
  assert.equal(h.replacements[0][0].isRemote, true)
  assert.equal(h.refreshCount, 1)
  h.refreshResult.reject(new Error('refresh failed'))
  await tick()
  assert.equal(h.errors.length, 0)
  assert.equal(h.refreshErrors.length, 1)
})

test('older fetch may not replace even if it resolves first; obsolete errors are silent', async () => {
  const h = fetchHarness()
  const older = h.fetch(), latest = h.fetch()
  h.requests[0].reject(new Error('obsolete'))
  assert.equal(await older, false)
  assert.equal(h.errors.length, 0)
  h.requests[1].reject(new Error('latest'))
  assert.equal(await latest, false)
  assert.equal(h.errors.length, 1)
  assert.equal(h.refreshCount, 0)
  const h2 = fetchHarness()
  const first = h2.fetch(), second = h2.fetch()
  h2.requests[0].resolve([task()])
  assert.equal(await first, false)
  assert.equal(h2.replacements.length, 0)
  h2.requests[1].resolve([])
  assert.equal(await second, true)
})

test('fetch completing after unmount cannot replace, refresh or display errors', async () => {
  const h = fetchHarness()
  const request = h.fetch()
  h.deactivate()
  h.requests[0].resolve([task()])
  assert.equal(await request, false)
  assert.equal(h.refreshCount, 0)
  assert.equal(h.replacements.length, 0)
  assert.equal(await h.fetch(), false)
  assert.equal(h.requests.length, 1)
})

function lifecycleHarness() {
  const subscriptions = new Map(), events = [], unlistened = [], intervals = [], cleared = []
  const ready = deferred(), fetched = deferred()
  const lifecycle = createTaskProgressLifecycle({
    listen: (name, callback) => {
      const result = deferred(); subscriptions.set(name, { ...result, callback }); return result.promise
    },
    ready: () => { events.push('ready'); return ready.promise },
    fetch: () => { events.push('fetch'); return fetched.promise },
    poll: () => events.push('poll'),
    onProgress: (payload) => events.push(['success', payload]),
    onError: (payload) => events.push(['failure', payload]),
    onRefreshError: (payload) => events.push(['discovery-failure', payload]),
    setIntervalFn: (callback, delay) => { intervals.push({ callback, delay }); return 123 },
    clearIntervalFn: (id) => cleared.push(id),
  })
  const resolveListener = (name) => subscriptions.get(name).resolve(() => unlistened.push(name))
  return { lifecycle, subscriptions, events, unlistened, intervals, cleared, ready, fetched, resolveListener }
}
const SUCCESS = 'task-progress-update', ERROR = 'task-progress-error', REFRESH_ERROR = 'task-progress-refresh-error'

test('all three listeners registered before ready, then fetch and polling; stop removes all', async () => {
  const h = lifecycleHarness()
  const starting = h.lifecycle.start()
  assert.equal(h.subscriptions.size, 3)
  h.resolveListener(SUCCESS)
  await tick()
  assert.deepEqual(h.events, [])
  h.resolveListener(ERROR)
  await tick()
  assert.deepEqual(h.events, [])
  h.resolveListener(REFRESH_ERROR)
  await tick()
  assert.deepEqual(h.events, ['ready'])
  h.subscriptions.get(SUCCESS).callback({ payload: 'progress' })
  h.subscriptions.get(ERROR).callback({ payload: 'offline' })
  h.subscriptions.get(REFRESH_ERROR).callback({ payload: 'discovery failed' })
  assert.deepEqual(h.events.slice(1), [
    ['success', 'progress'], ['failure', 'offline'], ['discovery-failure', 'discovery failed'],
  ])
  h.ready.resolve()
  await tick()
  assert.equal(h.events.at(-1), 'fetch')
  h.fetched.resolve()
  await starting
  assert.equal(h.events.at(-1), 'poll')
  assert.equal(h.intervals[0].delay, 600000)
  h.lifecycle.stop()
  assert.deepEqual(h.unlistened.sort(), [ERROR, SUCCESS, REFRESH_ERROR].sort())
  assert.deepEqual(h.cleared, [123])
  const count = h.events.length
  h.subscriptions.get(SUCCESS).callback({ payload: 'late' })
  h.subscriptions.get(ERROR).callback({ payload: 'late' })
  h.subscriptions.get(REFRESH_ERROR).callback({ payload: 'late' })
  assert.equal(h.events.length, count)
})

test('unmount while listener promises are pending immediately disposes late subscriptions', async () => {
  const h = lifecycleHarness()
  const starting = h.lifecycle.start()
  h.lifecycle.stop()
  h.resolveListener(SUCCESS)
  h.resolveListener(ERROR)
  h.resolveListener(REFRESH_ERROR)
  await starting
  assert.equal(h.unlistened.length, 3)
  assert.deepEqual(h.events, [])
  assert.deepEqual(h.intervals, [])
})

test('unmount during frontend_ready prevents fetch and interval', async () => {
  const h = lifecycleHarness()
  const starting = h.lifecycle.start()
  h.resolveListener(SUCCESS); h.resolveListener(ERROR); h.resolveListener(REFRESH_ERROR)
  await tick()
  h.lifecycle.stop()
  h.ready.resolve()
  await starting
  assert.deepEqual(h.events, ['ready'])
  assert.equal(h.intervals.length, 0)
})

test('unmount during initial fetch prevents polling and interval creation', async () => {
  const h = lifecycleHarness()
  const starting = h.lifecycle.start()
  h.resolveListener(SUCCESS); h.resolveListener(ERROR); h.resolveListener(REFRESH_ERROR)
  h.ready.resolve()
  await tick()
  assert.deepEqual(h.events, ['ready', 'fetch'])
  h.lifecycle.stop()
  h.fetched.resolve()
  await starting
  assert.deepEqual(h.events, ['ready', 'fetch'])
  assert.equal(h.intervals.length, 0)
  assert.equal(h.unlistened.length, 3)
})

test('listener registration failure cleans already registered and late listeners', async () => {
  const h = lifecycleHarness()
  const starting = h.lifecycle.start()
  h.subscriptions.get(SUCCESS).reject(new Error('registration failed'))
  await assert.rejects(starting, /registration failed/)
  h.resolveListener(ERROR)
  h.resolveListener(REFRESH_ERROR)
  await tick()
  assert.deepEqual(h.unlistened.sort(), [ERROR, REFRESH_ERROR].sort())
  assert.deepEqual(h.events, [])
})

test('third listener registration failure removes already installed success/error listeners', async () => {
  const h = lifecycleHarness()
  const starting = h.lifecycle.start()
  h.resolveListener(SUCCESS); h.resolveListener(ERROR)
  await tick()
  h.subscriptions.get(REFRESH_ERROR).reject(new Error('refresh listener failed'))
  await assert.rejects(starting, /refresh listener failed/)
  assert.deepEqual(h.unlistened.sort(), [SUCCESS, ERROR].sort())
  assert.deepEqual(h.events, [])
  assert.deepEqual(h.intervals, [])
})

// R01 retires the count-completion scheduler and its implementation-only tests.
// Before/why is preserved in the task implementation/before snapshot. Actual
// SFC timer absence and manual pause/queued-pause are tested in taskManagerPause.
test('full success remains display data; progress/error/recovery never changes task state', () => {
  const store = freshStore()
  store.updateProgress(progress({ success: 10, updated_at: 100 }))
  assert.equal(store.taskProgressMap.alpha.success, 10)
  assert.equal(store.tasks[0].status, 'running')
  assert.equal(store.updateProgressError(failure({ updated_at: 99 })), false)
  assert.equal(store.taskProgressMap.alpha.stale, false)
  assert.equal(store.updateProgressError(failure()), true)
  assert.equal(store.taskProgressMap.alpha.success, 10)
  assert.equal(store.taskProgressMap.alpha.stale, true)
  assert.equal(store.updateProgress(progress({ updated_at: 199, success: 10 })), false)
  assert.equal(store.updateProgress(progress({ updated_at: 201, success: 10 })), true)
  assert.equal(store.taskProgressMap.alpha.stale, false)
  assert.equal(store.tasks[0].status, 'running')
  assert.deepEqual(store.taskStopOperationMap, {})
  store.removeTask('alpha')
  assert.equal(store.taskProgressMap.alpha, undefined)
})

test('full counts do not override registry status, identity or execution source', () => {
  const store = freshStore()
  store.updateProgress(progress({ success: 10, updated_at: 100 }))
  store.replaceTasks([task({ status: 'completed' })])
  assert.equal(store.tasks[0].status, 'completed')
  assert.equal(store.taskProgressMap.alpha.success, 10)
  store.replaceTasks([task({ status: 'running' })])
  assert.equal(store.tasks[0].status, 'running')
  store.setTaskSource('alpha', true)
  assert.equal(store.taskProgressMap.alpha.success, 0)
  assert.equal(store.taskProgressMap.alpha.stale, true)
  store.updateProgress(progress({ is_remote: true, success: 10, updated_at: 101 }))
  assert.equal(store.taskProgressMap.alpha.success, 10)
  store.replaceTasks([task({ _id: 'new-id', isRemote: true })])
  assert.equal(store.taskProgressMap.alpha.success, 0)
  assert.equal(store.tasks[0].status, 'running')
  assert.deepEqual(store.taskStopOperationMap, {})
})

test('sync awaits sync -> source update -> exactly one refresh', async () => {
  const calls = [], synced = deferred(), updated = deferred()
  const result = syncTaskToRemote({
    sync: () => { calls.push('sync'); return synced.promise },
    updateSource: () => { calls.push('update'); return updated.promise },
    refresh: () => calls.push('refresh'),
  })
  assert.deepEqual(calls, ['sync'])
  synced.resolve(); await tick()
  assert.deepEqual(calls, ['sync', 'update'])
  updated.resolve(); await result
  assert.deepEqual(calls, ['sync', 'update', 'refresh'])
})

test('sync and source-update failures propagate and do not refresh', async () => {
  for (const failing of ['sync', 'update']) {
    const calls = []
    await assert.rejects(syncTaskToRemote({
      sync: async () => { calls.push('sync'); if (failing === 'sync') throw new Error('sync failed') },
      updateSource: async () => { calls.push('update'); if (failing === 'update') throw new Error('update failed') },
      refresh: () => calls.push('refresh'),
    }), new RegExp(`${failing} failed`))
    assert.deepEqual(calls, failing === 'sync' ? ['sync'] : ['sync', 'update'])
  }
})

test('success and failure require matching task_id when supplied; legacy identity stays compatible', () => {
  const store = freshStore([task({ _id: { $oid: 'id-a' } })])
  for (const identity of ['old-id', null, 42, {}, true]) {
    assert.equal(store.updateProgress(progress({ task_id: identity, updated_at: 100 })), false)
  }
  assert.equal(store.updateProgress(progress({ task_id: 'id-a', updated_at: 100 })), true)
  for (const identity of ['old-id', 42, {}, true]) {
    assert.equal(store.updateProgressError(failure({ task_id: identity })), false)
  }
  assert.equal(store.taskProgressMap.alpha.stale, false)
  assert.equal(store.updateProgressError(failure({ task_id: 'id-a' })), true)
  assert.equal(store.updateProgress(progress({ updated_at: 201 })), true) // missing task_id is legacy
  assert.equal(store.updateProgressError(failure({ updated_at: 202 })), true)
})

test('metadata errors with null identity may mark stale but keep counters and source guards', () => {
  const store = freshStore()
  store.updateProgress(progress({ task_id: 'id-a', updated_at: 100 }))
  assert.equal(store.updateProgressError(failure({ task_id: null, is_remote: true })), false)
  assert.equal(store.updateProgressError(failure({ task_id: null, is_remote: null })), true)
  assert.equal(store.taskProgressMap.alpha.success, 7)
  assert.equal(store.taskProgressMap.alpha.error, 2)
  assert.equal(store.taskProgressMap.alpha.is_remote, false)
  assert.equal(store.taskProgressMap.alpha.stale, true)
  assert.equal(store.updateProgressError(failure({ task_id: null, updated_at: 199 })), false)
})

test('in-flight old task responses cannot contaminate a same-name recreated task, even with newer versions', () => {
  const store = freshStore([task({ _id: { $oid: 'old-id' } })])
  store.updateProgress(progress({ task_id: 'old-id', success: 10, updated_at: 100 }))
  store.replaceTasks([task({ _id: { $oid: 'new-id' } })])
  assert.equal(store.updateProgress(progress({ task_id: 'old-id', success: 10, updated_at: 500 })), false)
  assert.equal(store.updateProgressError(failure({ task_id: 'old-id', updated_at: 600 })), false)
  assert.equal(store.taskProgressMap.alpha.success, 0)
  assert.equal(store.taskProgressMap.alpha.lastError, null)
  assert.equal(store.taskProgressMap.alpha.updated_at, 100)
  assert.equal(store.updateProgress(progress({ task_id: 'new-id', updated_at: 200 })), true)
  assert.equal(store.taskProgressMap.alpha.success, 7)
  store.removeTask('alpha')
  store.addTask(task({ _id: 'third-id' }))
  assert.equal(store.updateProgress(progress({ task_id: 'new-id', updated_at: 700 })), false)
  assert.equal(store.updateProgressError(failure({ task_id: 'old-id', updated_at: 800 })), false)
})

const discoveryFailure = (overrides = {}) => ({ scope: 'all', error: 'discovery offline', updated_at: 200, ...overrides })
const counterKeys = ['success', 'error', 'total', 'priority_success', 'priority_error', 'priority_total']

test('remote discovery failure marks only current remote tasks stale, preserving all six counters', () => {
  const store = freshStore([task(), task({ _id: 'id-remote', name: 'remote', isRemote: true })])
  store.updateProgress(progress({ updated_at: 100 }))
  store.updateProgress(progress({ collection: 'remote', is_remote: true, task_id: 'id-remote', updated_at: 101 }))
  const local = { ...store.taskProgressMap.alpha }, remote = { ...store.taskProgressMap.remote }
  assert.equal(store.markProgressStale(discoveryFailure({ scope: 'remote' })), true)
  assert.deepEqual({ ...store.taskProgressMap.alpha }, local)
  for (const key of counterKeys) assert.equal(store.taskProgressMap.remote[key], remote[key])
  assert.equal(store.taskProgressMap.remote.stale, true)
  assert.equal(store.taskProgressMap.remote.lastError, 'discovery offline')
  assert.equal(store.taskProgressMap.remote.updated_at, 200)
  store.setTaskSource('alpha', true)
  store.setTaskSource('remote', false)
  assert.equal(store.markProgressStale(discoveryFailure({ scope: 'remote', updated_at: 201 })), true)
  assert.equal(store.taskProgressMap.alpha.updated_at, 201)
  assert.equal(store.taskProgressMap.remote.updated_at, 200)
})

test('all-scope discovery failures share each task version watermark with successes and task errors', () => {
  const store = freshStore([task(), task({ name: 'remote', isRemote: true }), task({ name: 'uninitialized' })])
  store.updateProgress(progress({ updated_at: 300 }))
  store.updateProgress(progress({ collection: 'remote', is_remote: true, updated_at: 100 }))
  const remote = { ...store.taskProgressMap.remote }
  store.markProgressStale(discoveryFailure())
  assert.equal(store.taskProgressMap.alpha.stale, false) // newer success wins
  assert.equal(store.taskProgressMap.alpha.updated_at, 300)
  for (const key of counterKeys) assert.equal(store.taskProgressMap.remote[key], remote[key])
  assert.equal(store.taskProgressMap.remote.stale, true)
  assert.equal(store.taskProgressMap.uninitialized.stale, true)
  assert.equal(store.taskProgressMap.uninitialized.lastError, 'discovery offline')
  assert.equal(store.updateProgress(progress({ collection: 'remote', is_remote: true, updated_at: 199 })), false)
  assert.equal(store.updateProgressError(failure({ collection: 'remote', is_remote: true, updated_at: 199 })), false)
  assert.equal(store.updateProgress(progress({ collection: 'remote', is_remote: true, updated_at: 201 })), true)
  assert.equal(store.taskProgressMap.remote.stale, false)
  assert.equal(store.taskProgressMap.remote.lastError, null)
  assert.equal(store.updateProgressError(failure({ updated_at: 400 })), true)
  store.markProgressStale(discoveryFailure({ updated_at: 350 }))
  assert.equal(store.taskProgressMap.alpha.lastError, 'offline')
  assert.equal(store.taskProgressMap.alpha.updated_at, 400)
})

test('invalid discovery failures and empty remote scopes do not change the store', () => {
  const store = freshStore()
  const original = store.state
  for (const event of [null, {}, discoveryFailure({ scope: 'local' }), discoveryFailure({ error: 123 }),
    discoveryFailure({ updated_at: undefined }), discoveryFailure({ updated_at: null }),
    discoveryFailure({ updated_at: 1.5 }), discoveryFailure({ updated_at: -1 }), discoveryFailure({ updated_at: Infinity })]) {
    assert.equal(store.markProgressStale(event), false)
    assert.equal(store.state, original)
  }
  assert.equal(store.markProgressStale(discoveryFailure({ scope: 'remote' })), false)
  store.clearTasks()
  assert.equal(store.markProgressStale(discoveryFailure()), false)
  assert.deepEqual(store.taskProgressMap, {})
})

test('discovery failure preserves full remote counts without changing task state', () => {
  const store = freshStore()
  store.setTaskSource('alpha', true)
  store.updateProgress(progress({ is_remote: true, success: 10, updated_at: 100 }))
  store.markProgressStale(discoveryFailure({ scope: 'remote' }))
  assert.equal(store.taskProgressMap.alpha.success, 10)
  assert.equal(store.taskProgressMap.alpha.stale, true)
  assert.equal(store.tasks[0].status, 'running')
  assert.deepEqual(store.taskStopOperationMap, {})
})

test('fetch invalidate discards all in-flight successes/errors and permits a fresh request', async () => {
  const h = fetchHarness()
  const oldSuccess = h.fetch(), oldError = h.fetch()
  h.fetch.invalidate()
  h.requests[0].resolve([task()])
  h.requests[1].reject(new Error('old fetch error'))
  assert.equal(await oldSuccess, false)
  assert.equal(await oldError, false)
  assert.equal(h.replacements.length, 0)
  assert.equal(h.errors.length, 0)
  assert.equal(h.refreshCount, 0)
  const fresh = h.fetch()
  h.requests[2].resolve([task({ isRemote: true })])
  assert.equal(await fresh, true)
  assert.equal(h.refreshCount, 1)
})

test('source mutation invalidates old snapshots both during command and after its source commit', async () => {
  const store = freshStore(), requests = [], command = deferred()
  let refreshCount = 0
  const fetch = createTaskFetcher({
    load: () => { const request = deferred(); requests.push(request); return request.promise },
    replace: (tasks) => store.replaceTasks(tasks), refresh: () => { refreshCount++ },
    isActive: () => true, onError: assert.fail, onRefreshError: assert.fail,
  })
  const old = fetch()
  const mutation = mutateTaskSource({ fetch, update: () => command.promise,
    commit: () => store.setTaskSource('alpha', true) })
  requests[0].resolve([task({ status: 'waiting' })])
  assert.equal(await old, false)
  assert.equal(store.tasks[0].status, 'running') // old snapshot may not replace while mutation is pending
  const during = fetch()
  command.resolve()
  await mutation
  assert.equal(store.tasks[0].isRemote, true)
  requests[1].resolve([task({ isRemote: false })])
  assert.equal(await during, false)
  assert.equal(store.tasks[0].isRemote, true)
  assert.equal(store.taskProgressMap.alpha.is_remote, true)
  assert.equal(refreshCount, 0)
  const fresh = fetch()
  requests[2].resolve([task({ isRemote: true })])
  assert.equal(await fresh, true)
  assert.equal(refreshCount, 1)
})

test('failed source mutation propagates, skips commit and invalidates reads started during command', async () => {
  const h = fetchHarness(), command = deferred()
  let committed = false
  const mutation = mutateTaskSource({ fetch: h.fetch, update: () => command.promise,
    commit: () => { committed = true } })
  const during = h.fetch()
  command.reject(new Error('source mutation failed'))
  await assert.rejects(mutation, /source mutation failed/)
  h.requests[0].resolve([task({ isRemote: true })])
  assert.equal(await during, false)
  assert.equal(committed, false)
  assert.equal(h.replacements.length, 0)
  assert.equal(h.refreshCount, 0)
})

const campaign = (overrides = {}) => ({ collection: 'alpha', task_id: 'id-a', is_remote: false,
  updated_at: 100, auth_profile: 'user1', campaign_id: 'campaign-a', status: 'blocked',
  reason: 'feedback_incomplete_or_wait_timeout', current_round: 1, max_rounds: 3,
  max_per_round: 60, reserved: 60, unresolved: 60, data_source: 'ledger',
  stats_confirmed: true, stale: false, last_error: null,
  counts: { success: 999, error: 999, total: 999 }, ...overrides });
const campaignProgress = (overrides = {}) => progress({ task_id: 'id-a', updated_at: 100,
  success: 0, error: 0, total: 60, priority_total: 60, priority_success: 0, priority_error: 0,
  priority_campaign: campaign(), ...overrides });

test('priority campaign cache retains real blocked round metadata and only actual flat counters', () => {
  const store = freshStore();
  assert.equal(store.updateProgress(campaignProgress()), true);
  const cached = store.taskProgressMap.alpha;
  assert.equal(cached.total, 60);
  assert.equal(cached.success, 0);
  assert.equal(cached.error, 0);
  assert.equal(cached.priority_campaign.status, 'blocked');
  assert.equal(cached.priority_campaign.reason, 'feedback_incomplete_or_wait_timeout');
  assert.equal(cached.priority_campaign.current_round, 1);
  assert.equal(cached.priority_campaign.max_rounds, 3);
  assert.equal(cached.priority_campaign.max_per_round, 60);
  assert.equal(cached.priority_campaign.counts.total, 60); // not nested fake 999 or summary budget
  assert.equal(cached.priority_campaign.counts.error, 0);
  store.replaceTasks([task()]);
  assert.equal(store.taskProgressMap.alpha.priority_campaign.status, 'blocked');
});

test('priority metadata must match active task identity/source/version independently of outer event', () => {
  const store = freshStore();
  store.updateProgress(campaignProgress());
  for (const override of [{ task_id: 'id-old' }, { collection: 'other' }, { is_remote: true },
    { updated_at: 99 }, { updated_at: undefined }]) {
    assert.equal(store.updateProgress(campaignProgress({ priority_campaign: campaign(override) })), false);
  }
  for (const invalid of [null, [], 'bad']) {
    assert.equal(store.updateProgress(campaignProgress({ priority_campaign: invalid })), false);
  }
  assert.equal(store.taskProgressMap.alpha.priority_campaign.counts.total, 60);
});

test('priority failures preserve same-campaign actual counts and do not turn reserved/UNKNOWN into failures', () => {
  const store = freshStore();
  store.updateProgress(campaignProgress());
  assert.equal(store.updateProgressError(failure({ task_id: 'id-a', updated_at: 101,
    priority_campaign: campaign({ updated_at: 101, stats_confirmed: false, counts: null,
      stale: true, last_error: 'ledger read failed', reserved: 180 }) })), true);
  const cached = store.taskProgressMap.alpha;
  assert.equal(cached.stale, true);
  assert.equal(cached.total, 60);
  assert.equal(cached.error, 0);
  assert.equal(cached.priority_campaign.counts.total, 60);
  assert.equal(cached.priority_campaign.counts.error, 0);
  assert.equal(cached.priority_campaign.reserved, 180);
  assert.equal(cached.priority_campaign.unresolved, 60);
  assert.equal(store.updateProgressError(failure({ updated_at: 102 })), true);
  assert.equal(store.taskProgressMap.alpha.priority_campaign.counts.total, 60);
});

test('metadata read errors keep the last campaign; new campaigns never reuse old actual counts', () => {
  const store = freshStore();
  store.updateProgress(campaignProgress());
  store.updateProgressError(failure({ updated_at: 101, priority_campaign: campaign({ updated_at: 101,
    campaign_id: null, status: null, current_round: null, stats_confirmed: false, counts: null,
    last_error: 'registry timeout' }) }));
  assert.equal(store.taskProgressMap.alpha.priority_campaign.campaign_id, 'campaign-a');
  assert.equal(store.taskProgressMap.alpha.priority_campaign.status, 'blocked');
  assert.equal(store.taskProgressMap.alpha.priority_campaign.current_round, 1);
  store.updateProgressError(failure({ updated_at: 102, priority_campaign: campaign({ updated_at: 102,
    campaign_id: 'campaign-b', stats_confirmed: false, counts: null, last_error: 'ledger unready' }) }));
  assert.equal(store.taskProgressMap.alpha.priority_campaign.counts, null);
});

test('source changes and same-name replacements cannot reuse campaign metadata', () => {
  const store = freshStore();
  store.updateProgress(campaignProgress());
  store.setTaskSource('alpha', true);
  assert.equal(store.taskProgressMap.alpha.priority_campaign, undefined);
  assert.equal(store.updateProgress(campaignProgress()), false);
  store.replaceTasks([task({ _id: 'id-new' })]);
  assert.equal(store.taskProgressMap.alpha.priority_campaign, undefined);
});


test('complete intermediate priority round remains display data, not a task state transition', () => {
  const priority = task({ taskType: 'priority', mission_config: { rounds: { enabled: true } } })
  const store = freshStore([priority])
  assert.equal(store.updateProgress(campaignProgress({ success: 60, total: 60 })), true)
  assert.equal(store.taskProgressMap.alpha.success, 60)
  assert.equal(store.taskProgressMap.alpha.total, 60)
  assert.equal(store.tasks[0].status, 'running')
  assert.deepEqual(store.taskStopOperationMap, {})
})

test('root pool counts over 180 remain cached only within the same pool identity', () => {
  const store = freshStore();
  const pool = campaign({ campaign_id: null, pool_id: 'pool-a', mode: 'per_root',
    root_count: 4, stage_distribution: { '1': 2, '2': 1, '3': 1 }, waiting_feedback: 2,
    waiting_slots: 1, completed: 1, blocked: 0, source_tasks: ['source-a', 'source-b'], current_round: null });
  store.updateProgress(campaignProgress({ total: 240, priority_total: 240, priority_campaign: pool }));
  assert.equal(store.taskProgressMap.alpha.priority_campaign.counts.total, 240);
  store.updateProgressError(failure({ updated_at: 101, priority_campaign: { ...pool, updated_at: 101,
    pool_id: null, mode: null, root_count: null, stage_distribution: null, source_tasks: [],
    stats_confirmed: false, counts: null, stale: true, last_error: 'registry timeout' } }));
  let cached = store.taskProgressMap.alpha.priority_campaign;
  assert.equal(cached.pool_id, 'pool-a');
  assert.deepEqual(cached.stage_distribution, pool.stage_distribution);
  assert.equal(cached.counts.total, 240);
  store.updateProgressError(failure({ updated_at: 102, priority_campaign: { ...pool, updated_at: 102,
    pool_id: 'pool-b', stats_confirmed: false, counts: null, stale: true, last_error: 'ledger timeout' } }));
  cached = store.taskProgressMap.alpha.priority_campaign;
  assert.equal(cached.pool_id, 'pool-b');
  assert.equal(cached.counts, null);
});

test('selected-host missing registry is not a transport error that can revive legacy global CP', () => {
  for (const is_remote of [false, true]) {
    const store = freshStore([task({ isRemote: is_remote })]);
    store.updateProgress(campaignProgress({ is_remote, priority_campaign: campaign({ is_remote, mode: 'legacy' }) }));
    const missing = campaign({ is_remote, updated_at: 101, campaign_id: null, pool_id: null, mode: 'per_root',
      reason: 'root_pool_not_registered', status: null, current_round: null, stats_confirmed: false,
      counts: null, stale: true, last_error: 'selected host registry missing' });
    assert.equal(store.updateProgressError(failure({ is_remote, updated_at: 101, priority_campaign: missing })), true);
    const runtime = store.taskProgressMap.alpha.priority_campaign;
    assert.equal(runtime.campaign_id, null);
    assert.equal(runtime.pool_id, null);
    assert.equal(runtime.mode, 'per_root');
    assert.equal(runtime.counts, null);
    assert.equal(runtime.reason, 'root_pool_not_registered');
    assert.equal(store.taskProgressMap.alpha.stale, true);
  }
});

const groupedPool = (overrides = {}) => campaign({ campaign_id: null, pool_id: 'pool-a', mode: 'per_root',
  root_count: 144, stage_distribution: { '0': 36, '1': 92, '2': 8, '3': 8 },
  source_tasks: ['glb_spread', 'asi_revision'], source_summaries: [
    { source_task: 'glb_spread', source_remote: false, root_count: 118,
      stage_distribution: { '0': 29, '1': 89, '2': 0, '3': 0 } },
    { source_task: 'asi_revision', source_remote: true, root_count: 26,
      stage_distribution: { '0': 7, '1': 3, '2': 8, '3': 8 } },
  ], ...overrides });

test('source-group cache survives metadata/discovery errors and remount snapshots without mixing counts', () => {
  const store = freshStore();
  const saved = groupedPool();
  store.updateProgress(campaignProgress({ priority_campaign: saved }));
  store.updateProgressError(failure({ updated_at: 101, priority_campaign: groupedPool({ updated_at: 101,
    pool_id: null, source_summaries: null, source_tasks: [], stage_distribution: null, root_count: null,
    stale: true, last_error: 'registry timeout', stats_confirmed: false }) }));
  let cached = store.taskProgressMap.alpha.priority_campaign;
  assert.deepEqual(cached.source_summaries, saved.source_summaries);
  assert.equal(cached.stale, true);
  assert.equal(cached.counts.total, 60); // item counts are not root counts
  store.markProgressStale(discoveryFailure({ scope: 'all', updated_at: 102 }));
  store.replaceTasks([task()]);
  cached = store.taskProgressMap.alpha.priority_campaign;
  assert.deepEqual(cached.source_summaries.map(group => group.root_count), [118, 26]);
  assert.deepEqual(cached.source_summaries.map(group => group.stage_distribution['0']), [29, 7]);
});

test('new or missing pool and selected host/task identity changes cannot inherit old source groups', () => {
  for (const extra of [{ pool_id: 'pool-b' }, { pool_id: null, reason: 'root_pool_not_registered' }]) {
    const store = freshStore();
    store.updateProgress(campaignProgress({ priority_campaign: groupedPool() }));
    store.updateProgressError(failure({ updated_at: 101, priority_campaign: groupedPool({ ...extra, updated_at: 101,
      source_summaries: null, stale: true, last_error: 'unconfirmed', stats_confirmed: false }) }));
    assert.equal(store.taskProgressMap.alpha.priority_campaign.source_summaries, null);
    assert.equal(store.taskProgressMap.alpha.priority_campaign.counts, null);
  }
  for (const change of [store => store.setTaskSource('alpha', true),
    store => store.replaceTasks([task({ _id: 'other-id' })])]) {
    const store = freshStore();
    store.updateProgress(campaignProgress({ priority_campaign: groupedPool() }));
    change(store);
    assert.equal(store.taskProgressMap.alpha.priority_campaign, undefined);
  }
});

test('v6 optional joined-stop launch fence rejects late proof for a successor while old direct calls remain compatible', () => {
  for (const launch of ['joined-launch', null]) {
    const original = task({ launchAttemptId: launch });
    const store = freshStore([original]);
    const target = { ...original, ...(launch === null ? { expectedLaunchAbsent: true } : { expectedLaunchAttemptId: launch }) };
    const operation = store.beginTaskStop(target); assert.ok(operation);
    store.replaceTasks([task({ launchAttemptId: 'successor' })]);
    assert.equal(store.taskStopOperationMap.alpha, undefined);
    assert.equal(store.confirmTaskPaused(target, { confirmed: true, taskId: 'id-a', taskName: 'alpha', isRemote: false, stopConfirmedAt: Date.now() }, operation), false);
    assert.equal(store.tasks[0].status, 'running');
    assert.equal(store.tasks[0].launchAttemptId, 'successor');
  }
  const legacy = freshStore([task({ launchAttemptId: 'current' })]);
  const target = { _id: 'id-a', name: 'alpha', isRemote: false };
  const operation = legacy.beginTaskStop(target); assert.ok(operation);
  assert.equal(legacy.confirmTaskPaused(target, { confirmed: true, taskId: 'id-a', taskName: 'alpha', isRemote: false, stopConfirmedAt: Date.now() }, operation), true);
});

test('v6 explicit absence stop target never interprets malformed launch as missing', () => {
  for (const launch of [false, 0, '', ' ', {}, [], undefined]) {
    const original = task({ launchAttemptId: launch });
    const store = freshStore([original]);
    assert.equal(store.beginTaskStop({ ...original, expectedLaunchAbsent: true }), null);
  }
  for (const original of [task(), task({ launchAttemptId: null })]) {
    const store = freshStore([original]);
    assert.ok(store.beginTaskStop({ ...original, expectedLaunchAbsent: true }));
  }
});

test('loading belongs to latest eligible list request, not obsolete reads or progress work', async () => {
  const reads = [], loading = [], failures = [], progress = deferred()
  const fetch = createTaskFetcher({
    load: () => { const read = deferred(); reads.push(read); return read.promise },
    replace: () => {}, refresh: () => progress.promise, isActive: () => true,
    onError: error => failures.push(error), onRefreshError: error => failures.push(error),
    onLoadingChange: value => loading.push(value),
  })
  const old = fetch(), current = fetch()
  assert.deepEqual(loading, [true, true])
  reads[0].resolve([]); assert.equal(await old, false)
  assert.deepEqual(loading, [true, true])
  reads[1].resolve([]); assert.equal(await current, true)
  assert.deepEqual(loading, [true, true, false])
  progress.reject(new Error('progress failed')); await tick()
  assert.equal(failures[0].message, 'progress failed')
  assert.deepEqual(loading, [true, true, false])
})

test('invalidation releases loading without cancellation; old completion cannot clear a new owner', async () => {
  const reads = [], loading = [], errors = []
  let active = true
  const fetch = createTaskFetcher({
    load: () => { const read = deferred(); reads.push(read); return read.promise },
    replace: () => {}, refresh: () => {}, isActive: () => active,
    onError: error => errors.push(error), onRefreshError: () => {},
    onLoadingChange: value => loading.push(value),
  })
  const old = fetch(); fetch.invalidate()
  assert.deepEqual(loading, [true, false])
  const latest = fetch()
  reads[0].reject(new Error('obsolete')); assert.equal(await old, false)
  assert.deepEqual(loading, [true, false, true]); assert.deepEqual(errors, [])
  reads[1].reject(new Error('latest')); assert.equal(await latest, false)
  assert.deepEqual(loading, [true, false, true, false]); assert.equal(errors.length, 1)
  const disposed = fetch(); active = false
  fetch.invalidate(); reads[2].resolve([]); assert.equal(await disposed, false)
  assert.deepEqual(loading, [true, false, true, false, true])
  assert.equal(await fetch(), false)
})
